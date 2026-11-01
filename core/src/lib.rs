//! MusiKPL's shared core: everything that isn't drawing or the operating
//! system. The desktop app (egui, mpv, yt-dlp) and the Android app (Compose,
//! ExoPlayer, NewPipe) are shells around it.
//!
//! What differs per platform comes in through small interfaces:
//! [`audio::AudioOutput`] plays a stream, [`extractor::Extractor`] finds songs
//! and their audio, and [`platform`] opens links and wakes the interface.

pub mod audio;
pub mod cloudflare_dns;
pub mod covers;
pub mod devices;
pub mod domain;
pub mod downloads;
pub mod engine;
pub mod extractor;
pub mod friends;
pub mod jam;
pub mod lastfm;
pub mod lyrics;
pub mod net;
pub mod platform;
pub mod playback;
pub mod recommendations;
pub mod settings;
pub mod storage;
pub mod updates;
