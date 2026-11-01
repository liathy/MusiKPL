//! The device's networks, for sending LAN Jam discovery out of each one.

use std::{net::Ipv4Addr, sync::Mutex};

/// Networks the platform reported itself (Android passes its Wi-Fi
/// network in), used instead of asking the system.
static GIVEN: Mutex<Option<Vec<(Ipv4Addr, u8)>>> = Mutex::new(None);

/// Use these networks (address, prefix length) from now on.
pub fn set_networks(networks: Vec<(Ipv4Addr, u8)>) {
    if let Ok(mut given) = GIVEN.lock() {
        *given = Some(networks);
    }
}

/// The IPv4 address and prefix length of every active network, loopback
/// excluded. Empty when unknown; discovery then broadcasts to everyone.
pub fn ipv4_networks() -> Vec<(Ipv4Addr, u8)> {
    if let Some(given) = GIVEN.lock().ok().and_then(|given| given.clone()) {
        return given;
    }
    system_networks()
}

/// The IPv4 address and prefix length of every active network adapter,
/// loopback excluded. Used to send Jam discovery out of each one: a PC with
/// a VPN or virtual adapter would otherwise only reach one network.
#[cfg(windows)]
fn system_networks() -> Vec<(Ipv4Addr, u8)> {
    use windows_sys::Win32::{
        NetworkManagement::{
            IpHelper::{
                GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
                GAA_FLAG_SKIP_FRIENDLY_NAME, GAA_FLAG_SKIP_MULTICAST, IF_TYPE_SOFTWARE_LOOPBACK,
                IP_ADAPTER_ADDRESSES_LH,
            },
            Ndis::IfOperStatusUp,
        },
        Networking::WinSock::{AF_INET, SOCKADDR_IN},
    };
    const ERROR_BUFFER_OVERFLOW: u32 = 111;

    let flags = GAA_FLAG_SKIP_ANYCAST
        | GAA_FLAG_SKIP_MULTICAST
        | GAA_FLAG_SKIP_DNS_SERVER
        | GAA_FLAG_SKIP_FRIENDLY_NAME;
    // u64 elements keep the buffer aligned for the structures written to it.
    let mut size: u32 = 16 * 1024;
    let mut buffer: Vec<u64> = Vec::new();
    for _ in 0..3 {
        buffer.resize((size as usize).div_ceil(8), 0);
        // SAFETY: the buffer holds `size` writable bytes, as the call requires.
        let status = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_INET),
                flags,
                std::ptr::null(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        match status {
            0 => break,
            ERROR_BUFFER_OVERFLOW => continue,
            _ => return Vec::new(),
        }
    }

    let mut networks = Vec::new();
    let mut adapter = buffer.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    // SAFETY: on success the buffer holds a linked list of adapters whose
    // pointers stay valid while `buffer` lives; each is checked for null.
    unsafe {
        while !adapter.is_null() {
            let current = &*adapter;
            if current.OperStatus == IfOperStatusUp && current.IfType != IF_TYPE_SOFTWARE_LOOPBACK {
                let mut unicast = current.FirstUnicastAddress;
                while !unicast.is_null() {
                    let entry = &*unicast;
                    let sockaddr = entry.Address.lpSockaddr;
                    if !sockaddr.is_null() && (*sockaddr).sa_family == AF_INET {
                        let address = &*sockaddr.cast::<SOCKADDR_IN>();
                        let ip = Ipv4Addr::from(u32::from_be(address.sin_addr.S_un.S_addr));
                        networks.push((ip, entry.OnLinkPrefixLength));
                    }
                    unicast = entry.Next;
                }
            }
            adapter = current.Next;
        }
    }
    networks
}

#[cfg(not(windows))]
fn system_networks() -> Vec<(Ipv4Addr, u8)> {
    Vec::new()
}
