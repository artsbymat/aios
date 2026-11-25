//! Embedded variable fonts bundled directly into the AIOS binary (SIL OFL 1.1).
//!
//! - Sans: `Inter Variable` (`assets/fonts/InterVariable.ttf`)
//! - Mono: `JetBrains Mono` (`assets/fonts/JetBrainsMonoVariable.ttf`)

use std::borrow::Cow;

use gpui::App;

/// Registered family name inside `assets/fonts/InterVariable.ttf`.
pub const SANS_FAMILY: &str = "Inter Variable";

/// Registered family name inside `assets/fonts/JetBrainsMonoVariable.ttf`.
pub const MONO_FAMILY: &str = "JetBrains Mono";

const INTER_VARIABLE_TTF: &[u8] = include_bytes!("../../../assets/fonts/InterVariable.ttf");
const JETBRAINS_MONO_VARIABLE_TTF: &[u8] =
    include_bytes!("../../../assets/fonts/JetBrainsMonoVariable.ttf");

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
}
