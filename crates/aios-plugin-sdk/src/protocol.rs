//! Shared JSON protocol envelopes exchanged between the AIOS host and external WASM / native plugins.

use std::collections::BTreeMap;

use aios_core::view::{ConfirmAlert, HudMessage, ToastMessage, ViewNode};
use serde::{Deserialize, Serialize};

/// Event delivered from the AIOS host into a plugin's `lifecycle::handle-event`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum PluginEvent {
    /// Emitted when the user types in a view with `FilterMode::Plugin`.
    SearchTextChanged { command_id: String, query: String },
    /// Emitted when the user activates an `ActionItem` via Enter, Ctrl+Enter, click, or ActionPanel.
    ActionTriggered {
        command_id: String,
        action_id: String,
        item_id: Option<String>,
        #[serde(default)]
        inline_args: Vec<String>,
    },
    /// Emitted when the user selects a new option in a view's `FilterDropdown`.
    DropdownChanged {
        command_id: String,
        selected_id: String,
    },
    /// Emitted when a `FormView` passes validation and is submitted.
    FormSubmitted {
        command_id: String,
        action_id: String,
        values: BTreeMap<String, String>,
    },
    /// Emitted when the user confirms a destructive `ConfirmAlert` modal.
    ConfirmAccepted {
        command_id: String,
        alert_id: String,
    },
    /// Emitted by the background scheduler for `MenuBar` or cache-warming commands.
    BackgroundRefresh { command_id: String },
}

/// Declarative response returned by `lifecycle::init-command`, `handle-event`, or `background-refresh`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PluginResponse {
    /// Replaces the current plugin frame with `view` (or opens the initial view on `init-command`).
    Render { view: Box<ViewNode> },
    /// Pushes a child `ViewNode` onto the navigation stack.
    Push {
        frame_id: Option<String>,
        view: Box<ViewNode>,
    },
    /// Pops the current view frame.
    Pop,
    /// Pops back to Root Search.
    PopToRoot,
    /// Closes/hides the main launcher window (optionally showing a HUD pill).
    CloseWindow { hud: Option<HudMessage> },
    /// Displays an inline footer toast without changing the current view.
    ShowToast { toast: ToastMessage },
    /// Displays a non-interactive Wayland HUD pill.
    ShowHud { hud: HudMessage },
    /// Opens a confirmation modal before executing a destructive action.
    Confirm { alert: ConfirmAlert },
    /// Requests the host to copy `text` to the Wayland clipboard and notify the user.
    CopyToClipboard {
        text: String,
        hud_title: Option<String>,
    },
    /// Requests the host to open an external URL via `xdg-open`.
    OpenUrl { url: String },
    /// Publishes an updated status item for a `MenuBar` command (Waybar / SNI bridge).
    MenuBar { state: MenuBarState },
    /// Explicit no-op response (useful for `no-view` commands that already triggered host APIs).
    Noop,
}

impl PluginResponse {
    #[must_use]
    pub fn render(view: ViewNode) -> Self {
        Self::Render {
            view: Box::new(view),
        }
    }

    #[must_use]
    pub fn push(frame_id: Option<String>, view: ViewNode) -> Self {
        Self::Push {
            frame_id,
            view: Box::new(view),
        }
    }

    /// Serializes this response to compact JSON for returning across the WIT boundary.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserializes a response returned by a plugin across the WIT boundary.
    pub fn from_json(raw: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(raw)
    }
}

/// Compact menu-bar / status-bar representation emitted by `CommandMode::MenuBar` plugins.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuBarState {
    pub title: String,
    pub tooltip: Option<String>,
    pub icon: Option<String>,
    #[serde(default)]
    pub items: Vec<MenuBarItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuBarItem {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    #[serde(default)]
    pub shortcut: Option<String>,
}
