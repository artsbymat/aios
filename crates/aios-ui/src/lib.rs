//! GPUI theme tokens and multi-view UI renderers for AIOS.
//!
//! Token values strictly mirror `DESIGN.md` (Basalt Dark, Warm Alabaster Light, Signal Copper accent).

use aios_core::config::ResolvedTheme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemePalette {
    pub bg_canvas: &'static str,
    pub bg_elevated: &'static str,
    pub bg_subtle: &'static str,
    pub bg_selected: &'static str,
    pub bg_hover: &'static str,
    pub bg_keycap: &'static str,
    pub text_primary: &'static str,
    pub text_secondary: &'static str,
    pub text_muted: &'static str,
    pub text_on_accent: &'static str,
    pub border_subtle: &'static str,
    pub border_default: &'static str,
    pub border_strong: &'static str,
    pub border_focus: &'static str,
    pub accent_solid: &'static str,
    pub accent_fg: &'static str,
    pub accent_fg_on_selected: &'static str,
    pub status_success: &'static str,
    pub status_warning: &'static str,
    pub status_danger: &'static str,
}

pub const DARK_PALETTE: ThemePalette = ThemePalette {
    bg_canvas: "#131517",
    bg_elevated: "#1B1E22",
    bg_subtle: "#171A1D",
    bg_selected: "#25292E",
    bg_hover: "#1F2328",
    bg_keycap: "#22262B",
    text_primary: "#F1EFEA",
    text_secondary: "#A3A8AE",
    text_muted: "#858B93",
    text_on_accent: "#131517",
    border_subtle: "#2A2F36",
    border_default: "#363C44",
    border_strong: "#656D76",
    border_focus: "#EE7B4D",
    accent_solid: "#E87143",
    accent_fg: "#EE7B4D",
    accent_fg_on_selected: "#F28B61",
    status_success: "#4CC38A",
    status_warning: "#E5B546",
    status_danger: "#F26D6D",
};

pub const LIGHT_PALETTE: ThemePalette = ThemePalette {
    bg_canvas: "#F6F4EF",
    bg_elevated: "#FFFFFF",
    bg_subtle: "#EFECE5",
    bg_selected: "#E8E4DC",
    bg_hover: "#ECE8E1",
    bg_keycap: "#E9E5DD",
    text_primary: "#181A1C",
    text_secondary: "#4F555C",
    text_muted: "#5E656D",
    text_on_accent: "#FFFFFF",
    border_subtle: "#DFDAD0",
    border_default: "#C8C2B6",
    border_strong: "#7B828A",
    border_focus: "#B54116",
    accent_solid: "#B54116",
    accent_fg: "#B54116",
    accent_fg_on_selected: "#A43A12",
    status_success: "#176B45",
    status_warning: "#8A5600",
    status_danger: "#B91C1C",
};

pub fn palette_for(theme: ResolvedTheme) -> ThemePalette {
    match theme {
        ResolvedTheme::Dark => DARK_PALETTE,
        ResolvedTheme::Light => LIGHT_PALETTE,
    }
}
