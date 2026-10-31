//! Blocking calls to the Last.fm Web Services API. Run these on worker
//! threads only.
//!
//! See <https://www.last.fm/api>. Write and auth methods are signed with an
//! MD5 of the sorted parameters followed by the API shared secret.

use std::time::Duration;

use serde_json::Value;

use crate::domain::SongInfo;

const API_ROOT: &str = "https://ws.audioscrobbler.com/2.0/";
const AUTH_PAGE: &str = "https://www.last.fm/api/auth/";
/// `auth.getSession` answers this until the user approves the token.
const TOKEN_NOT_AUTHORIZED: i64 = 14;
/// `track.getInfo` answers this for songs Last.fm does not know.
const TRACK_NOT_FOUND: i64 = 6;
/// Last.fm's grey star placeholder, served when an album has no art.
const PLACEHOLDER_IMAGE: &str = "2a96cbd8b46e442fc41c2b86b821562f";

/// Everything needed to scrobble on behalf of the connected user.
#[derive(Clone, Debug)]
pub(crate) struct Credentials {
    pub(crate) api_key: String,
    pub(crate) api_secret: String,
    pub(crate) session_key: String,
}

type Params = Vec<(&'static str, String)>;

/// The `api_sig` for a signed call.
pub(crate) fn sign(params: &[(&str, String)], secret: &str) -> String {
    let mut sorted: Vec<&(&str, String)> = params
        .iter()
        .filter(|(name, _)| *name != "format" && *name != "callback")
        .collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    let mut text = String::new();
    for (name, value) in sorted {
        text.push_str(name);
        text.push_str(value);
    }
    text.push_str(secret);
    format!("{:x}", md5::compute(text.as_bytes()))
}

fn signed(mut params: Params, secret: &str) -> Params {
    let signature = sign(&params, secret);
    params.push(("api_sig", signature));
    params
}

enum ApiError {
    Network(String),
    Api { code: i64, message: String },
}

impl ApiError {
    fn describe(self) -> String {
        match self {
            Self::Network(message) => format!("Could not reach Last.fm: {message}"),
            Self::Api { code: 10, .. } => "Last.fm doesn't recognize that API key".into(),
            Self::Api { code: 13, .. } => {
                "Last.fm rejected the signature. Check the shared secret".into()
            }
            Self::Api { code: 9, .. } => {
                "Your Last.fm session expired. Connect again in Settings".into()
            }
            Self::Api { message, .. } => format!("Last.fm: {message}"),
        }
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("MusiKPL/", env!("CARGO_PKG_VERSION")))
        .build()
}

fn read(result: Result<ureq::Response, ureq::Error>) -> Result<Value, ApiError> {
    let response = match result {
        Ok(response) => response,
        // Last.fm reports API errors as JSON on non-2xx statuses.
        Err(ureq::Error::Status(_, response)) => response,
        Err(error) => return Err(ApiError::Network(error.to_string())),
    };
    let body = response
        .into_string()
        .map_err(|e| ApiError::Network(e.to_string()))?;
    parse_reply(&body)
}

fn parse_reply(body: &str) -> Result<Value, ApiError> {
    let value: Value = serde_json::from_str(body)
        .map_err(|_| ApiError::Network("Last.fm sent an unreadable reply".into()))?;
    if let Some(code) = value.get("error").and_then(Value::as_i64) {
        let message = value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error")
            .to_owned();
        return Err(ApiError::Api { code, message });
    }
    Ok(value)
}

fn get(params: Params) -> Result<Value, ApiError> {
    let mut request = agent().get(API_ROOT);
    for (name, value) in &params {
        request = request.query(name, value);
    }
    read(request.query("format", "json").call())
}

fn post(params: Params) -> Result<Value, ApiError> {
    let mut form: Vec<(&str, &str)> = params
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    form.push(("format", "json"));
    read(agent().post(API_ROOT).send_form(&form))
}

/// Step 1 of desktop authentication: an unapproved request token.
pub(crate) fn get_token(api_key: &str, api_secret: &str) -> Result<String, String> {
    let params = signed(
        vec![
            ("method", "auth.getToken".into()),
            ("api_key", api_key.into()),
        ],
        api_secret,
    );
    let value = get(params).map_err(ApiError::describe)?;
    value
        .get("token")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "Last.fm did not return a login token".into())
}

/// Step 2: the page where the user approves the token.
pub(crate) fn auth_url(api_key: &str, token: &str) -> String {
    format!("{AUTH_PAGE}?api_key={api_key}&token={token}")
}

/// Step 3: exchange the approved token for a session. `Ok(None)` means the
/// user has not approved it yet.
pub(crate) fn get_session(
    api_key: &str,
    api_secret: &str,
    token: &str,
) -> Result<Option<(String, String)>, String> {
    let params = signed(
        vec![
            ("method", "auth.getSession".into()),
            ("api_key", api_key.into()),
            ("token", token.into()),
        ],
        api_secret,
    );
    match get(params) {
        Ok(value) => parse_session(&value).map(Some),
        Err(ApiError::Api {
            code: TOKEN_NOT_AUTHORIZED,
            ..
        }) => Ok(None),
        Err(error) => Err(error.describe()),
    }
}

fn parse_session(value: &Value) -> Result<(String, String), String> {
    let session = value.get("session");
    let field = |name| {
        session
            .and_then(|session| session.get(name))
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    match (field("name"), field("key")) {
        (Some(name), Some(key)) => Ok((name, key)),
        _ => Err("Last.fm did not return a session".into()),
    }
}

fn song_params(method: &'static str, credentials: &Credentials, song: &SongInfo) -> Params {
    vec![
        ("method", method.into()),
        ("api_key", credentials.api_key.clone()),
        ("sk", credentials.session_key.clone()),
        ("artist", song.artist.clone()),
        ("track", song.title.clone()),
    ]
}

pub(crate) fn update_now_playing(
    credentials: &Credentials,
    song: &SongInfo,
    duration: Option<f64>,
) -> Result<(), String> {
    let mut params = song_params("track.updateNowPlaying", credentials, song);
    if let Some(duration) = duration {
        params.push(("duration", (duration.round() as u64).to_string()));
    }
    post(signed(params, &credentials.api_secret))
        .map(|_| ())
        .map_err(ApiError::describe)
}

pub(crate) fn scrobble(
    credentials: &Credentials,
    song: &SongInfo,
    started_at: u64,
    duration: Option<f64>,
) -> Result<(), String> {
    let mut params = song_params("track.scrobble", credentials, song);
    params.push(("timestamp", started_at.to_string()));
    if let Some(duration) = duration {
        params.push(("duration", (duration.round() as u64).to_string()));
    }
    let value = post(signed(params, &credentials.api_secret)).map_err(ApiError::describe)?;
    let accepted = value
        .pointer("/scrobbles/@attr/accepted")
        .and_then(|accepted| {
            accepted
                .as_u64()
                .or_else(|| accepted.as_str().and_then(|text| text.parse().ok()))
        })
        .unwrap_or(1);
    if accepted == 0 {
        return Err(format!(
            "Last.fm ignored the scrobble for \"{}\"",
            song.title
        ));
    }
    Ok(())
}

/// The song's album art, when Last.fm knows the track and has an image.
pub(crate) fn album_art(api_key: &str, song: &SongInfo) -> Result<Option<String>, String> {
    let params = vec![
        ("method", "track.getInfo".into()),
        ("api_key", api_key.into()),
        ("artist", song.artist.clone()),
        ("track", song.title.clone()),
        ("autocorrect", "1".into()),
    ];
    match get(params) {
        Ok(value) => Ok(pick_album_art(&value)),
        Err(ApiError::Api {
            code: TRACK_NOT_FOUND,
            ..
        }) => Ok(None),
        Err(error) => Err(error.describe()),
    }
}

/// The largest real album image in a `track.getInfo` reply.
fn pick_album_art(value: &Value) -> Option<String> {
    let images = value.pointer("/track/album/image")?.as_array()?;
    let url_of = |size: &str| {
        images
            .iter()
            .find(|image| image.get("size").and_then(Value::as_str) == Some(size))
            .and_then(|image| image.get("#text"))
            .and_then(Value::as_str)
            .filter(|url| url.starts_with("https://") && !url.contains(PLACEHOLDER_IMAGE))
            .map(str::to_owned)
    };
    ["extralarge", "large", "medium"]
        .into_iter()
        .find_map(url_of)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_sort_parameters_and_skip_format() {
        let params = vec![
            ("method", "auth.getToken".to_string()),
            ("format", "json".to_string()),
            ("api_key", "KEY".to_string()),
        ];
        let expected = format!(
            "{:x}",
            md5::compute("api_keyKEYmethodauth.getTokenSECRET".as_bytes())
        );
        assert_eq!(sign(&params, "SECRET"), expected);
    }

    #[test]
    fn unapproved_tokens_are_reported_as_api_errors() {
        let error = parse_reply(r#"{"error":14,"message":"Unauthorized Token"}"#)
            .expect_err("an error reply");
        assert!(matches!(
            error,
            ApiError::Api {
                code: TOKEN_NOT_AUTHORIZED,
                ..
            }
        ));
    }

    #[test]
    fn sessions_need_a_name_and_a_key() {
        let value: Value =
            serde_json::from_str(r#"{"session":{"name":"anna","key":"abc","subscriber":0}}"#)
                .unwrap();
        assert_eq!(
            parse_session(&value),
            Ok(("anna".to_string(), "abc".to_string()))
        );
        let value: Value = serde_json::from_str(r#"{"session":{"name":"anna"}}"#).unwrap();
        assert!(parse_session(&value).is_err());
    }

    #[test]
    fn album_art_prefers_the_largest_real_image() {
        let value: Value = serde_json::from_str(
            r##"{"track":{"album":{"image":[
                {"#text":"https://img/s.png","size":"small"},
                {"#text":"https://img/l.png","size":"large"},
                {"#text":"","size":"extralarge"}
            ]}}}"##,
        )
        .unwrap();
        assert_eq!(pick_album_art(&value), Some("https://img/l.png".into()));

        let placeholder: Value = serde_json::from_str(&format!(
            r##"{{"track":{{"album":{{"image":[{{"#text":"https://img/{PLACEHOLDER_IMAGE}.png","size":"extralarge"}}]}}}}}}"##
        ))
        .unwrap();
        assert_eq!(pick_album_art(&placeholder), None);
        assert_eq!(pick_album_art(&Value::Null), None);
    }
}
