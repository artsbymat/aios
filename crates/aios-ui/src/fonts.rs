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
const CALCULATOR_PLUGIN_ICON_SVG: &[u8] =
    include_bytes!("../../../plugins/com.aios.calculator/icon.svg");
const CALC_ARITHMETIC_ICON_SVG: &[u8] =
    include_bytes!("../../../plugins/com.aios.calculator/icons/calc-arithmetic.svg");
const CALC_BASE_ICON_SVG: &[u8] =
    include_bytes!("../../../plugins/com.aios.calculator/icons/calc-base.svg");
const CALC_UNIT_ICON_SVG: &[u8] =
    include_bytes!("../../../plugins/com.aios.calculator/icons/calc-unit.svg");
const CALC_CURRENCY_ICON_SVG: &[u8] =
    include_bytes!("../../../plugins/com.aios.calculator/icons/calc-currency.svg");
const CALC_DATE_ICON_SVG: &[u8] =
    include_bytes!("../../../plugins/com.aios.calculator/icons/calc-date.svg");
const CALC_TIMEZONE_ICON_SVG: &[u8] =
    include_bytes!("../../../plugins/com.aios.calculator/icons/calc-timezone.svg");

// GPUI's `SvgRenderer` queries these bundled fallback paths from the app's `AssetRegistry`
// when rasterizing SVG desktop icons. Serving AIOS's embedded fonts resolves them cleanly
// without missing-font warnings.
const GPUI_SVG_SANS_PATH: &str = "fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf";
const GPUI_SVG_MONO_PATH: &str = "fonts/lilex/Lilex-Regular.ttf";
const CALCULATOR_ICON_ASSET_PATH: &str = "icons/plugins/calculator.svg";
const CALC_ARITHMETIC_ASSET_PATH: &str = "icons/plugins/calc-arithmetic.svg";
const CALC_BASE_ASSET_PATH: &str = "icons/plugins/calc-base.svg";
const CALC_UNIT_ASSET_PATH: &str = "icons/plugins/calc-unit.svg";
const CALC_CURRENCY_ASSET_PATH: &str = "icons/plugins/calc-currency.svg";
const CALC_DATE_ASSET_PATH: &str = "icons/plugins/calc-date.svg";
const CALC_TIMEZONE_ASSET_PATH: &str = "icons/plugins/calc-timezone.svg";

/// In-memory GPUI [`AssetSource`] serving AIOS's embedded fonts and first-party plugin icons.
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
            CALCULATOR_ICON_ASSET_PATH => Ok(Some(Cow::Borrowed(CALCULATOR_PLUGIN_ICON_SVG))),
            CALC_ARITHMETIC_ASSET_PATH => Ok(Some(Cow::Borrowed(CALC_ARITHMETIC_ICON_SVG))),
            CALC_BASE_ASSET_PATH => Ok(Some(Cow::Borrowed(CALC_BASE_ICON_SVG))),
            CALC_UNIT_ASSET_PATH => Ok(Some(Cow::Borrowed(CALC_UNIT_ICON_SVG))),
            CALC_CURRENCY_ASSET_PATH => Ok(Some(Cow::Borrowed(CALC_CURRENCY_ICON_SVG))),
            CALC_DATE_ASSET_PATH => Ok(Some(Cow::Borrowed(CALC_DATE_ICON_SVG))),
            CALC_TIMEZONE_ASSET_PATH => Ok(Some(Cow::Borrowed(CALC_TIMEZONE_ICON_SVG))),
            _ => Ok(None),
        }
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        let all = [
            GPUI_SVG_SANS_PATH,
            GPUI_SVG_MONO_PATH,
            "fonts/InterVariable.ttf",
            "fonts/JetBrainsMonoVariable.ttf",
            CALCULATOR_ICON_ASSET_PATH,
            CALC_ARITHMETIC_ASSET_PATH,
            CALC_BASE_ASSET_PATH,
            CALC_UNIT_ASSET_PATH,
            CALC_CURRENCY_ASSET_PATH,
            CALC_DATE_ASSET_PATH,
            CALC_TIMEZONE_ASSET_PATH,
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
        for icon_path in [
            CALCULATOR_ICON_ASSET_PATH,
            CALC_ARITHMETIC_ASSET_PATH,
            CALC_BASE_ASSET_PATH,
            CALC_UNIT_ASSET_PATH,
            CALC_CURRENCY_ASSET_PATH,
            CALC_DATE_ASSET_PATH,
            CALC_TIMEZONE_ASSET_PATH,
        ] {
            let svg = source
                .load(icon_path)
                .expect("load plugin icon")
                .expect("plugin icon exists");
            assert!(
                svg.starts_with(b"<svg"),
                "invalid SVG header for {icon_path}"
            );
        }
    }
}
