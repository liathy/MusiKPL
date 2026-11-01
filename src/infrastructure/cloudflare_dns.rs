//! Looking up addresses through Cloudflare's 1.1.1.1 instead of the
//! internet provider's DNS, so a provider blocking GitHub by DNS can't stop
//! updates.
//!
//! The lookup goes over HTTPS (DNS over HTTPS) straight to 1.1.1.1, or
//! 1.0.0.1 if that fails, so it can't be read or changed on the way. If both
//! fail, the system's own lookup is used, as before.

use std::{
    collections::HashMap,
    io,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use serde_json::Value;

const SERVERS: [&str; 2] = ["https://1.1.1.1/dns-query", "https://1.0.0.1/dns-query"];
/// How long a found address is reused, at most (DNS answers carry their own,
/// usually shorter, lifetime).
const MAX_KEEP: Duration = Duration::from_secs(10 * 60);

/// A ureq resolver that asks Cloudflare.
pub(crate) struct CloudflareDns;

impl ureq::Resolver for CloudflareDns {
    fn resolve(&self, netloc: &str) -> io::Result<Vec<SocketAddr>> {
        let (host, port) = split(netloc)?;
        if let Ok(ip) = host.parse::<IpAddr>() {
            return Ok(vec![SocketAddr::new(ip, port)]);
        }
        match lookup(host) {
            Some(ips) => Ok(ips
                .into_iter()
                .map(|ip| SocketAddr::new(ip, port))
                .collect()),
            None => netloc.to_socket_addrs().map(Iterator::collect),
        }
    }
}

/// "host:port" (or "[v6]:port") into its parts.
fn split(netloc: &str) -> io::Result<(&str, u16)> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "bad address");
    let (host, port) = netloc.rsplit_once(':').ok_or_else(invalid)?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    Ok((host, port.parse().map_err(|_| invalid())?))
}

type Cache = Mutex<HashMap<String, (Vec<IpAddr>, Instant)>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(Cache::default)
}

/// `host`'s IPv4 addresses from Cloudflare, or None when it can't answer.
fn lookup(host: &str) -> Option<Vec<IpAddr>> {
    let key = host.to_ascii_lowercase();
    if let Some((ips, until)) = cache().lock().ok()?.get(&key) {
        if Instant::now() < *until {
            return Some(ips.clone());
        }
    }
    // Its own agent, reaching Cloudflare by address, so no lookup is needed.
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(5))
        .user_agent(concat!("MusiKPL/", env!("CARGO_PKG_VERSION")))
        .build();
    for server in SERVERS {
        let Ok(response) = agent
            .get(server)
            .query("name", &key)
            .query("type", "A")
            .set("Accept", "application/dns-json")
            .call()
        else {
            continue;
        };
        let Some(value) = response
            .into_string()
            .ok()
            .and_then(|body| serde_json::from_str::<Value>(&body).ok())
        else {
            continue;
        };
        if let Some((ips, ttl)) = parse_answer(&value) {
            let keep = Duration::from_secs(ttl).min(MAX_KEEP);
            if let Ok(mut cache) = cache().lock() {
                cache.insert(key, (ips.clone(), Instant::now() + keep));
            }
            return Some(ips);
        }
    }
    None
}

/// The A records of a DNS JSON answer, with the shortest lifetime. CNAMEs
/// in the chain are skipped: Cloudflare follows them itself.
fn parse_answer(value: &Value) -> Option<(Vec<IpAddr>, u64)> {
    if value.get("Status").and_then(Value::as_u64) != Some(0) {
        return None;
    }
    let mut ttl = u64::MAX;
    let ips: Vec<IpAddr> = value
        .get("Answer")?
        .as_array()?
        .iter()
        .filter(|record| record.get("type").and_then(Value::as_u64) == Some(1))
        .filter_map(|record| {
            let ip = record.get("data")?.as_str()?.parse().ok()?;
            ttl = ttl.min(record.get("TTL").and_then(Value::as_u64).unwrap_or(60));
            Some(ip)
        })
        .collect();
    (!ips.is_empty()).then_some((ips, ttl))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_read_from_a_dns_answer() {
        let value: Value = serde_json::from_str(
            r#"{"Status":0,"Answer":[
                {"name":"github.com","type":5,"TTL":300,"data":"x.github.com."},
                {"name":"x.github.com","type":1,"TTL":60,"data":"140.82.121.4"},
                {"name":"x.github.com","type":1,"TTL":30,"data":"140.82.121.3"}
            ]}"#,
        )
        .unwrap();
        let (ips, ttl) = parse_answer(&value).unwrap();
        assert_eq!(ips.len(), 2);
        assert_eq!(ttl, 30);
        let failed: Value = serde_json::from_str(r#"{"Status":3}"#).unwrap();
        assert!(parse_answer(&failed).is_none());
    }

    #[test]
    fn host_and_port_are_split() {
        assert_eq!(split("github.com:443").unwrap(), ("github.com", 443));
        assert_eq!(split("[::1]:80").unwrap(), ("::1", 80));
        assert!(split("github.com").is_err());
    }

    /// Needs the internet: asks 1.1.1.1 for GitHub's address.
    #[test]
    #[ignore]
    fn github_is_found_through_cloudflare() {
        assert!(lookup("api.github.com").is_some_and(|ips| !ips.is_empty()));
    }
}
