//! Embedded variable fonts bundled directly into the AIOS binary (SIL OFL 1.1).
//!
//! - Sans: `Inter Variable` (`assets/fonts/InterVariable.ttf`)
//! - Mono: `JetBrains Mono` (`assets/fonts/JetBrainsMonoVariable.ttf`)

use std::borrow::Cow;

use gpui::{App, AssetSource, SharedString};

/// Registered family name inside `assets/fonts/InterVariable.ttf`.
pub const SANS_FAMILY: &str = "Inter Variable";

/// Registered family name inside `assets/fonts/JetBrainsMonoVariable.ttf`.
pub const MONO_FAMILY: &str = "JetBrains Mono";

const INTER_VARIABLE_TTF: &[u8] = include_bytes!("../../../assets/fonts/InterVariable.ttf");
const JETBRAINS_MONO_VARIABLE_TTF: &[u8] =
    include_bytes!("../../../assets/fonts/JetBrainsMonoVariable.ttf");

// GPUI's `SvgRenderer` queries these bundled fallback paths from the app's `AssetRegistry`
// when rasterizing SVG desktop icons. Serving AIOS's embedded fonts resolves them cleanly
// without missing-font warnings.
const GPUI_SVG_SANS_PATH: &str = "fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf";
const GPUI_SVG_MONO_PATH: &str = "fonts/lilex/Lilex-Regular.ttf";

/// In-memory GPUI [`AssetSource`] serving AIOS's embedded fonts for `SvgRenderer`.
#[derive(Debug, Default, Clone, Copy)]
pub struct AiosAssetSource;

impl AssetSource for AiosAssetSource {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        match path {
            GPUI_SVG_SANS_PATH | "fonts/InterVariable.ttf" => {
                Ok(Some(Cow::Borrowed(INTER_VARIABLE_TTF)))
            }
            GPUI_SVG_MONO_PATH | "fonts/JetBrainsMonoVariable.ttf" => {
                Ok(Some(Cow::Borrowed(JETBRAINS_MONO_VARIABLE_TTF)))
            }
            _ => Ok(None),
        }
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let all = [
            GPUI_SVG_SANS_PATH,
            GPUI_SVG_MONO_PATH,
            "fonts/InterVariable.ttf",
            "fonts/JetBrainsMonoVariable.ttf",
        ];
        Ok(all
            .into_iter()
            .filter(|entry| path.is_empty() || entry.starts_with(path))
            .map(SharedString::from)
            .collect())
    }
}

/// Registers AIOS's bundled Sans (`Inter Variable`) and Mono (`JetBrains Mono`) fonts
/// with GPUI's text system so typography renders identically across all Linux distros.
pub fn register_embedded_fonts(cx: &App) {
    let fonts = vec![
        Cow::Borrowed(INTER_VARIABLE_TTF),
        Cow::Borrowed(JETBRAINS_MONO_VARIABLE_TTF),
    ];
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        tracing::warn!(error = %err, "failed to register embedded AIOS fonts");
    } else {
        tracing::debug!(
            sans = SANS_FAMILY,
            mono = MONO_FAMILY,
            "registered embedded AIOS fonts"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_font_blobs_are_valid_truetype_headers() {
        // TrueType/OpenType sfnt headers start with 0x00010000 or "OTTO".
        assert!(INTER_VARIABLE_TTF.len() > 100_000);
        assert!(JETBRAINS_MONO_VARIABLE_TTF.len() > 100_000);
        assert_eq!(&INTER_VARIABLE_TTF[0..4], &[0x00, 0x01, 0x00, 0x00]);
        assert_eq!(
            &JETBRAINS_MONO_VARIABLE_TTF[0..4],
            &[0x00, 0x01, 0x00, 0x00]
        );
    }

    #[test]
    fn asset_source_serves_gpui_svg_renderer_bundled_fonts() {
        let source = AiosAssetSource;
        assert!(
            source
                .load(GPUI_SVG_SANS_PATH)
                .expect("load sans")
                .is_some()
        );
        assert!(
            source
                .load(GPUI_SVG_MONO_PATH)
                .expect("load mono")
                .is_some()
        );
        assert!(source.load("fonts/missing.ttf").expect("missing").is_none());
        let listed = source.list("fonts/").expect("list fonts");
        assert!(listed.iter().any(|p| p.as_ref() == GPUI_SVG_SANS_PATH));
        assert!(listed.iter().any(|p| p.as_ref() == GPUI_SVG_MONO_PATH));
    }
}
