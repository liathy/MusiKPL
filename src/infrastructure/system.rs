//! Small operating-system integrations: the screen's usable area, opening a
//! link in the default browser, starting with Windows, and the computer's
//! network adapters.

use std::{net::Ipv4Addr, process::Command};

use super::process::hide_console;

/// The IPv4 address and prefix length of every active network adapter,
/// loopback excluded. Used to send Jam discovery out of each one: a PC with
/// a VPN or virtual adapter would otherwise only reach one network.
#[cfg(windows)]
pub(crate) fn ipv4_networks() -> Vec<(Ipv4Addr, u8)> {
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
pub(crate) fn ipv4_networks() -> Vec<(Ipv4Addr, u8)> {
    Vec::new()
}

/// Open a web page in the default browser.
pub(crate) fn open_url(url: &str) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Only web links can be opened".into());
    }
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("rundll32.exe");
        command.args(["url.dll,FileProtocolHandler", url]);
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    hide_console(&mut command);
    command
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open the browser: {e}"))
}

#[cfg(windows)]
mod autostart {
    use windows_sys::Win32::System::Registry::{
        RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
    };

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "MusiKPL";
    const ERROR_FILE_NOT_FOUND: u32 = 2;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    pub(super) fn enabled() -> bool {
        let (key, name) = (wide(RUN_KEY), wide(VALUE_NAME));
        let mut size = 0u32;
        // SAFETY: the strings are NUL-terminated; a null buffer only asks
        // whether the value exists and how large it is.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        status == 0
    }

    pub(super) fn set(enabled: bool) -> Result<(), String> {
        let (key, name) = (wide(RUN_KEY), wide(VALUE_NAME));
        let status = if enabled {
            let exe =
                std::env::current_exe().map_err(|e| format!("Could not find MusiKPL.exe: {e}"))?;
            let command = wide(&format!("\"{}\"", exe.display()));
            // SAFETY: all pointers are valid NUL-terminated UTF-16 buffers and
            // the size is the byte length of `command`, terminator included.
            unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    name.as_ptr(),
                    REG_SZ,
                    command.as_ptr().cast(),
                    (command.len() * 2) as u32,
                )
            }
        } else {
            // SAFETY: valid NUL-terminated UTF-16 strings.
            match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) } {
                ERROR_FILE_NOT_FOUND => 0,
                status => status,
            }
        };
        if status == 0 {
            Ok(())
        } else {
            Err(format!(
                "Windows refused to change the startup setting (error {status})"
            ))
        }
    }
}

/// Whether MusiKPL is registered to start when the user signs in.
pub(crate) fn starts_with_windows() -> bool {
    #[cfg(windows)]
    {
        autostart::enabled()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub(crate) fn set_start_with_windows(enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        autostart::set(enabled)
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Err("Starting with the system is only supported on Windows".into())
    }
}
