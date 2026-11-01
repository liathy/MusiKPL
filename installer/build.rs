//! Embeds the release build of MusiKPL.exe (`target/release/MusiKPL.exe`,
//! built first by `scripts/build-release.ps1`) into the setup. Without it
//! the setup still builds, and says it has nothing to install.

use std::{env, fs, path::PathBuf};

#[path = "../build_icon.rs"]
mod icon;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let out = PathBuf::from(env::var("OUT_DIR").expect("set by cargo"));
    let app = manifest
        .parent()
        .expect("installer/ sits in the project")
        .join("target")
        .join("release")
        .join("MusiKPL.exe");
    println!("cargo:rerun-if-changed={}", app.display());
    let bytes = fs::read(&app).unwrap_or_else(|_| {
        println!(
            "cargo:warning={} is missing, so this setup has nothing to install; build MusiKPL in release first",
            app.display()
        );
        Vec::new()
    });
    fs::write(out.join("MusiKPL.exe"), bytes).expect("write the embedded app");

    // The version shown in Installed apps is MusiKPL's own.
    let root_manifest = manifest.parent().expect("project root").join("Cargo.toml");
    println!("cargo:rerun-if-changed={}", root_manifest.display());
    let version = fs::read_to_string(&root_manifest)
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.trim().strip_prefix("version = \"")?.strip_suffix('"'))
                .map(str::to_owned)
        })
        .unwrap_or_else(|| env::var("CARGO_PKG_VERSION").expect("set by cargo"));
    println!("cargo:rustc-env=MUSIKPL_VERSION={version}");

    icon::build(
        &manifest.parent().expect("project root").join("logo.svg"),
        &out,
    );
}
