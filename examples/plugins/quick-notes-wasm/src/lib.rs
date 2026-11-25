//! Reference Rust source for `com.aios.quick-notes` (compiles to `wasm32-wasip2` Component).
//!
//! Demonstrates `List + Split Detail` declarative view construction using `aios-plugin-sdk`
//! alongside capability-gated clipboard copy and scoped `LocalStorage`.

use aios_plugin_sdk::{
    ActionItem, ActionPanelBuilder, ListItemBuilder, ListViewBuilder, MetadataEntry, PluginEvent,
    PluginResponse, SemanticTone,
};

pub fn render_notes_list(default_folder: &str) -> PluginResponse {
    let view = ListViewBuilder::new("Quick Notes (WASM)")
        .placeholder("Filter notes...")
        .showing_detail(true)
        .section(
            Some("Saved Notes"),
            Some("2 notes"),
            vec![
                ListItemBuilder::new("note-1", "Wayland Layer-Shell Checklist")
                    .subtitle(default_folder)
                    .icon("NT")
                    .accessory_tag("Pinned", SemanticTone::Accent)
                    .detail(
                        "# Wayland Layer-Shell Checklist\n\n- Hyprland `layerrule = blur, aios`\n- Niri `layer-rule { match namespace=\"^aios$\" }`",
                        vec![
                            MetadataEntry::Label {
                                title: "Folder".to_string(),
                                text: default_folder.to_string(),
                            },
                            MetadataEntry::Label {
                                title: "Runtime".to_string(),
                                text: "WASM Component (aios:plugin@0.1.0)".to_string(),
                            },
                        ],
                    )
                    .actions(
                        ActionPanelBuilder::new()
                            .action(ActionItem::new("note.copy", "Copy Note to Clipboard").shortcut("Enter"))
                            .build(),
                    )
                    .build(),
            ],
        )
        .build();

    PluginResponse::render(view)
}

pub fn handle_event(event: PluginEvent, default_folder: &str) -> PluginResponse {
    match event {
        PluginEvent::ActionTriggered { action_id, .. } if action_id == "note.copy" => {
            PluginResponse::CopyToClipboard {
                text: "Wayland Layer-Shell Checklist".to_string(),
                hud_title: Some("Copied Quick Note".to_string()),
            }
        }
        _ => render_notes_list(default_folder),
    }
}
