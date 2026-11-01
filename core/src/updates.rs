//! New versions, from MusiKPL's GitHub releases.
//!
//! GitHub's addresses are looked up through Cloudflare's 1.1.1.1 (over
//! HTTPS), so an internet provider blocking GitHub by DNS doesn't stop
//! updates. Each app installs what it finds its own way (the Windows setup,
//! swapping the portable file, Android's package installer).

use std::{
    io::{Read, Write},
    path::Path,
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

use serde_json::Value;

const LATEST_RELEASE: &str = "https://api.github.com/repos/liathy/MusiKPL/releases/latest";

/// A release newer than this build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    /// (file name, https download link) of each file in the release.
    pub assets: Vec<(String, String)>,
}

impl Release {
    /// The download link of the release file called `name`.
    pub fn asset(&self, name: &str) -> Option<String> {
        self.assets
            .iter()
            .find(|(asset, _)| asset == name)
            .map(|(_, url)| url.clone())
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .resolver(crate::cloudflare_dns::CloudflareDns)
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .user_agent(concat!("MusiKPL/", env!("CARGO_PKG_VERSION")))
        .build()
}

/// Ask GitHub for the latest release. `Ok(None)` means up to date.
pub fn check() -> Result<Option<Release>, String> {
    let response = agent()
        .get(LATEST_RELEASE)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("Couldn't reach GitHub to check for updates ({e})"))?;
    let body = response
        .into_string()
        .map_err(|e| format!("Couldn't read the update information ({e})"))?;
    let value: Value =
        serde_json::from_str(&body).map_err(|_| "GitHub sent unreadable update information")?;
    Ok(parse_release(&value)
        .filter(|release| is_newer(&release.version, env!("CARGO_PKG_VERSION"))))
}

pub fn parse_release(value: &Value) -> Option<Release> {
    if value.get("draft").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let version = value
        .get("tag_name")
        .and_then(Value::as_str)?
        .trim_start_matches('v')
        .to_owned();
    let assets = value
        .get("assets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|asset| {
            let name = asset.get("name")?.as_str()?;
            let url = asset
                .get("browser_download_url")?
                .as_str()
                .filter(|url| url.starts_with("https://"))?;
            Some((name.to_owned(), url.to_owned()))
        })
        .collect();
    Some(Release { version, assets })
}

/// Whether version `candidate` ("2.3.0") is newer than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let parts = |version: &str| -> Vec<u64> {
        version
            .trim_start_matches('v')
            .split(['.', '-', '+'])
            .take(3)
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    };
    parts(candidate) > parts(current)
}

/// Download `url` to `target`, reporting progress in per-mille. The file
/// only appears at `target` once it's complete.
pub fn download(url: &str, target: &Path, progress: &AtomicU32) -> Result<(), String> {
    let failed = |e: &dyn std::fmt::Display| format!("The update download failed ({e})");
    let response = agent().get(url).call().map_err(|e| failed(&e))?;
    let total: u64 = response
        .header("Content-Length")
        .and_then(|length| length.parse().ok())
        .unwrap_or(0);
    let temporary = target.with_extension("part");
    let mut file = std::fs::File::create(&temporary).map_err(|e| failed(&e))?;
    let mut reader = response.into_reader();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut done: u64 = 0;
    loop {
        let read = reader.read(&mut buffer).map_err(|e| failed(&e))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read]).map_err(|e| failed(&e))?;
        done += read as u64;
        if let Some(per_mille) = (done * 1000).checked_div(total) {
            progress.store(per_mille.min(1000) as u32, Ordering::Relaxed);
        }
    }
    file.flush().map_err(|e| failed(&e))?;
    drop(file);
    if total > 0 && done != total {
        let _ = std::fs::remove_file(&temporary);
        return Err("The update download was cut short. Try again.".into());
    }
    std::fs::rename(&temporary, target).map_err(|e| failed(&e))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs the internet: the real update check, with GitHub found through
    /// Cloudflare.
    #[test]
    #[ignore]
    fn the_update_check_reaches_github_through_cloudflare() {
        check().unwrap();
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("2.3.0", "2.2.0"));
        assert!(is_newer("v2.10.0", "2.9.9"));
        assert!(is_newer("3.0.0", "2.99.99"));
        assert!(!is_newer("2.2.0", "2.2.0"));
        assert!(!is_newer("2.1.9", "2.2.0"));
        assert!(!is_newer("garbage", "2.2.0"));
    }

    #[test]
    fn releases_are_read_from_github_answers() {
        let value: Value = serde_json::from_str(
            r#"{"tag_name":"v2.3.0","draft":false,"assets":[
                {"name":"MusiKPL.exe","browser_download_url":"https://github.com/x/MusiKPL.exe"},
                {"name":"MusiKPL-android-arm64.apk","browser_download_url":"https://github.com/x/app.apk"},
                {"name":"MusiKPL-Setup.exe","browser_download_url":"http://insecure"}
            ]}"#,
        )
        .unwrap();
        let release = parse_release(&value).unwrap();
        assert_eq!(release.version, "2.3.0");
        assert_eq!(
            release.asset("MusiKPL-android-arm64.apk").as_deref(),
            Some("https://github.com/x/app.apk")
        );
        assert!(release.asset("MusiKPL-Setup.exe").is_none(), "only https");
        let draft: Value = serde_json::from_str(r#"{"tag_name":"v9.0.0","draft":true}"#).unwrap();
        assert!(parse_release(&draft).is_none());
    }
}
