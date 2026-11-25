//! Wayland platform integrations for AIOS (Hyprland, Niri, clipboard, XDG portal).

use aios_core::Result;
use serde::{Deserialize, Serialize};

/// Supported Wayland compositors with first-class IPC adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompositorKind {
    Hyprland,
    Niri,
    GenericWayland,
}

/// Resolved desktop color scheme from `org.freedesktop.portal.Settings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SystemColorScheme {
    NoPreference,
    PreferDark,
    PreferLight,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: String,
    pub title: String,
    pub app_id: String,
    pub workspace_id: String,
    pub is_focused: bool,
    pub is_floating: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub id: String,
    pub name: String,
    pub monitor: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub is_focused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WindowAction {
    FocusWindow(String),
    CloseWindow(String),
    ToggleFloating,
    ToggleFullscreen,
    Maximize,
    Center,
    SwitchWorkspace(String),
    MoveToWorkspace(String),
}

/// Compositor-specific IPC contract implemented for Hyprland and Niri in Phase 3.
pub trait CompositorAdapter: Send + Sync {
    fn kind(&self) -> CompositorKind;
    fn list_windows(&self) -> Result<Vec<WindowInfo>>;
    fn active_window(&self) -> Result<Option<WindowInfo>>;
    fn list_workspaces(&self) -> Result<Vec<WorkspaceInfo>>;
    fn list_monitors(&self) -> Result<Vec<MonitorInfo>>;
    fn execute(&self, action: WindowAction) -> Result<()>;
}

/// Detects the active Wayland compositor from standard environment variables.
pub fn detect_compositor() -> CompositorKind {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        CompositorKind::Hyprland
    } else if std::env::var_os("NIRI_SOCKET").is_some() {
        CompositorKind::Niri
    } else {
        CompositorKind::GenericWayland
    }
}
