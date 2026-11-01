//! Downloads a stream link with MusiKPL's downloader, to check it works:
//!
//!     cargo run -p musikpl-core --example download_probe -- <url> <file>

use std::sync::atomic::{AtomicBool, AtomicU32};

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(url), Some(file)) = (args.next(), args.next()) else {
        eprintln!("usage: download_probe <url> <file>");
        std::process::exit(2);
    };
    let started = std::time::Instant::now();
    let progress = AtomicU32::new(0);
    match musikpl_core::downloads::fetch(&url, file.as_ref(), &progress, &AtomicBool::new(false)) {
        Ok(bytes) => println!(
            "saved {bytes} bytes in {:.1}s ({} per mille)",
            started.elapsed().as_secs_f64(),
            progress.into_inner()
        ),
        Err(error) => {
            eprintln!("failed: {error}");
            std::process::exit(1);
        }
    }
}
