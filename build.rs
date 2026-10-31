//! Packs mpv.exe and yt-dlp.exe (from the project root) into MusiKPL.exe for
//! release builds, so the app ships as a single file. Debug builds and tests
//! skip this and use the executables beside them or in the project root.

use std::{env, fs, path::PathBuf};

const TOOLS: [&str; 2] = ["mpv.exe", "yt-dlp.exe"];

fn main() {
    println!("cargo:rustc-check-cfg=cfg(bundled_tools)");
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let out = PathBuf::from(env::var("OUT_DIR").expect("set by cargo"));
    let release = env::var("PROFILE").as_deref() == Ok("release");

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
}
