//! Friends: MusiKPL users who added each other's friend codes can see when
//! the other has MusiKPL open and invite them to a Jam.
//!
//! It runs on iroh, like internet Jams: encrypted peer-to-peer connections
//! dialed by public key, with relay fallback and no MusiKPL server. Each
//! install has a permanent key (its friend code); the n0 directory lets a
//! friend find it from the key alone.
//!
//! Kept light on purpose: nothing runs until a friend with a code is added;
//! then one thread serves a single-threaded async runtime. Presence is a
//! tiny ping, made only when the app asks (menu open, or every few
//! minutes). Only keys on the friends list may connect.

use std::{
    collections::HashSet,
    sync::{
        mpsc::{self, Receiver, Sender},
        Arc, RwLock,
    },
    time::Duration,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use iroh::{
    endpoint::{presets, Connection, Incoming},
    Endpoint, EndpointId, SecretKey,
};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    time::timeout,
};

use crate::infrastructure::storage;

const CODE_PREFIX: &str = "musikpl-friend:";
const KEY_FILE: &str = "friend-key.txt";
/// The friends protocol on iroh; other protocols are refused.
const ALPN: &[u8] = b"musikpl/friends/1";
/// Reaching a friend, possibly through a relay first.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(12);
/// Waiting for an answer once connected.
const REPLY_TIMEOUT: Duration = Duration::from_secs(6);
/// Longest message line accepted.
const MAX_LINE: usize = 4 * 1024;
/// Longest name kept from a friend code or message.
const MAX_NAME: usize = 40;

/// What a friend code holds: the install's key and a name to show.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FriendCode {
    pub(crate) id: String,
    pub(crate) name: String,
}

impl FriendCode {
    pub(crate) fn encode(&self) -> String {
        let json = serde_json::to_vec(self).unwrap_or_default();
        format!("{CODE_PREFIX}{}", URL_SAFE_NO_PAD.encode(json))
    }

    /// Read a pasted friend code (untrusted input).
    pub(crate) fn decode(text: &str) -> Result<Self, String> {
        const INVALID: &str = "That isn't a MusiKPL friend code. Ask your friend to copy it again.";
        let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        if text.len() > 600 {
            return Err(INVALID.into());
        }
        let encoded = text.strip_prefix(CODE_PREFIX).unwrap_or(&text);
        let json = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| INVALID.to_string())?;
        let code: FriendCode = serde_json::from_slice(&json).map_err(|_| INVALID.to_string())?;
        if code.id.parse::<EndpointId>().is_err() {
            return Err(INVALID.into());
        }
        Ok(FriendCode {
            id: code.id,
            name: code.name.chars().take(MAX_NAME).collect(),
        })
    }
}

/// This install's permanent key, created on first use.
fn identity() -> SecretKey {
    if let Some(hex) = storage::read::<String>(KEY_FILE) {
        if let Some(bytes) = parse_hex(&hex) {
            return SecretKey::from_bytes(&bytes);
        }
    }
    let key = SecretKey::generate();
    let hex: String = key.to_bytes().iter().map(|b| format!("{b:02x}")).collect();
    let _ = storage::write(KEY_FILE, &hex);
    key
}

fn parse_hex(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.is_ascii() {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(bytes)
}

/// This install's friend code, with `name` as the name friends see.
pub(crate) fn my_code(name: &str) -> FriendCode {
    FriendCode {
        id: identity().public().to_string(),
        name: name.chars().take(MAX_NAME).collect(),
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
enum Message {
    Ping { name: String },
    Invite { name: String, invite: String },
    Ok { name: String },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FriendEvent {
    /// Whether a friend has MusiKPL open.
    Presence { id: String, online: bool },
    /// A friend invited us to their Jam.
    Invited {
        id: String,
        name: String,
        invite: String,
    },
    /// An invite we sent arrived, or couldn't be delivered.
    InviteSent { id: String, delivered: bool },
    /// The service couldn't start.
    Failed(String),
}

enum Command {
    Check(Vec<String>),
    Invite { id: String, invite: String },
}

/// The running friends service.
pub(crate) struct Friends {
    commands: tokio::sync::mpsc::UnboundedSender<Command>,
    events: Receiver<FriendEvent>,
    allowed: Arc<RwLock<HashSet<EndpointId>>>,
    name: Arc<RwLock<String>>,
}

impl Friends {
    /// Start serving, allowing `friends` (keys) to connect.
    pub(crate) fn start(friends: &[String], name: &str, ctx: eframe::egui::Context) -> Self {
        Self::start_as(identity(), friends, name, ctx)
    }

    fn start_as(
        key: SecretKey,
        friends: &[String],
        name: &str,
        ctx: eframe::egui::Context,
    ) -> Self {
        let (commands, inbox) = tokio::sync::mpsc::unbounded_channel();
        let (sender, events) = mpsc::channel();
        let allowed = Arc::new(RwLock::new(parse_ids(friends)));
        let name = Arc::new(RwLock::new(name.to_owned()));
        let (task_allowed, task_name) = (allowed.clone(), name.clone());
        std::thread::Builder::new()
            .name("musikpl-friends".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = sender.send(FriendEvent::Failed(error.to_string()));
                        return;
                    }
                };
                runtime.block_on(run(key, inbox, sender, task_allowed, task_name, ctx));
            })
            .ok();
        Self {
            commands,
            events,
            allowed,
            name,
        }
    }

    /// Who may connect: the friends' keys.
    pub(crate) fn allow(&self, friends: &[String]) {
        if let Ok(mut allowed) = self.allowed.write() {
            *allowed = parse_ids(friends);
        }
    }

    pub(crate) fn set_name(&self, name: &str) {
        if let Ok(mut current) = self.name.write() {
            name.clone_into(&mut current);
        }
    }

    /// Ask which friends are online; answers arrive as `Presence` events.
    pub(crate) fn check(&self, friends: Vec<String>) {
        let _ = self.commands.send(Command::Check(friends));
    }

    pub(crate) fn invite(&self, id: String, invite: String) {
        let _ = self.commands.send(Command::Invite { id, invite });
    }

    pub(crate) fn poll(&self) -> Vec<FriendEvent> {
        self.events.try_iter().collect()
    }
}

fn parse_ids(friends: &[String]) -> HashSet<EndpointId> {
    friends.iter().filter_map(|id| id.parse().ok()).collect()
}

async fn run(
    key: SecretKey,
    mut inbox: tokio::sync::mpsc::UnboundedReceiver<Command>,
    events: Sender<FriendEvent>,
    allowed: Arc<RwLock<HashSet<EndpointId>>>,
    name: Arc<RwLock<String>>,
    ctx: eframe::egui::Context,
) {
    let endpoint = match Endpoint::builder(presets::N0)
        .secret_key(key)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
    {
        Ok(endpoint) => endpoint,
        Err(error) => {
            let _ = events.send(FriendEvent::Failed(format!(
                "Couldn't start Friends: {error}"
            )));
            return;
        }
    };
    let report = {
        let events = events.clone();
        move |event: FriendEvent| {
            let _ = events.send(event);
            ctx.request_repaint();
        }
    };

    // Answer friends' pings and invites.
    {
        let (endpoint, allowed, name, report) = (
            endpoint.clone(),
            allowed.clone(),
            name.clone(),
            report.clone(),
        );
        tokio::spawn(async move {
            while let Some(incoming) = endpoint.accept().await {
                tokio::spawn(answer(
                    incoming,
                    allowed.clone(),
                    name.clone(),
                    report.clone(),
                ));
            }
        });
    }

    while let Some(command) = inbox.recv().await {
        let my_name = name.read().map(|name| name.clone()).unwrap_or_default();
        match command {
            Command::Check(friends) => {
                for id in friends {
                    let (endpoint, report, my_name) =
                        (endpoint.clone(), report.clone(), my_name.clone());
                    tokio::spawn(async move {
                        let online = send(&endpoint, &id, Message::Ping { name: my_name })
                            .await
                            .is_ok();
                        report(FriendEvent::Presence { id, online });
                    });
                }
            }
            Command::Invite { id, invite } => {
                let (endpoint, report) = (endpoint.clone(), report.clone());
                tokio::spawn(async move {
                    let message = Message::Invite {
                        name: my_name,
                        invite,
                    };
                    let delivered = send(&endpoint, &id, message).await.is_ok();
                    report(FriendEvent::InviteSent { id, delivered });
                });
            }
        }
    }
    endpoint.close().await;
}

/// Dial a friend, send one message, and wait for their OK.
async fn send(endpoint: &Endpoint, id: &str, message: Message) -> Result<(), ()> {
    let id: EndpointId = id.parse().map_err(|_| ())?;
    let connection = timeout(CONNECT_TIMEOUT, endpoint.connect(id, ALPN))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())?;
    let result = exchange(&connection, &message).await;
    connection.close(0u32.into(), b"done");
    result
}

async fn exchange(connection: &Connection, message: &Message) -> Result<(), ()> {
    let (mut send, recv) = connection.open_bi().await.map_err(|_| ())?;
    let mut line = serde_json::to_vec(message).map_err(|_| ())?;
    line.push(b'\n');
    send.write_all(&line).await.map_err(|_| ())?;
    let _ = send.finish();
    let reply = timeout(REPLY_TIMEOUT, read_line(recv))
        .await
        .map_err(|_| ())?
        .ok_or(())?;
    match serde_json::from_str::<Message>(&reply) {
        Ok(Message::Ok { .. }) => Ok(()),
        _ => Err(()),
    }
}

async fn read_line(recv: iroh::endpoint::RecvStream) -> Option<String> {
    let mut reader = BufReader::new(recv).take(MAX_LINE as u64);
    let mut line = String::new();
    reader.read_line(&mut line).await.ok()?;
    (!line.trim().is_empty()).then_some(line)
}

/// A friend connected: check they are on the list, then answer.
async fn answer(
    incoming: Incoming,
    allowed: Arc<RwLock<HashSet<EndpointId>>>,
    name: Arc<RwLock<String>>,
    report: impl Fn(FriendEvent),
) {
    let Ok(connection) = incoming.await else {
        return;
    };
    let from = connection.remote_id();
    let is_friend = allowed.read().is_ok_and(|allowed| allowed.contains(&from));
    if !is_friend {
        connection.close(1u32.into(), b"not a friend");
        return;
    }
    let Ok(Ok((mut send, recv))) = timeout(REPLY_TIMEOUT, connection.accept_bi()).await else {
        return;
    };
    let Ok(Some(line)) = timeout(REPLY_TIMEOUT, read_line(recv)).await else {
        return;
    };
    let my_name = name.read().map(|name| name.clone()).unwrap_or_default();
    match serde_json::from_str::<Message>(&line) {
        Ok(Message::Ping { .. }) => {}
        Ok(Message::Invite { name, invite }) => {
            report(FriendEvent::Invited {
                id: from.to_string(),
                name: name.chars().take(MAX_NAME).collect(),
                invite: invite.chars().take(2_000).collect(),
            });
        }
        _ => return,
    }
    let mut reply = serde_json::to_vec(&Message::Ok { name: my_name }).unwrap_or_default();
    reply.push(b'\n');
    let _ = send.write_all(&reply).await;
    let _ = send.finish();
    // Let the reply reach them before the connection drops.
    let _ = timeout(REPLY_TIMEOUT, connection.closed()).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friend_codes_survive_copy_and_paste() {
        let code = FriendCode {
            id: SecretKey::generate().public().to_string(),
            name: "liathy".into(),
        };
        let text = code.encode();
        assert!(text.starts_with(CODE_PREFIX));
        assert_eq!(FriendCode::decode(&format!("  {text}\n")).unwrap(), code);
        assert!(FriendCode::decode("musikpl-friend:nonsense").is_err());
        assert!(FriendCode::decode("musikpl:invite").is_err());
    }

    /// Two friends on this PC, over the real internet (iroh relays and
    /// directory): each sees the other online, an invite arrives, and a
    /// stranger is refused. Run with cargo test -- --ignored.
    #[test]
    #[ignore = "uses the internet"]
    fn friends_see_each_other_and_invite_over_the_internet() {
        use std::time::Instant;
        let (ana_key, bia_key, eve_key) = (
            SecretKey::generate(),
            SecretKey::generate(),
            SecretKey::generate(),
        );
        let (ana_id, bia_id) = (ana_key.public().to_string(), bia_key.public().to_string());
        let ctx = eframe::egui::Context::default();
        let ana = Friends::start_as(ana_key, std::slice::from_ref(&bia_id), "Ana", ctx.clone());
        let bia = Friends::start_as(bia_key, std::slice::from_ref(&ana_id), "Bia", ctx.clone());
        let eve = Friends::start_as(eve_key, std::slice::from_ref(&ana_id), "Eve", ctx);

        let wait = |friends: &Friends, matches: &dyn Fn(&FriendEvent) -> bool| {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(90) {
                if friends.poll().iter().any(matches) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            false
        };
        // Give both a moment to publish themselves in the directory.
        std::thread::sleep(Duration::from_secs(5));
        let mut online = false;
        for _ in 0..4 {
            ana.check(vec![bia_id.clone()]);
            if wait(&ana, &|event| {
                matches!(event, FriendEvent::Presence { online: true, .. })
            }) {
                online = true;
                break;
            }
        }
        assert!(online, "Ana sees Bia online");

        ana.invite(bia_id, "musikpl:test-invite".into());
        assert!(
            wait(&bia, &|event| matches!(
                event,
                FriendEvent::Invited { name, invite, .. } if name == "Ana" && invite == "musikpl:test-invite"
            )),
            "Bia gets Ana's invite"
        );

        // Eve isn't on Ana's list: Ana refuses her.
        eve.check(vec![ana_id]);
        assert!(wait(&eve, &|event| matches!(
            event,
            FriendEvent::Presence { online: false, .. }
        )));
    }

    #[test]
    fn messages_are_tagged_lines() {
        let line = serde_json::to_string(&Message::Ping { name: "a".into() }).unwrap();
        assert_eq!(line, r#"{"t":"ping","name":"a"}"#);
        assert!(matches!(
            serde_json::from_str::<Message>(r#"{"t":"ok","name":"b"}"#),
            Ok(Message::Ok { .. })
        ));
    }
}
