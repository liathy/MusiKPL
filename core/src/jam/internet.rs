//! Internet Jams: an encrypted iroh tunnel that carries the LAN Jam
//! protocol, unchanged, between a host and guests outside its network.
//!
//! iroh dials devices by public key: it connects directly when the routers
//! allow it and falls back to relay servers otherwise, and every connection
//! is end-to-end encrypted and authenticated by the devices' keys.
//!
//! Access control:
//! - The host only accepts connections for MusiKPL's Jam protocol (its ALPN).
//! - The invite holds the host's key, so a guest reaches the real host.
//! - Every stream must first present the invite's 128-bit secret, created
//!   fresh for each Jam; anything else is dropped. Ending the Jam ends the
//!   invite.
//! - Through the tunnel, guests speak the normal Jam protocol, where the
//!   host already decides what a guest may do.
//!
//! The host forwards each accepted stream to its own Jam listener on
//! 127.0.0.1; the guest opens a local port that its normal Jam connection
//! uses. The async runtime exists only while an internet Jam is active.

use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use iroh::{
    endpoint::{presets, Connection, Incoming, RecvStream, SendStream},
    Endpoint, EndpointAddr, SecretKey,
};
use serde::{Deserialize, Serialize};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    time::timeout,
};

use super::protocol::{valid_token, MAX_NAME_CHARS, PROTOCOL_VERSION};

/// MusiKPL's Jam protocol on iroh. The version keeps incompatible builds
/// from connecting at all.
fn alpn() -> Vec<u8> {
    format!("musikpl/jam/{PROTOCOL_VERSION}").into_bytes()
}

const SECRET_LEN: usize = 16;
const ACCEPTED: u8 = 1;
const INVITE_PREFIX: &str = "musikpl:";
/// Longest an invite may be; a pasted invite is untrusted input.
const MAX_INVITE_CHARS: usize = 2_000;
/// Reaching the relay servers when sharing.
const ONLINE_TIMEOUT: Duration = Duration::from_secs(20);
/// Reaching the host, possibly through a relay first.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// A connection must present the secret this quickly.
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);
/// Time given to close connections gracefully when a Jam ends.
const CLOSE_GRACE: Duration = Duration::from_secs(2);

/// Progress of starting an internet share or join.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Progress<T> {
    Preparing,
    Ready(T),
    Failed(String),
}

/// Everything a guest needs to join: where the host is, the secret that
/// proves the invite, and the Jam's own code.
#[derive(Clone, Debug, PartialEq)]
pub struct Invite {
    host: EndpointAddr,
    secret: [u8; SECRET_LEN],
    pub token: String,
    pub host_name: String,
}

#[derive(Serialize, Deserialize)]
struct InviteData {
    /// Jam protocol version.
    v: u16,
    host: EndpointAddr,
    secret: String,
    code: String,
    name: String,
}

impl Invite {
    pub fn encode(&self) -> String {
        let data = InviteData {
            v: PROTOCOL_VERSION,
            host: self.host.clone(),
            secret: self
                .secret
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            code: self.token.clone(),
            name: self.host_name.clone(),
        };
        let json = serde_json::to_vec(&data).unwrap_or_default();
        format!("{INVITE_PREFIX}{}", URL_SAFE_NO_PAD.encode(json))
    }

    pub fn decode(text: &str) -> Result<Self, String> {
        const INVALID: &str = "That isn't a MusiKPL invite. Ask the host to copy it again.";
        let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        if text.len() > MAX_INVITE_CHARS {
            return Err(INVALID.into());
        }
        let encoded = text.strip_prefix(INVITE_PREFIX).unwrap_or(&text);
        let json = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| INVALID.to_string())?;
        let data: InviteData = serde_json::from_slice(&json).map_err(|_| INVALID.to_string())?;
        if data.v != PROTOCOL_VERSION {
            return Err(
                "This invite is from a different MusiKPL version. Both of you need the same version."
                    .into(),
            );
        }
        let secret = parse_secret(&data.secret).ok_or(INVALID)?;
        if !valid_token(&data.code) {
            return Err(INVALID.into());
        }
        Ok(Self {
            host: data.host,
            secret,
            token: data.code,
            host_name: data.name.chars().take(MAX_NAME_CHARS).collect(),
        })
    }
}

fn parse_secret(hex: &str) -> Option<[u8; SECRET_LEN]> {
    if hex.len() != SECRET_LEN * 2 || !hex.is_ascii() {
        return None;
    }
    let mut secret = [0u8; SECRET_LEN];
    for (index, byte) in secret.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(secret)
}

/// A new random session secret, from the system's secure random source.
fn new_secret() -> [u8; SECRET_LEN] {
    let key = SecretKey::generate();
    let mut secret = [0u8; SECRET_LEN];
    secret.copy_from_slice(&key.to_bytes()[..SECRET_LEN]);
    secret
}

/// Compare secrets without revealing, through timing, how much matched.
fn same_secret(a: &[u8; SECRET_LEN], b: &[u8; SECRET_LEN]) -> bool {
    a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}

/// The async runtime and endpoint behind one internet share or join. It
/// exists only while the Jam does; dropping it closes connections in the
/// background without blocking the interface.
struct Background {
    runtime: Option<tokio::runtime::Runtime>,
    endpoint: Arc<Mutex<Option<Endpoint>>>,
}

impl Background {
    fn start() -> Result<Self, String> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("musikpl-internet")
            .enable_all()
            .build()
            .map_err(|e| format!("Could not start internet Jams: {e}"))?;
        Ok(Self {
            runtime: Some(runtime),
            endpoint: Arc::default(),
        })
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        let Some(runtime) = self.runtime.take() else {
            return;
        };
        let endpoint = self.endpoint.lock().ok().and_then(|mut slot| slot.take());
        std::thread::spawn(move || {
            if let Some(endpoint) = endpoint {
                runtime.block_on(async {
                    let _ = timeout(CLOSE_GRACE, endpoint.close()).await;
                });
            }
            runtime.shutdown_background();
        });
    }
}

fn set<T>(status: &Mutex<Progress<T>>, progress: Progress<T>) {
    if let Ok(mut status) = status.lock() {
        *status = progress;
    }
}

fn get<T: Clone>(status: &Mutex<Progress<T>>) -> Progress<T> {
    status
        .lock()
        .map(|status| status.clone())
        .unwrap_or(Progress::Failed("Internet Jam stopped unexpectedly".into()))
}

/// Host: share a running Jam over the internet.
pub struct Share {
    status: Arc<Mutex<Progress<String>>>,
    _background: Option<Background>,
}

impl Share {
    /// Start sharing the Jam listening on `local_jam` (this device).
    pub fn start(local_jam: SocketAddr, token: String, host_name: String) -> Self {
        let status = Arc::new(Mutex::new(Progress::Preparing));
        let background = match Background::start() {
            Ok(background) => background,
            Err(error) => {
                set(&status, Progress::Failed(error));
                return Self {
                    status,
                    _background: None,
                };
            }
        };
        let (task_status, slot) = (status.clone(), background.endpoint.clone());
        if let Some(runtime) = &background.runtime {
            runtime.spawn(async move {
                let endpoint = match Endpoint::builder(presets::N0)
                    .alpns(vec![alpn()])
                    .bind()
                    .await
                {
                    Ok(endpoint) => endpoint,
                    Err(error) => {
                        set(
                            &task_status,
                            Progress::Failed(format!("Could not start internet sharing: {error}")),
                        );
                        return;
                    }
                };
                if let Ok(mut slot) = slot.lock() {
                    *slot = Some(endpoint.clone());
                }
                if timeout(ONLINE_TIMEOUT, endpoint.online()).await.is_err() {
                    set(
                        &task_status,
                        Progress::Failed(
                            "Couldn't reach the internet relay servers. Check your connection and try again."
                                .into(),
                        ),
                    );
                    return;
                }
                // Share only the key and relay: the relay helps both sides
                // find their best direct route, without listing local IPs.
                let full = endpoint.addr();
                let host = EndpointAddr::from_parts(
                    full.id,
                    full.addrs.into_iter().filter(|addr| addr.is_relay()),
                );
                serve(endpoint, host, local_jam, token, host_name, &task_status).await;
            });
        }
        Self {
            status,
            _background: Some(background),
        }
    }

    /// `Ready` holds the invite to send to friends.
    pub fn status(&self) -> Progress<String> {
        get(&self.status)
    }
}

/// Publish the invite, then admit guests that present its secret.
async fn serve(
    endpoint: Endpoint,
    host: EndpointAddr,
    local_jam: SocketAddr,
    token: String,
    host_name: String,
    status: &Mutex<Progress<String>>,
) {
    let secret = new_secret();
    let invite = Invite {
        host,
        secret,
        token,
        host_name,
    };
    set(status, Progress::Ready(invite.encode()));
    while let Some(incoming) = endpoint.accept().await {
        tokio::spawn(admit(incoming, local_jam, secret));
    }
}

async fn admit(incoming: Incoming, local_jam: SocketAddr, secret: [u8; SECRET_LEN]) {
    let Ok(connection) = incoming.await else {
        return;
    };
    // A guest may reconnect its Jam session over the same connection, so
    // every stream is checked on its own.
    while let Ok((send, recv)) = connection.accept_bi().await {
        tokio::spawn(forward(send, recv, local_jam, secret, connection.clone()));
    }
}

async fn forward(
    mut send: SendStream,
    mut recv: RecvStream,
    local_jam: SocketAddr,
    secret: [u8; SECRET_LEN],
    connection: Connection,
) {
    let mut presented = [0u8; SECRET_LEN];
    let proved = timeout(AUTH_TIMEOUT, recv.read_exact(&mut presented))
        .await
        .is_ok_and(|read| read.is_ok());
    if !proved || !same_secret(&presented, &secret) {
        connection.close(1u32.into(), b"invalid invite");
        return;
    }
    let Ok(jam) = TcpStream::connect(local_jam).await else {
        return;
    };
    if send.write_all(&[ACCEPTED]).await.is_err() {
        return;
    }
    pipe(jam, send, recv).await;
}

/// Copy bytes both ways until either side ends.
async fn pipe(tcp: TcpStream, mut send: SendStream, mut recv: RecvStream) {
    let _ = tcp.set_nodelay(true);
    let (mut tcp_read, mut tcp_write) = tcp.into_split();
    let inbound = async {
        let _ = tokio::io::copy(&mut recv, &mut tcp_write).await;
        let _ = tcp_write.shutdown().await;
    };
    let outbound = async {
        let _ = tokio::io::copy(&mut tcp_read, &mut send).await;
        let _ = send.finish();
    };
    tokio::join!(inbound, outbound);
}

/// Guest: connect to a host's internet Jam.
pub struct Tunnel {
    status: Arc<Mutex<Progress<SocketAddr>>>,
    pub token: String,
    pub host_name: String,
    _background: Option<Background>,
}

impl Tunnel {
    pub fn start(invite: Invite) -> Self {
        let status = Arc::new(Mutex::new(Progress::Preparing));
        let (token, host_name) = (invite.token.clone(), invite.host_name.clone());
        let background = match Background::start() {
            Ok(background) => background,
            Err(error) => {
                set(&status, Progress::Failed(error));
                return Self {
                    status,
                    token,
                    host_name,
                    _background: None,
                };
            }
        };
        let (task_status, slot) = (status.clone(), background.endpoint.clone());
        if let Some(runtime) = &background.runtime {
            runtime.spawn(async move {
                let endpoint = match Endpoint::bind(presets::N0).await {
                    Ok(endpoint) => endpoint,
                    Err(error) => {
                        set(
                            &task_status,
                            Progress::Failed(format!("Could not start internet Jams: {error}")),
                        );
                        return;
                    }
                };
                if let Ok(mut slot) = slot.lock() {
                    *slot = Some(endpoint.clone());
                }
                if let Err(error) = dial(&endpoint, &invite, &task_status).await {
                    set(&task_status, Progress::Failed(error));
                }
            });
        }
        Self {
            status,
            token,
            host_name,
            _background: Some(background),
        }
    }

    /// `Ready` holds the local address the Jam connection should use.
    pub fn status(&self) -> Progress<SocketAddr> {
        get(&self.status)
    }
}

/// Connect to the host, prove the invite, and serve the local port.
async fn dial(
    endpoint: &Endpoint,
    invite: &Invite,
    status: &Mutex<Progress<SocketAddr>>,
) -> Result<(), String> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|e| format!("Could not open a local port for the Jam: {e}"))?;
    let local = listener
        .local_addr()
        .map_err(|e| format!("Could not open a local port for the Jam: {e}"))?;
    let unreachable =
        "Couldn't reach the host. The Jam may have ended, or the invite is out of date.";
    let connection = timeout(
        CONNECT_TIMEOUT,
        endpoint.connect(invite.host.clone(), &alpn()),
    )
    .await
    .map_err(|_| unreachable.to_string())?
    .map_err(|_| unreachable.to_string())?;
    // Prove the invite once before reporting success, so a stale invite
    // fails with a clear message instead of a stalled Jam.
    let mut first = Some(open_proved(&connection, &invite.secret).await?);
    set(status, Progress::Ready(local));
    loop {
        let Ok((tcp, peer)) = listener.accept().await else {
            return Ok(());
        };
        if !peer.ip().is_loopback() {
            continue;
        }
        let streams = match first.take() {
            Some(streams) => streams,
            None => match open_proved(&connection, &invite.secret).await {
                Ok(streams) => streams,
                Err(_) => return Ok(()), // The Jam connection reports the loss.
            },
        };
        tokio::spawn(pipe(tcp, streams.0, streams.1));
    }
}

async fn open_proved(
    connection: &Connection,
    secret: &[u8; SECRET_LEN],
) -> Result<(SendStream, RecvStream), String> {
    let refused = "The host didn't accept this invite. Ask them for a new one.";
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|_| refused.to_string())?;
    send.write_all(secret)
        .await
        .map_err(|_| refused.to_string())?;
    let mut answer = [0u8; 1];
    let accepted = timeout(AUTH_TIMEOUT, recv.read_exact(&mut answer))
        .await
        .is_ok_and(|read| read.is_ok())
        && answer[0] == ACCEPTED;
    if accepted {
        Ok((send, recv))
    } else {
        Err(refused.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};

    fn invite() -> Invite {
        Invite {
            host: EndpointAddr::new(SecretKey::generate().public()),
            secret: new_secret(),
            token: "483921".into(),
            host_name: "Living room".into(),
        }
    }

    #[test]
    fn invites_survive_copy_and_paste() {
        let original = invite();
        let text = original.encode();
        assert!(text.starts_with(INVITE_PREFIX));
        assert_eq!(Invite::decode(&text), Ok(original.clone()));
        // Chat apps wrap long text and add spaces; the prefix is optional.
        let wrapped = format!("  {}\n{} ", &text[..40], &text[40..]);
        assert_eq!(Invite::decode(&wrapped), Ok(original.clone()));
        assert_eq!(
            Invite::decode(text.strip_prefix(INVITE_PREFIX).unwrap()),
            Ok(original)
        );
    }

    #[test]
    fn broken_or_foreign_invites_are_refused() {
        assert!(Invite::decode("").is_err());
        assert!(Invite::decode("musikpl:not-base64!").is_err());
        assert!(Invite::decode(&format!("{INVITE_PREFIX}{}", "A".repeat(3_000))).is_err());
        let other_version = InviteData {
            v: PROTOCOL_VERSION + 1,
            host: invite().host,
            secret: "00".repeat(SECRET_LEN),
            code: "123456".into(),
            name: "x".into(),
        };
        let text = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&other_version).unwrap());
        assert!(Invite::decode(&text)
            .unwrap_err()
            .contains("different MusiKPL version"));
    }

    #[test]
    fn secrets_are_random_and_compared_exactly() {
        let (a, b) = (new_secret(), new_secret());
        assert_ne!(a, b);
        assert!(same_secret(&a, &a));
        let mut almost = a;
        almost[SECRET_LEN - 1] ^= 1;
        assert!(!same_secret(&a, &almost));
        assert_eq!(
            parse_secret(&"ab".repeat(SECRET_LEN)),
            Some([0xab; SECRET_LEN])
        );
        assert_eq!(parse_secret("abc"), None);
        assert_eq!(parse_secret(&"zz".repeat(SECRET_LEN)), None);
    }

    /// The real thing, through n0's public relay servers: share, decode the
    /// invite, join, and exchange a line with the host's Jam listener.
    #[test]
    #[ignore = "needs internet access to n0's relay servers; run with --ignored"]
    fn an_invite_works_over_the_internet() {
        let jam = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let jam_addr = jam.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in jam.incoming().flatten() {
                let mut writer = stream.try_clone().unwrap();
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    let _ = writeln!(writer, "host got {line}");
                }
            }
        });
        let share = Share::start(jam_addr, "483921".into(), "Desk".into());
        let started = std::time::Instant::now();
        let invite = loop {
            match share.status() {
                Progress::Ready(invite) => break invite,
                Progress::Failed(error) => panic!("share failed: {error}"),
                Progress::Preparing => {
                    assert!(
                        started.elapsed() < Duration::from_secs(40),
                        "share timed out"
                    );
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        };
        println!(
            "invite ({} chars) ready after {:?}",
            invite.len(),
            started.elapsed()
        );
        let tunnel = Tunnel::start(Invite::decode(&invite).unwrap());
        let local = loop {
            match tunnel.status() {
                Progress::Ready(local) => break local,
                Progress::Failed(error) => panic!("join failed: {error}"),
                Progress::Preparing => {
                    assert!(
                        started.elapsed() < Duration::from_secs(80),
                        "join timed out"
                    );
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        };
        println!("joined after {:?}", started.elapsed());
        let mut stream = std::net::TcpStream::connect(local).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(15)))
            .unwrap();
        writeln!(stream, "hello").unwrap();
        let mut reply = String::new();
        BufReader::new(stream).read_line(&mut reply).unwrap();
        assert_eq!(reply.trim(), "host got hello");
    }

    /// Two endpoints on this machine, no internet or relays: a guest with
    /// the right secret reaches the host's Jam listener through the tunnel;
    /// one with the wrong secret is refused.
    #[test]
    fn the_tunnel_carries_the_jam_and_checks_the_secret() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        // Stand-in for the host's Jam listener: answers each line it gets.
        let jam = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let jam_addr = jam.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in jam.incoming().flatten() {
                std::thread::spawn(move || {
                    let mut writer = stream.try_clone().unwrap();
                    for line in BufReader::new(stream).lines().map_while(Result::ok) {
                        let _ = writeln!(writer, "host got {line}");
                    }
                });
            }
        });

        let bind = |alpns: Vec<Vec<u8>>| {
            Endpoint::builder(presets::Minimal)
                .relay_mode(iroh::RelayMode::Disabled)
                .alpns(alpns)
                .bind()
        };
        let host = runtime.block_on(bind(vec![alpn()])).unwrap();
        let port = host
            .bound_sockets()
            .into_iter()
            .find(SocketAddr::is_ipv4)
            .unwrap()
            .port();
        let host_addr = EndpointAddr::new(host.id())
            .with_ip_addr(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
        let share_status = Arc::new(Mutex::new(Progress::Preparing));
        let serve_status = share_status.clone();
        let serve_addr = host_addr.clone();
        runtime.spawn(async move {
            serve(
                host,
                serve_addr,
                jam_addr,
                "483921".into(),
                "Desk".into(),
                &serve_status,
            )
            .await;
        });
        let invite_text = loop {
            if let Progress::Ready(text) = get(&share_status) {
                break text;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let invite = Invite::decode(&invite_text).unwrap();
        assert_eq!(invite.token, "483921");

        // Right secret: the line comes back from the host's Jam listener.
        let guest = runtime.block_on(bind(vec![])).unwrap();
        let tunnel_status = Arc::new(Mutex::new(Progress::Preparing));
        let dial_status = tunnel_status.clone();
        let dial_invite = invite.clone();
        let dial_guest = guest.clone();
        runtime.spawn(async move {
            let _ = dial(&dial_guest, &dial_invite, &dial_status).await;
        });
        let local = loop {
            match get(&tunnel_status) {
                Progress::Ready(local) => break local,
                Progress::Failed(error) => panic!("tunnel failed: {error}"),
                Progress::Preparing => std::thread::sleep(Duration::from_millis(20)),
            }
        };
        let mut stream = std::net::TcpStream::connect(local).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        writeln!(stream, "hello").unwrap();
        let mut reply = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut reply)
            .unwrap();
        assert_eq!(reply.trim(), "host got hello");

        // Wrong secret: refused before reaching the Jam.
        let mut forged = invite.clone();
        forged.secret[0] ^= 0xff;
        let intruder = runtime.block_on(bind(vec![])).unwrap();
        let refused = runtime.block_on(async {
            let connection = intruder
                .connect(forged.host.clone(), &alpn())
                .await
                .unwrap();
            open_proved(&connection, &forged.secret).await
        });
        assert!(refused.is_err());

        // A different protocol is refused at the handshake.
        let other = runtime.block_on(async {
            intruder
                .connect(forged.host.clone(), b"something/else")
                .await
        });
        assert!(other.is_err());
        drop(stream);
        runtime.shutdown_background();
    }
}
