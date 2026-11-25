//! Reference Rust source for `com.aios.palette-grid` (compiles to `wasm32-wasip2` Component).
//!
//! Demonstrates both `GridView` (`ColorSwatch` tiles) and `FormView` (creating custom swatches)
//! using `aios-plugin-sdk` builders.

use aios_plugin_sdk::{
    ActionItem, ActionPanel, FormField, FormViewBuilder, GridAspectRatio, GridItemBuilder,
    GridItemContent, GridViewBuilder, HudMessage, PluginEvent, PluginResponse,
};

pub fn render_grid(columns: u8) -> PluginResponse {
    let view = GridViewBuilder::new("Palette Swatches (WASM)", columns)
        .placeholder("Filter mineral swatches...")
        .aspect_ratio(GridAspectRatio::Square)
        .section(
            Some("AIOS Signal & Mineral Palette"),
            Some("4 swatches"),
            vec![
                GridItemBuilder::new(
                    "sw-copper",
                    "Signal Copper",
                    GridItemContent::ColorSwatch {
                        hex: "#E87143".to_string(),
                    },
                )
                .subtitle("#E87143")
                .actions(ActionPanel::from_actions(vec![
                    ActionItem::new("swatch.copy", "Copy #E87143").shortcut("Enter"),
                ]))
                .build(),
                GridItemBuilder::new(
                    "sw-kiln",
                    "Kiln Copper",
                    GridItemContent::ColorSwatch {
                        hex: "#B54116".to_string(),
                    },
                )
                .subtitle("#B54116")
                .actions(ActionPanel::from_actions(vec![
                    ActionItem::new("swatch.copy", "Copy #B54116").shortcut("Enter"),
                ]))
                .build(),
            ],
        )
        .build();

    PluginResponse::render(view)
}

pub fn render_create_form() -> PluginResponse {
    let view = FormViewBuilder::new("Create Custom Swatch (WASM)")
        .field(FormField::TextField {
            id: "name".to_string(),
            label: "Swatch Name".to_string(),
            placeholder: Some("e.g. Slate Stone".to_string()),
            value: String::new(),
            required: true,
            error: None,
        })
        .field(FormField::TextField {
            id: "hex".to_string(),
            label: "Hex Color".to_string(),
            placeholder: Some("#363C44".to_string()),
            value: String::new(),
            required: true,
            error: None,
        })
        .actions(ActionPanel::from_actions(vec![
            ActionItem::new("swatch.save", "Save Swatch").shortcut("Enter"),
        ]))
        .build();

    PluginResponse::render(view)
}

pub fn handle_event(event: PluginEvent) -> PluginResponse {
    match event {
        PluginEvent::FormSubmitted { values, .. } => {
            let name = values
                .get("name")
                .cloned()
                .unwrap_or_else(|| "Swatch".to_string());
            PluginResponse::ShowHud {
                hud: HudMessage {
                    title: format!("Saved swatch {name}"),
                },
            }
        }
        _ => render_grid(4),
    }
}
