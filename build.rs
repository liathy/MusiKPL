//! Packs mpv.exe and yt-dlp.exe (from the project root) into MusiKPL.exe for
//! Windows release builds, so the app ships as a single file. Debug builds and tests
//! skip this and use the executables beside them or in the project root.

use std::{env, fs, path::PathBuf};

#[path = "build_icon.rs"]
mod icon;

const TOOLS: [&str; 2] = ["mpv.exe", "yt-dlp.exe"];

fn main() {
    println!("cargo:rustc-check-cfg=cfg(bundled_tools)");
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let out = PathBuf::from(env::var("OUT_DIR").expect("set by cargo"));
    // Only Windows builds carry mpv.exe and yt-dlp.exe; elsewhere the system's
    // mpv and yt-dlp are used.
    let windows = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let release = env::var("PROFILE").as_deref() == Ok("release") && windows;

    let mut bundle_id: u64 = 0xcbf2_9ce4_8422_2325;
    let mut complete = release;
    for name in TOOLS {
        let source = root.join(name);
        println!("cargo:rerun-if-changed={}", source.display());
        let target = out.join(format!("{name}.deflate"));
        let packed = match (release, fs::read(&source)) {
            (true, Ok(bytes)) => miniz_oxide::deflate::compress_to_vec(&bytes, 6),
            (true, Err(_)) => {
                println!(
                    "cargo:warning={name} was not found beside Cargo.toml, so it is not bundled; \
                     run scripts/install-dependencies.ps1 first"
                );
                complete = false;
                Vec::new()
            }
            (false, _) => Vec::new(),
        };
        // FNV-1a over the packed bytes identifies this exact bundle.
        for byte in &packed {
            bundle_id = (bundle_id ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
        fs::write(&target, packed).expect("write packed tool");
    }
    println!("cargo:rustc-env=MUSIKPL_BUNDLE_ID={bundle_id:016x}");
    if complete {
        println!("cargo:rustc-cfg=bundled_tools");
    }
    embed_lastfm_key(&root);
    embed_lastfm_secret(&root);
    icon::build(&root.join("logo.svg"), &out);
}

/// MusiKPL's read-only Last.fm API key, from the `MUSIKPL_LASTFM_KEY`
/// environment variable or `lastfm-api-key.txt` (ignored by git), so the key
/// ships in builds without being stored in the repository. Builds without
/// it still work; genres and covers then need the user's own key.
fn embed_lastfm_key(root: &std::path::Path) {
    let file = root.join("lastfm-api-key.txt");
    println!("cargo:rerun-if-changed={}", file.display());
    println!("cargo:rerun-if-env-changed=MUSIKPL_LASTFM_KEY");
    let key = env::var("MUSIKPL_LASTFM_KEY")
        .ok()
        .or_else(|| fs::read_to_string(&file).ok())
        .map(|key| key.trim().to_owned())
        .unwrap_or_default();
    let valid = key.len() == 32 && key.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !key.is_empty() && !valid {
        println!("cargo:warning=lastfm-api-key.txt doesn't hold a 32-character Last.fm API key; ignoring it");
    }
    println!(
        "cargo:rustc-env=MUSIKPL_LASTFM_KEY={}",
        if valid { key.as_str() } else { "" }
    );
}

/// MusiKPL's Last.fm shared secret, from `MUSIKPL_LASTFM_SECRET` or
/// `lastfm-api-secret.txt` (ignored by git). With it, users connect their
/// account in one click; without it, they enter their own key and secret.
fn embed_lastfm_secret(root: &std::path::Path) {
    let file = root.join("lastfm-api-secret.txt");
    println!("cargo:rerun-if-changed={}", file.display());
    println!("cargo:rerun-if-env-changed=MUSIKPL_LASTFM_SECRET");
    let secret = env::var("MUSIKPL_LASTFM_SECRET")
        .ok()
        .or_else(|| fs::read_to_string(&file).ok())
        .map(|secret| secret.trim().to_owned())
        .unwrap_or_default();
    let valid = secret.len() == 32 && secret.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !secret.is_empty() && !valid {
        println!("cargo:warning=lastfm-api-secret.txt doesn't hold a 32-character shared secret; ignoring it");
    }
    println!(
        "cargo:rustc-env=MUSIKPL_LASTFM_SECRET={}",
        if valid { secret.as_str() } else { "" }
    );
}
