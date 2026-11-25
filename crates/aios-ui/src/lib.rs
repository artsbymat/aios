//! GPUI dual-mode theme engine, shell chrome, Wayland LayerShell host, and all 4 declarative
//! interactive view renderers (`List`, `Grid`, `Detail`, `Form`).

pub mod components;
pub mod demo;
pub mod fonts;
pub mod hud;
pub mod shell;
pub mod state;
pub mod theme;
pub mod views;

pub use fonts::register_embedded_fonts;
pub use shell::{LauncherWindow, run_daemon_app, run_preview_app};
pub use state::{ActiveOverlay, HeaderFocus, LauncherState, NavFrame, ShellEffect};
pub use theme::{
    AiosTheme, DARK_PALETTE, LIGHT_PALETTE, ThemeColors, ThemeMetrics, ThemePalette, palette_for,
};
