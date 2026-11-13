//! Embedded fonts, Lucide icons and themes, served to GPUI as an `AssetSource`.

use std::borrow::Cow;

use anyhow::Result;
use gpui::{App, AssetSource, SharedString};

macro_rules! embed {
    ($($path:literal),* $(,)?) => {
        &[$(($path, include_bytes!(concat!("../assets/", $path)).as_slice())),*]
    };
}

const FILES: &[(&str, &[u8])] = embed![
    "icons/check.svg",
    "icons/calendar.svg",
    "icons/list-tree.svg",
    "icons/undo-2.svg",
    "icons/external-link.svg",
    "icons/download.svg",
    "icons/upload.svg",
    "icons/pencil.svg",
    "icons/trash-2.svg",
    "icons/copy.svg",
    "icons/braces.svg",
    "icons/chevron-left.svg",
    "icons/arrow-up.svg",
    "icons/arrow-down.svg",
    "icons/ban.svg",
    "icons/exclamation.svg",
    "icons/zap.svg",
    "icons/wifi.svg",
    "icons/wifi-off.svg",
    "icons/dice.svg",
    "icons/star.svg",
    "icons/chevron-down.svg",
    "icons/chevron-right.svg",
    "icons/circle-x.svg",
    "icons/columns-2.svg",
    "icons/ellipsis.svg",
    "icons/file-text.svg",
    "icons/folder-open.svg",
    "icons/folder.svg",
    "icons/globe.svg",
    "icons/info.svg",
    "icons/layers.svg",
    "icons/database.svg",
    "icons/table.svg",
    "icons/lock.svg",
    "icons/ellipsis-vertical.svg",
    "icons/minus.svg",
    "icons/palette.svg",
    "icons/pin.svg",
    "icons/play.svg",
    "icons/plus.svg",
    "icons/refresh-cw.svg",
    "icons/rows-2.svg",
    "icons/search.svg",
    "icons/server.svg",
    "icons/square.svg",
    "icons/terminal.svg",
    "icons/triangle-alert.svg",
    "icons/x.svg",
    "icons/eye.svg",
    "icons/eye-off.svg",
    "icons/book-open.svg",
    "icons/menu.svg",
    "brand/fauxfield.svg",
];

const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/IBMPlexSans_400Regular.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans_500Medium.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans_600SemiBold.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono_400Regular.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono_400Regular_Italic.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono_700Bold.ttf"),
];

/// UI font family (SPEC §9.1).
pub const UI_FONT: &str = "IBM Plex Sans";
/// Buffer font. SPEC §9.1 names IBM Plex Mono; it is not bundled yet (docs/adr/002), so
/// JetBrains Mono (OFL) stands in. Swap this constant and `FONTS` when the Plex Mono files land.
pub const BUFFER_FONT: &str = "JetBrains Mono";

/// The bundled theme family (Zed theme JSON, schema v0.2.0).
pub const BUNDLED_THEME_FAMILIES: &[&str] = &[include_str!("../assets/themes/one.json")];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(FILES
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, b)| Cow::Borrowed(*b)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(FILES
            .iter()
            .filter(|(p, _)| p.starts_with(path))
            .map(|(p, _)| SharedString::from(*p))
            .collect())
    }
}

pub fn load_fonts(cx: &App) -> Result<()> {
    cx.text_system()
        .add_fonts(FONTS.iter().map(|b| Cow::Borrowed(*b)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_on_disk_is_embedded() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons");
        for e in std::fs::read_dir(dir).unwrap().filter_map(Result::ok) {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".svg") {
                let path = format!("icons/{name}");
                assert!(
                    Assets.load(&path).unwrap().is_some(),
                    "{path} is on disk but not embedded"
                );
            }
        }
    }
}
