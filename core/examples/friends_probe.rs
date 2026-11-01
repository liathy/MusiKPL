//! A MusiKPL friend without an interface, for testing friends between
//! devices: presence (who's online) and Jam invites.
//!
//!   cargo run -p musikpl-core --example friends_probe -- FRIEND_ID
//!
//! It keeps its own identity in a temporary folder (never the user's), prints
//! its friend code to add on the other device, then reports for two minutes
//! whether the friend is online and any Jam invite they send.

use std::time::{Duration, Instant};

use musikpl_core::{
    friends::{my_code, FriendEvent, Friends},
    platform::quiet_waker,
    storage,
};

fn main() {
    storage::set_data_dir(std::env::temp_dir().join("musikpl-friends-probe"));
    let arg = std::env::args()
        .nth(1)
        .expect("the friend's id, or their friend-key.txt (64 hex digits)");
    let arg = arg.trim().trim_matches('"').to_owned();
    // A device's friend-key.txt (testing with a debug build): its public id.
    let friend = if arg.len() == 64 && arg.chars().all(|c| c.is_ascii_hexdigit()) {
        let bytes: Vec<u8> = (0..32)
            .map(|i| u8::from_str_radix(&arg[i * 2..i * 2 + 2], 16).unwrap())
            .collect();
        let bytes: [u8; 32] = bytes.try_into().unwrap();
        iroh::SecretKey::from_bytes(&bytes).public().to_string()
    } else {
        arg
    };
    let code = my_code("PC probe").encode();
    println!("my friend code: {code}");
    std::fs::write(
        std::env::temp_dir().join("musikpl-friends-probe-code.txt"),
        &code,
    )
    .ok();
    let friends = Friends::start(std::slice::from_ref(&friend), "PC probe", quiet_waker());
    let started = Instant::now();
    let mut last_check = Instant::now() - Duration::from_secs(60);
    while started.elapsed() < Duration::from_secs(120) {
        if last_check.elapsed() > Duration::from_secs(5) {
            last_check = Instant::now();
            friends.check(vec![friend.clone()]);
        }
        for event in friends.poll() {
            match event {
                FriendEvent::Presence { online, .. } => {
                    println!(
                        "[{:>3}s] friend online: {online}",
                        started.elapsed().as_secs()
                    )
                }
                FriendEvent::Invited { name, invite, .. } => {
                    println!(
                        "[{:>3}s] {name} invited me to their Jam ({} characters)",
                        started.elapsed().as_secs(),
                        invite.len()
                    );
                    return;
                }
                other => println!("[{:>3}s] {other:?}", started.elapsed().as_secs()),
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
