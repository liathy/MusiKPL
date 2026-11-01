//! Embeds MusiKPL's Last.fm key and secret (from git-ignored files in the
//! repository root, or environment variables), so builds can use Last.fm
//! without the key being stored in the repository.

use std::{env, fs, path::PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let root = manifest
        .parent()
        .expect("core/ sits in the repository")
        .to_path_buf();
    embed_lastfm_key(&root);
    embed_lastfm_secret(&root);
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
