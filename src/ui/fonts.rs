//! The design's typefaces: Baloo Thambi for titles and headings, Baloo
//! Thambi 2 for everything else. Both are SIL Open Font License fonts from
//! Google Fonts, bundled into the executable.
//!
//! Song titles come in every script. Characters those fonts (and egui's
//! built-in fallbacks) lack would show as squares, so Windows' own fonts
//! are added as fallbacks: each only the first time some text needs it,
//! read on a background thread, so nobody pays memory for scripts they
//! never see.

use std::{
    collections::HashSet,
    sync::{LazyLock, Mutex},
};

use eframe::egui::{self, FontData, FontDefinitions, FontFamily, FontId};

const DISPLAY: &str = "display";
const BALOO_THAMBI: &[u8] = include_bytes!("../assets/fonts/BalooThambi-Regular.ttf");
const BALOO_THAMBI_2: &[u8] = include_bytes!("../assets/fonts/BalooThambi2-Regular.ttf");

/// A Windows font that covers scripts the bundled fonts don't.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Fallback {
    /// Segoe UI: Greek, Cyrillic, Armenian, Hebrew, Arabic, Georgian, and
    /// extended Latin (Vietnamese and more).
    World,
    /// Segoe UI Symbol: arrows, math, music, dingbats, many emoji.
    Symbols,
    /// Yu Gothic: Japanese kana (and its own kanji).
    Japanese,
    /// Microsoft YaHei: Chinese characters, full-width forms.
    Chinese,
    /// Malgun Gothic: Korean.
    Korean,
    /// Leelawadee UI: Thai, Lao, Khmer.
    Thai,
    /// Nirmala UI: Hindi and the other Indian scripts.
    Indic,
}

impl Fallback {
    /// In fallback order: Japanese before Chinese, so Japanese titles use
    /// Japanese forms of shared characters.
    const ALL: [Fallback; 7] = [
        Fallback::World,
        Fallback::Japanese,
        Fallback::Chinese,
        Fallback::Korean,
        Fallback::Thai,
        Fallback::Indic,
        Fallback::Symbols,
    ];

    fn file(self) -> &'static str {
        match self {
            Fallback::World => "segoeui.ttf",
            Fallback::Symbols => "seguisym.ttf",
            Fallback::Japanese => "YuGothM.ttc",
            Fallback::Chinese => "msyh.ttc",
            Fallback::Korean => "malgun.ttf",
            Fallback::Thai => "LeelawUI.ttf",
            Fallback::Indic => "Nirmala.ttc",
        }
    }

    fn name(self) -> String {
        format!("fallback-{self:?}")
    }

    /// The fallback a character needs, if any.
    fn for_char(c: char) -> Option<Fallback> {
        let code = c as u32;
        Some(match code {
            0x0000..=0x024F => return None,
            0x0250..=0x058F | 0x0590..=0x08FF | 0x10A0..=0x10FF | 0x1E00..=0x1FFF => {
                Fallback::World
            }
            0xFB00..=0xFDFF | 0xFE70..=0xFEFF => Fallback::World,
            0x0900..=0x0DFF => Fallback::Indic,
            0x0E00..=0x0EFF | 0x1780..=0x17FF => Fallback::Thai,
            0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF => {
                Fallback::Korean
            }
            0x3040..=0x30FF | 0x31F0..=0x31FF => Fallback::Japanese,
            0x2E80..=0x2FFF
            | 0x3000..=0x303F
            | 0x3100..=0x312F
            | 0x3190..=0x31EF
            | 0x3200..=0x9FFF
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFFEF
            | 0x20000..=0x3FFFF => Fallback::Chinese,
            0x2000..=0x2BFF | 0x1F000..=0x1FAFF => Fallback::Symbols,
            _ => return None,
        })
    }
}

#[derive(Default)]
struct Fallbacks {
    /// Asked for (loading or loaded), so each is read once.
    requested: HashSet<Fallback>,
    /// Read from disk, waiting to be installed on the next frame.
    loaded: Vec<(Fallback, Vec<u8>)>,
    /// Installed, with their data, so fonts can be rebuilt.
    installed: Vec<(Fallback, Vec<u8>)>,
}

static FALLBACKS: LazyLock<Mutex<Fallbacks>> = LazyLock::new(Default::default);

fn fallbacks() -> std::sync::MutexGuard<'static, Fallbacks> {
    FALLBACKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn install(ctx: &egui::Context) {
    ctx.set_fonts(definitions(&fallbacks().installed));
}

fn definitions(extra: &[(Fallback, Vec<u8>)]) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("baloo-thambi".into(), FontData::from_static(BALOO_THAMBI));
    fonts.font_data.insert(
        "baloo-thambi-2".into(),
        FontData::from_static(BALOO_THAMBI_2),
    );

    // egui's bundled fonts stay as fallbacks for symbols Baloo lacks, then
    // the Windows fonts loaded so far.
    let mut fallback_names = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    for fallback in Fallback::ALL {
        if let Some((_, data)) = extra.iter().find(|(loaded, _)| *loaded == fallback) {
            let name = fallback.name();
            fonts
                .font_data
                .insert(name.clone(), FontData::from_owned(data.clone()));
            fallback_names.push(name);
        }
    }
    let mut body = vec!["baloo-thambi-2".to_owned()];
    body.extend(fallback_names.iter().cloned());
    let mut display = vec!["baloo-thambi".to_owned(), "baloo-thambi-2".to_owned()];
    display.extend(fallback_names);
    fonts.families.insert(FontFamily::Proportional, body);
    fonts
        .families
        .insert(FontFamily::Name(DISPLAY.into()), display);
    fonts
}

/// Make sure `text` can be drawn: start loading the fonts its characters
/// need. Cheap for text that needs nothing new, so it can run every frame.
pub(crate) fn cover(ctx: &egui::Context, text: &str) {
    if text.is_ascii() {
        return;
    }
    let mut needed: Vec<Fallback> = Vec::new();
    for c in text.chars() {
        if let Some(fallback) = Fallback::for_char(c) {
            if !needed.contains(&fallback) {
                needed.push(fallback);
            }
        }
    }
    if needed.is_empty() {
        return;
    }
    let mut state = fallbacks();
    for fallback in needed {
        if !state.requested.insert(fallback) {
            continue;
        }
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let Some(windows) = std::env::var_os("WINDIR") else {
                return;
            };
            let path = std::path::Path::new(&windows)
                .join("Fonts")
                .join(fallback.file());
            // egui stops the app on a font it can't read, so check first.
            let readable =
                |data: &[u8]| ab_glyph::FontRef::try_from_slice_and_index(data, 0).is_ok();
            if let Some(data) = std::fs::read(path).ok().filter(|data| readable(data)) {
                fallbacks().loaded.push((fallback, data));
                ctx.request_repaint();
            }
        });
    }
}

/// Install fonts that finished loading. Call once per frame, before drawing.
pub(crate) fn apply_loaded(ctx: &egui::Context) {
    let mut state = fallbacks();
    if state.loaded.is_empty() {
        return;
    }
    let loaded = std::mem::take(&mut state.loaded);
    state.installed.extend(loaded);
    ctx.set_fonts(definitions(&state.installed));
}

/// Baloo Thambi: song titles, headings, and large labels.
pub(crate) fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY.into()))
}

/// Baloo Thambi 2: body text, captions, and inputs.
pub(crate) fn body(size: f32) -> FontId {
    FontId::proportional(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn characters_map_to_the_font_that_has_them() {
        assert_eq!(Fallback::for_char('a'), None);
        assert_eq!(Fallback::for_char('é'), None);
        assert_eq!(Fallback::for_char('Ж'), Some(Fallback::World));
        assert_eq!(Fallback::for_char('ش'), Some(Fallback::World));
        assert_eq!(Fallback::for_char('ア'), Some(Fallback::Japanese));
        assert_eq!(Fallback::for_char('漢'), Some(Fallback::Chinese));
        assert_eq!(Fallback::for_char('한'), Some(Fallback::Korean));
        assert_eq!(Fallback::for_char('ไ'), Some(Fallback::Thai));
        assert_eq!(Fallback::for_char('ह'), Some(Fallback::Indic));
        assert_eq!(Fallback::for_char('♫'), Some(Fallback::Symbols));
    }

    #[test]
    fn every_fallback_font_exists_on_windows_and_can_be_read() {
        let Some(windows) = std::env::var_os("WINDIR") else {
            return;
        };
        for fallback in Fallback::ALL {
            let path = std::path::Path::new(&windows)
                .join("Fonts")
                .join(fallback.file());
            let data = std::fs::read(&path).expect("the font is there");
            assert!(
                ab_glyph::FontRef::try_from_slice_and_index(&data, 0).is_ok(),
                "{} can't be read",
                path.display()
            );
        }
    }
}
