//! GPUI dual-mode theme engine derived strictly from `DESIGN.md`.
//!
//! Dark mode: Mineral Basalt (`#131517`) + Signal Copper (`#E87143` / `#EE7B4D`).
//! Light mode: Warm Alabaster (`#F6F4EF`) + Kiln Copper (`#B54116` / `#A43A12`).

use aios_core::config::{ResolvedTheme, ThemeMode};
use aios_core::view::SemanticTone;
use gpui::{Font, FontFallbacks, Global, Hsla, Pixels, Rgba, px, rgb, rgba};

/// Static hex token palette mirroring `DESIGN.md` (used for diagnostics and WCAG audits).
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

#[must_use]
pub fn palette_for(theme: ResolvedTheme) -> ThemePalette {
    match theme {
        ResolvedTheme::Dark => DARK_PALETTE,
        ResolvedTheme::Light => LIGHT_PALETTE,
    }
}

/// Resolved GPUI color tokens for active rendering (`DESIGN.md` colors.*).
#[derive(Debug, Clone, Copy)]
pub struct ThemeColors {
    pub bg_canvas: Rgba,
    pub bg_canvas_root: Rgba,
    pub bg_elevated: Rgba,
    pub bg_subtle: Rgba,
    pub bg_selected: Rgba,
    pub bg_hover: Rgba,
    pub bg_keycap: Rgba,
    pub bg_scrim: Rgba,
    pub text_primary: Rgba,
    pub text_secondary: Rgba,
    pub text_muted: Rgba,
    pub text_on_accent: Rgba,
    pub border_subtle: Rgba,
    pub border_default: Rgba,
    pub border_strong: Rgba,
    pub border_focus: Rgba,
    pub accent_solid: Rgba,
    pub accent_fg: Rgba,
    pub accent_fg_on_selected: Rgba,
    pub accent_subtle_bg: Rgba,
    pub status_success: Rgba,
    pub status_success_bg: Rgba,
    pub status_warning: Rgba,
    pub status_warning_bg: Rgba,
    pub status_danger: Rgba,
    pub status_danger_bg: Rgba,
    pub shadow_window: Rgba,
    pub shadow_popover: Rgba,
}

impl ThemeColors {
    #[must_use]
    pub fn from_resolved(resolved: ResolvedTheme, translucent_root: bool) -> Self {
        match resolved {
            ResolvedTheme::Dark => Self {
                bg_canvas: rgb(0x131517),
                bg_canvas_root: if translucent_root {
                    rgba(0x131517F0) // ~0.94 alpha matching DESIGN.md >= 0.92 floor
                } else {
                    rgb(0x131517)
                },
                bg_elevated: rgb(0x1B1E22),
                bg_subtle: rgb(0x171A1D),
                bg_selected: rgb(0x25292E),
                bg_hover: rgb(0x1F2328),
                bg_keycap: rgb(0x22262B),
                bg_scrim: rgba(0x0A0C0EA3), // rgba(10, 12, 14, 0.64)
                text_primary: rgb(0xF1EFEA),
                text_secondary: rgb(0xA3A8AE),
                text_muted: rgb(0x858B93),
                text_on_accent: rgb(0x131517),
                border_subtle: rgb(0x2A2F36),
                border_default: rgb(0x363C44),
                border_strong: rgb(0x656D76),
                border_focus: rgb(0xEE7B4D),
                accent_solid: rgb(0xE87143),
                accent_fg: rgb(0xEE7B4D),
                accent_fg_on_selected: rgb(0xF28B61),
                accent_subtle_bg: rgba(0xE8714324), // 0.14 alpha
                status_success: rgb(0x4CC38A),
                status_success_bg: rgba(0x4CC38A24),
                status_warning: rgb(0xE5B546),
                status_warning_bg: rgba(0xE5B54624),
                status_danger: rgb(0xF26D6D),
                status_danger_bg: rgba(0xF26D6D24),
                shadow_window: rgba(0x00000070),  // 0.44 alpha
                shadow_popover: rgba(0x00000057), // 0.34 alpha
            },
            ResolvedTheme::Light => Self {
                bg_canvas: rgb(0xF6F4EF),
                bg_canvas_root: if translucent_root {
                    rgba(0xF6F4EFF2) // ~0.95 alpha matching DESIGN.md
                } else {
                    rgb(0xF6F4EF)
                },
                bg_elevated: rgb(0xFFFFFF),
                bg_subtle: rgb(0xEFECE5),
                bg_selected: rgb(0xE8E4DC),
                bg_hover: rgb(0xECE8E1),
                bg_keycap: rgb(0xE9E5DD),
                bg_scrim: rgba(0x181A1C5C), // rgba(24, 26, 28, 0.36)
                text_primary: rgb(0x181A1C),
                text_secondary: rgb(0x4F555C),
                text_muted: rgb(0x5E656D),
                text_on_accent: rgb(0xFFFFFF),
                border_subtle: rgb(0xDFDAD0),
                border_default: rgb(0xC8C2B6),
                border_strong: rgb(0x7B828A),
                border_focus: rgb(0xB54116),
                accent_solid: rgb(0xB54116),
                accent_fg: rgb(0xB54116),
                accent_fg_on_selected: rgb(0xA43A12),
                accent_subtle_bg: rgba(0xB541161C), // 0.11 alpha
                status_success: rgb(0x176B45),
                status_success_bg: rgba(0x176B451C),
                status_warning: rgb(0x8A5600),
                status_warning_bg: rgba(0x8A56001F),
                status_danger: rgb(0xB91C1C),
                status_danger_bg: rgba(0xB91C1C1C),
                shadow_window: rgba(0x181A1C33),
                shadow_popover: rgba(0x181A1C29),
            },
        }
    }

    #[must_use]
    pub fn tone_colors(&self, tone: SemanticTone) -> (Rgba, Rgba) {
        match tone {
            SemanticTone::Neutral => (self.text_secondary, self.bg_subtle),
            SemanticTone::Accent => (self.accent_fg, self.accent_subtle_bg),
            SemanticTone::Success => (self.status_success, self.status_success_bg),
            SemanticTone::Warning => (self.status_warning, self.status_warning_bg),
            SemanticTone::Danger => (self.status_danger, self.status_danger_bg),
        }
    }

    #[must_use]
    pub fn accent_fg_hsla(&self, selected: bool) -> Hsla {
        if selected {
            gpui::rgb_to_hsla(self.accent_fg_on_selected)
        } else {
            gpui::rgb_to_hsla(self.accent_fg)
        }
    }
}

/// Spacing & layout metrics strictly matching `DESIGN.md`.
#[derive(Debug, Clone, Copy)]
pub struct ThemeMetrics {
    pub radius_xs: Pixels,
    pub radius_sm: Pixels,
    pub radius_md: Pixels,
    pub radius_lg: Pixels,
    pub radius_window: Pixels,
    pub header_height: Pixels,
    pub footer_height: Pixels,
    pub list_row_height: Pixels,
    pub metadata_sidebar_width: Pixels,
    pub action_panel_width: Pixels,
    pub action_panel_max_height: Pixels,
}

impl Default for ThemeMetrics {
    fn default() -> Self {
        Self {
            radius_xs: px(4.0),
            radius_sm: px(6.0),
            radius_md: px(8.0),
            radius_lg: px(10.0),
            radius_window: px(12.0),
            header_height: px(52.0),
            footer_height: px(40.0),
            list_row_height: px(40.0),
            metadata_sidebar_width: px(240.0),
            action_panel_width: px(320.0),
            action_panel_max_height: px(360.0),
        }
    }
}

/// Global GPUI theme state updated live when user config or XDG portal color-scheme changes.
#[derive(Debug, Clone)]
pub struct AiosTheme {
    pub mode: ThemeMode,
    pub system_prefers_dark: bool,
    pub resolved: ResolvedTheme,
    pub translucent_root: bool,
    pub reduced_motion: bool,
    pub colors: ThemeColors,
    pub metrics: ThemeMetrics,
    pub font_sans: &'static str,
    pub font_mono: &'static str,
}

impl Global for AiosTheme {}

impl AiosTheme {
    #[must_use]
    pub fn new(
        mode: ThemeMode,
        system_prefers_dark: bool,
        translucent_root: bool,
        reduced_motion: bool,
    ) -> Self {
        let resolved = mode.resolve(system_prefers_dark);
        Self {
            mode,
            system_prefers_dark,
            resolved,
            translucent_root,
            reduced_motion,
            colors: ThemeColors::from_resolved(resolved, translucent_root),
            metrics: ThemeMetrics::default(),
            font_sans: crate::fonts::SANS_FAMILY,
            font_mono: crate::fonts::MONO_FAMILY,
        }
    }

    pub fn set_mode(&mut self, mode: ThemeMode) {
        self.mode = mode;
        self.recompute();
    }

    pub fn set_system_prefers_dark(&mut self, prefers_dark: bool) {
        self.system_prefers_dark = prefers_dark;
        self.recompute();
    }

    pub fn cycle_mode(&mut self) -> ThemeMode {
        self.mode = match self.mode {
            ThemeMode::System => ThemeMode::Dark,
            ThemeMode::Dark => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::System,
        };
        self.recompute();
        self.mode
    }

    #[must_use]
    pub fn mode_label(&self) -> &'static str {
        match (self.mode, self.resolved) {
            (ThemeMode::Dark, _) => "Dark",
            (ThemeMode::Light, _) => "Light",
            (ThemeMode::System, ResolvedTheme::Dark) => "System (Dark)",
            (ThemeMode::System, ResolvedTheme::Light) => "System (Light)",
        }
    }

    #[must_use]
    pub fn sans_font(&self) -> Font {
        Font {
            family: self.font_sans.into(),
            fallbacks: Some(FontFallbacks::from_fonts(vec![
                "Inter".to_string(),
                "Adwaita Sans".to_string(),
                "Noto Sans".to_string(),
                "Cantarell".to_string(),
                "DejaVu Sans".to_string(),
                "sans-serif".to_string(),
            ])),
            ..Default::default()
        }
    }

    #[must_use]
    pub fn mono_font(&self) -> Font {
        Font {
            family: self.font_mono.into(),
            fallbacks: Some(FontFallbacks::from_fonts(vec![
                "JetBrainsMono Nerd Font Mono".to_string(),
                "Adwaita Mono".to_string(),
                "Noto Sans Mono".to_string(),
                "DejaVu Sans Mono".to_string(),
                "monospace".to_string(),
            ])),
            ..Default::default()
        }
    }

    fn recompute(&mut self) {
        self.resolved = self.mode.resolve(self.system_prefers_dark);
        self.colors = ThemeColors::from_resolved(self.resolved, self.translucent_root);
    }
}

/// Parses a `#RRGGBB` hex string into GPUI [`Rgba`], returning a neutral fallback on invalid input.
#[must_use]
pub fn parse_hex_color(hex: &str) -> Rgba {
    let stripped = hex.trim().trim_start_matches('#');
    if stripped.len() == 6
        && let Ok(val) = u32::from_str_radix(stripped, 16)
    {
        return rgb(val);
    }
    rgb(0x858B93)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex_to_rgb_f64(hex: &str) -> (f64, f64, f64) {
        let s = hex.trim_start_matches('#');
        let v = u32::from_str_radix(s, 16).expect("valid 6-digit hex");
        (
            f64::from((v >> 16) & 0xFF) / 255.0,
            f64::from((v >> 8) & 0xFF) / 255.0,
            f64::from(v & 0xFF) / 255.0,
        )
    }

    fn linearize(c: f64) -> f64 {
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    fn contrast_ratio(fg: &str, bg: &str) -> f64 {
        let (r1, g1, b1) = hex_to_rgb_f64(fg);
        let (r2, g2, b2) = hex_to_rgb_f64(bg);
        let l1 = 0.2126 * linearize(r1) + 0.7152 * linearize(g1) + 0.0722 * linearize(b1);
        let l2 = 0.2126 * linearize(r2) + 0.7152 * linearize(g2) + 0.0722 * linearize(b2);
        let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn dark_and_light_palettes_meet_wcag_aa() {
        for (name, p) in [("dark", DARK_PALETTE), ("light", LIGHT_PALETTE)] {
            assert!(
                contrast_ratio(p.text_primary, p.bg_canvas) >= 4.5,
                "{name} text.primary on bg.canvas"
            );
            assert!(
                contrast_ratio(p.text_secondary, p.bg_canvas) >= 4.5,
                "{name} text.secondary on bg.canvas"
            );
            assert!(
                contrast_ratio(p.text_muted, p.bg_canvas) >= 4.5,
                "{name} text.muted on bg.canvas"
            );
            assert!(
                contrast_ratio(p.text_primary, p.bg_selected) >= 4.5,
                "{name} text.primary on bg.selected"
            );
            assert!(
                contrast_ratio(p.accent_fg, p.bg_canvas) >= 4.5,
                "{name} accent.fg on bg.canvas"
            );
            assert!(
                contrast_ratio(p.accent_fg_on_selected, p.bg_selected) >= 4.5,
                "{name} accent.fgOnSelected on bg.selected"
            );
            assert!(
                contrast_ratio(p.text_on_accent, p.accent_solid) >= 4.5,
                "{name} text.onAccent on accent.solid"
            );
            assert!(
                contrast_ratio(p.border_strong, p.bg_canvas) >= 3.0,
                "{name} border.strong on bg.canvas"
            );
            assert!(
                contrast_ratio(p.border_focus, p.bg_canvas) >= 3.0,
                "{name} border.focus on bg.canvas"
            );
            assert!(
                contrast_ratio(p.status_success, p.bg_canvas) >= 4.5,
                "{name} status.success on bg.canvas"
            );
            assert!(
                contrast_ratio(p.status_warning, p.bg_canvas) >= 4.5,
                "{name} status.warning on bg.canvas"
            );
            assert!(
                contrast_ratio(p.status_danger, p.bg_canvas) >= 4.5,
                "{name} status.danger on bg.canvas"
            );
        }
    }

    #[test]
    fn theme_mode_cycles_and_follows_system_portal() {
        let mut theme = AiosTheme::new(ThemeMode::System, true, true, false);
        assert_eq!(theme.resolved, ResolvedTheme::Dark);

        theme.set_system_prefers_dark(false);
        assert_eq!(theme.resolved, ResolvedTheme::Light);

        assert_eq!(theme.cycle_mode(), ThemeMode::Dark);
        assert_eq!(theme.resolved, ResolvedTheme::Dark);

        assert_eq!(theme.cycle_mode(), ThemeMode::Light);
        assert_eq!(theme.resolved, ResolvedTheme::Light);

        assert_eq!(theme.cycle_mode(), ThemeMode::System);
        assert_eq!(theme.resolved, ResolvedTheme::Light);
    }
}
