//! Phase 2 interactive view fixtures covering all 4 declarative view renderers
//! (`List` + Split `Detail`, `Grid`, `Detail` + Metadata, `Form`) plus mandatory
//! `Empty`, `Loading`, `Error`, `ActionPanel`, `Toast`, `HUD`, and `ConfirmAlert` states.

use aios_core::view::{
    Accessory, ActionItem, ActionPanel, ActionSection, ConfirmAlert, DetailView, EmptyView,
    ErrorView, FilterDropdown, FilterMode, FilterOption, FormField, FormView, GridAspectRatio,
    GridItem, GridItemContent, GridSection, GridView, HudMessage, InlineArgumentSpec, ItemDetail,
    ListItem, ListSection, ListView, MetadataEntry, SemanticTone, ToastStyle, ViewNode,
};
use aios_wayland::{CompositorKind, detect_compositor};

use crate::state::{ActiveOverlay, LauncherState, NavFrame, ShellEffect};

/// Builds the Phase 2 Root Workbench `NavFrame` showcasing every renderer and state.
#[must_use]
pub fn build_root_showcase_frame() -> NavFrame {
    let compositor = detect_compositor();

    let view = ViewNode::List(ListView {
        navigation_title: Some("AIOS Phase 2 Workbench".to_string()),
        search_placeholder: Some("Search views, states, or actions...".to_string()),
        filtering: FilterMode::Host,
        is_loading: false,
        loading_message: None,
        is_showing_detail: false,
        filter_dropdown: Some(FilterDropdown {
            tooltip: "Filter Category".to_string(),
            selected_id: "all".to_string(),
            options: vec![
                FilterOption {
                    id: "all".to_string(),
                    title: "All Fixtures".to_string(),
                },
                FilterOption {
                    id: "views".to_string(),
                    title: "View Renderers".to_string(),
                },
                FilterOption {
                    id: "states".to_string(),
                    title: "UI States & Overlays".to_string(),
                },
                FilterOption {
                    id: "theme".to_string(),
                    title: "Theme & Shell".to_string(),
                },
            ],
        }),
        inline_arguments: vec![InlineArgumentSpec {
            id: "query_arg".to_string(),
            placeholder: "arg1".to_string(),
            required: false,
        }],
        sections: vec![
            ListSection {
                title: Some("Declarative View Renderers (Phase 2)".to_string()),
                subtitle: Some("4 interactive views".to_string()),
                items: vec![
                    ListItem {
                        id: "demo.list-detail".to_string(),
                        title: "List + Split Detail View".to_string(),
                        subtitle: Some(
                            "40/60 split list with Markdown & Metadata preview".to_string(),
                        ),
                        icon: Some("LD".to_string()),
                        icon_path: None,
                        alias: Some("cb".to_string()),
                        keywords: vec!["clipboard".into(), "split".into(), "list".into()],
                        filter_group: Some("views".to_string()),
                        accessories: vec![
                            Accessory::Tag {
                                label: "List.Detail".to_string(),
                                tone: SemanticTone::Accent,
                            },
                            Accessory::Text {
                                value: "alias: cb".to_string(),
                            },
                        ],
                        detail: None,
                        actions: ActionPanel {
                            sections: vec![
                                ActionSection {
                                    title: Some("Navigation".to_string()),
                                    actions: vec![
                                        ActionItem::new(
                                            "open.list-detail",
                                            "Open List + Detail View",
                                        )
                                        .shortcut("Enter"),
                                        ActionItem::new(
                                            "theme.cycle",
                                            "Cycle Theme (System / Dark / Light)",
                                        )
                                        .shortcut("Ctrl+Enter"),
                                    ],
                                },
                                ActionSection {
                                    title: Some("Quick Switch".to_string()),
                                    actions: vec![
                                        ActionItem::new("submenu.themes", "Switch Theme Mode")
                                            .with_submenu(vec![
                                                ActionItem::new("theme.dark", "Use Dark (Basalt)"),
                                                ActionItem::new(
                                                    "theme.light",
                                                    "Use Light (Alabaster)",
                                                ),
                                                ActionItem::new(
                                                    "theme.system",
                                                    "Follow System Portal",
                                                ),
                                            ]),
                                    ],
                                },
                            ],
                        },
                    },
                    ListItem {
                        id: "demo.grid".to_string(),
                        title: "Grid View (Symbols & Palette)".to_string(),
                        subtitle: Some(
                            "4-column aspect-locked tiles with arrow-key navigation".to_string(),
                        ),
                        icon: Some("GR".to_string()),
                        icon_path: None,
                        alias: Some("gr".to_string()),
                        keywords: vec![
                            "grid".into(),
                            "symbols".into(),
                            "colors".into(),
                            "swatches".into(),
                        ],
                        filter_group: Some("views".to_string()),
                        accessories: vec![Accessory::Tag {
                            label: "4 cols".to_string(),
                            tone: SemanticTone::Neutral,
                        }],
                        detail: None,
                        actions: ActionPanel::from_actions(vec![
                            ActionItem::new("open.grid", "Open Grid View").shortcut("Enter"),
                            ActionItem::new("hud.ping", "Test HUD Pill").shortcut("Ctrl+Enter"),
                        ]),
                    },
                    ListItem {
                        id: "demo.detail".to_string(),
                        title: "Detail + Metadata View".to_string(),
                        subtitle: Some(format!("Wayland surface inspector ({compositor:?})")),
                        icon: Some("DT".to_string()),
                        icon_path: None,
                        alias: Some("dt".to_string()),
                        keywords: vec![
                            "detail".into(),
                            "markdown".into(),
                            "wayland".into(),
                            "hyprland".into(),
                            "niri".into(),
                        ],
                        filter_group: Some("views".to_string()),
                        accessories: vec![Accessory::Tag {
                            label: format!("{compositor:?}"),
                            tone: SemanticTone::Success,
                        }],
                        detail: None,
                        actions: ActionPanel::from_actions(vec![
                            ActionItem::new("open.detail", "Open Detail View").shortcut("Enter"),
                            ActionItem::new("toast.info", "Trigger Success Toast")
                                .shortcut("Ctrl+Enter"),
                        ]),
                    },
                    ListItem {
                        id: "demo.form".to_string(),
                        title: "Form View (Quicklink Editor)".to_string(),
                        subtitle: Some(
                            "All field types, Tab focus order, and inline validation".to_string(),
                        ),
                        icon: Some("FM".to_string()),
                        icon_path: None,
                        alias: Some("fm".to_string()),
                        keywords: vec![
                            "form".into(),
                            "input".into(),
                            "validation".into(),
                            "quicklink".into(),
                        ],
                        filter_group: Some("views".to_string()),
                        accessories: vec![Accessory::Tag {
                            label: "6 fields".to_string(),
                            tone: SemanticTone::Neutral,
                        }],
                        detail: None,
                        actions: ActionPanel::from_actions(vec![
                            ActionItem::new("open.form", "Open Form View").shortcut("Enter"),
                        ]),
                    },
                ],
            },
            ListSection {
                title: Some("Mandatory States & Feedback Overlays".to_string()),
                subtitle: None,
                items: vec![
                    ListItem {
                        id: "demo.empty".to_string(),
                        title: "Inspect Empty State View".to_string(),
                        subtitle: Some(
                            "Shows clear cause and immediate keyboard recovery".to_string(),
                        ),
                        icon: Some("EM".to_string()),
                        icon_path: None,
                        alias: None,
                        keywords: vec!["empty".into(), "state".into()],
                        filter_group: Some("states".to_string()),
                        accessories: vec![Accessory::Text {
                            value: "R-27".to_string(),
                        }],
                        detail: None,
                        actions: ActionPanel::from_actions(vec![
                            ActionItem::new("open.empty", "Open Empty State").shortcut("Enter"),
                        ]),
                    },
                    ListItem {
                        id: "demo.loading".to_string(),
                        title: "Inspect Loading State View".to_string(),
                        subtitle: Some(
                            "Header 2px progress track and descriptive status text".to_string(),
                        ),
                        icon: Some("LD".to_string()),
                        icon_path: None,
                        alias: None,
                        keywords: vec!["loading".into(), "progress".into(), "state".into()],
                        filter_group: Some("states".to_string()),
                        accessories: vec![Accessory::Tag {
                            label: "2px track".to_string(),
                            tone: SemanticTone::Warning,
                        }],
                        detail: None,
                        actions: ActionPanel::from_actions(vec![
                            ActionItem::new("open.loading", "Open Loading State").shortcut("Enter"),
                        ]),
                    },
                    ListItem {
                        id: "demo.error".to_string(),
                        title: "Inspect Error Isolation View".to_string(),
                        subtitle: Some(
                            "Simulated plugin timeout with recovery actions".to_string(),
                        ),
                        icon: Some("ER".to_string()),
                        icon_path: None,
                        alias: None,
                        keywords: vec!["error".into(), "crash".into(), "wasm".into()],
                        filter_group: Some("states".to_string()),
                        accessories: vec![Accessory::Tag {
                            label: "Isolated".to_string(),
                            tone: SemanticTone::Danger,
                        }],
                        detail: None,
                        actions: ActionPanel::from_actions(vec![
                            ActionItem::new("open.error", "Open Error State").shortcut("Enter"),
                        ]),
                    },
                    ListItem {
                        id: "demo.alert".to_string(),
                        title: "Trigger Confirm Alert Modal".to_string(),
                        subtitle: Some(
                            "Destructive confirmation dialog closable with Esc".to_string(),
                        ),
                        icon: Some("AL".to_string()),
                        icon_path: None,
                        alias: None,
                        keywords: vec![
                            "alert".into(),
                            "modal".into(),
                            "confirm".into(),
                            "dialog".into(),
                        ],
                        filter_group: Some("states".to_string()),
                        accessories: vec![Accessory::Tag {
                            label: "Destructive".to_string(),
                            tone: SemanticTone::Danger,
                        }],
                        detail: None,
                        actions: ActionPanel::from_actions(vec![
                            ActionItem::new("overlay.confirm", "Open Confirm Alert")
                                .shortcut("Enter")
                                .destructive(),
                        ]),
                    },
                    ListItem {
                        id: "demo.hud".to_string(),
                        title: "Show Wayland HUD Pill".to_string(),
                        subtitle: Some(
                            "Separate non-interactive LayerShell pill (`aios-hud`)".to_string(),
                        ),
                        icon: Some("HD".to_string()),
                        icon_path: None,
                        alias: None,
                        keywords: vec!["hud".into(), "overlay".into(), "pill".into()],
                        filter_group: Some("states".to_string()),
                        accessories: vec![Accessory::Text {
                            value: "aios-hud".to_string(),
                        }],
                        detail: None,
                        actions: ActionPanel::from_actions(vec![
                            ActionItem::new("hud.ping", "Show HUD Confirmation").shortcut("Enter"),
                        ]),
                    },
                ],
            },
            ListSection {
                title: Some("Theme & Shell Controls".to_string()),
                subtitle: None,
                items: vec![ListItem {
                    id: "theme.cycle.row".to_string(),
                    title: "Cycle Theme Mode (System / Dark / Light)".to_string(),
                    subtitle: Some(
                        "Switch Basalt Dark, Warm Alabaster Light, or XDG Portal".to_string(),
                    ),
                    icon: Some("TH".to_string()),
                    icon_path: None,
                    alias: Some("th".to_string()),
                    keywords: vec![
                        "theme".into(),
                        "dark".into(),
                        "light".into(),
                        "system".into(),
                    ],
                    filter_group: Some("theme".to_string()),
                    accessories: vec![Accessory::Tag {
                        label: "Dual-Mode".to_string(),
                        tone: SemanticTone::Accent,
                    }],
                    detail: None,
                    actions: ActionPanel::from_actions(vec![
                        ActionItem::new("theme.cycle", "Cycle Theme Mode").shortcut("Enter"),
                        ActionItem::new("theme.dark", "Switch to Dark (Basalt)"),
                        ActionItem::new("theme.light", "Switch to Light (Alabaster)"),
                        ActionItem::new("theme.system", "Follow System Portal"),
                    ]),
                }],
            },
        ],
        empty_view: Some(EmptyView {
            title: "No matching fixtures".to_string(),
            description: Some(
                "No Phase 2 fixture matched your query. Press Esc to clear the search filter."
                    .to_string(),
            ),
            icon: Some("0".to_string()),
            actions: ActionPanel::from_actions(vec![
                ActionItem::new("search.clear", "Clear Search Filter").shortcut("Enter"),
            ]),
        }),
    });

    NavFrame::new("root.showcase", view)
}

#[must_use]
pub fn build_split_list_detail_frame() -> NavFrame {
    let view = ViewNode::List(ListView {
        navigation_title: Some("List + Split Detail (Fixture)".to_string()),
        search_placeholder: Some("Filter sample entries...".to_string()),
        filtering: FilterMode::Host,
        is_loading: false,
        loading_message: None,
        is_showing_detail: true,
        filter_dropdown: Some(FilterDropdown {
            tooltip: "Entry Type".to_string(),
            selected_id: "all".to_string(),
            options: vec![
                FilterOption {
                    id: "all".to_string(),
                    title: "All Types".to_string(),
                },
                FilterOption {
                    id: "code".to_string(),
                    title: "Code".to_string(),
                },
                FilterOption {
                    id: "config".to_string(),
                    title: "Config".to_string(),
                },
            ],
        }),
        inline_arguments: Vec::new(),
        sections: vec![ListSection {
            title: Some("Sample Split Entries (Phase 2 Fixture)".to_string()),
            subtitle: Some("3 items".to_string()),
            items: vec![
                ListItem {
                    id: "split.hypr".to_string(),
                    title: "hyprland.conf layerrule".to_string(),
                    subtitle: Some("Compositor rule snippet".to_string()),
                    icon: Some("HY".to_string()),
                    icon_path: None,
                    alias: None,
                    keywords: vec!["hyprland".into(), "blur".into(), "wayland".into()],
                    filter_group: Some("config".to_string()),
                    accessories: vec![Accessory::Text {
                        value: "4 lines".to_string(),
                    }],
                    detail: Some(ItemDetail {
                        markdown: "## Hyprland Layer Rules\n\nConfigure AIOS overlay surface rules in `~/.config/hypr/hyprland.conf`:\n\n```ini\nbind = SUPER, Space, exec, aios toggle\nlayerrule = blur, aios\nlayerrule = ignorezero, aios\n```"
                            .to_string(),
                        metadata: vec![
                            MetadataEntry::Label {
                                title: "Namespace".to_string(),
                                text: "aios".to_string(),
                            },
                            MetadataEntry::Label {
                                title: "Layer".to_string(),
                                text: "Overlay (Exclusive KB)".to_string(),
                            },
                            MetadataEntry::TagList {
                                title: "Tags".to_string(),
                                tags: vec![
                                    ("Hyprland".to_string(), SemanticTone::Accent),
                                    ("Wayland".to_string(), SemanticTone::Neutral),
                                ],
                            },
                        ],
                    }),
                    actions: ActionPanel::from_actions(vec![
                        ActionItem::new("hud.copied", "Copy Snippet (HUD)").shortcut("Enter"),
                        ActionItem::new("toast.info", "Pin Entry").shortcut("Ctrl+Enter"),
                        ActionItem::new("overlay.confirm", "Delete Entry").destructive(),
                    ]),
                },
                ListItem {
                    id: "split.niri".to_string(),
                    title: "config.kdl layer-rule".to_string(),
                    subtitle: Some("Niri compositor snippet".to_string()),
                    icon: Some("NR".to_string()),
                    icon_path: None,
                    alias: None,
                    keywords: vec!["niri".into(), "kdl".into(), "layer-rule".into()],
                    filter_group: Some("config".to_string()),
                    accessories: vec![Accessory::Text {
                        value: "5 lines".to_string(),
                    }],
                    detail: Some(ItemDetail {
                        markdown: "## Niri Layer Rule\n\nAdd the toggle binding and namespace rule in `~/.config/niri/config.kdl`:\n\n```kdl\nbinds {\n    Mod+Space { spawn \"aios\" \"toggle\"; }\n}\nlayer-rule {\n    match namespace=\"^aios$\"\n    shadow { on }\n}\n```"
                            .to_string(),
                        metadata: vec![
                            MetadataEntry::Label {
                                title: "IPC Socket".to_string(),
                                text: "$NIRI_SOCKET".to_string(),
                            },
                            MetadataEntry::TagList {
                                title: "Tags".to_string(),
                                tags: vec![
                                    ("Niri".to_string(), SemanticTone::Success),
                                    ("KDL".to_string(), SemanticTone::Neutral),
                                ],
                            },
                        ],
                    }),
                    actions: ActionPanel::from_actions(vec![
                        ActionItem::new("hud.copied", "Copy Snippet (HUD)").shortcut("Enter"),
                        ActionItem::new("overlay.confirm", "Delete Entry").destructive(),
                    ]),
                },
                ListItem {
                    id: "split.fuzzy".to_string(),
                    title: "aios-fuzzy SearchSession".to_string(),
                    subtitle: Some("Adaptive inline + Rayon pipeline".to_string()),
                    icon: Some("RS".to_string()),
                    icon_path: None,
                    alias: None,
                    keywords: vec!["rust".into(), "fuzzy".into(), "parallel".into()],
                    filter_group: Some("code".to_string()),
                    accessories: vec![Accessory::Tag {
                        label: "Rust".to_string(),
                        tone: SemanticTone::Accent,
                    }],
                    detail: Some(ItemDetail {
                        markdown: "## Adaptive Parallel Search\n\nSmall lists finish inline within `1.5ms`. Larger lists hand off to background Rayon chunks:\n\n```rust\nlet handle = session.search(corpus, query, frecency, now);\n// Render `handle.initial` immediately, stream `handle.updates`\n```"
                            .to_string(),
                        metadata: vec![
                            MetadataEntry::Label {
                                title: "Crate".to_string(),
                                text: "aios-fuzzy".to_string(),
                            },
                            MetadataEntry::Label {
                                title: "Inline Budget".to_string(),
                                text: "1.5 ms".to_string(),
                            },
                        ],
                    }),
                    actions: ActionPanel::from_actions(vec![
                        ActionItem::new("hud.copied", "Copy Code (HUD)").shortcut("Enter"),
                    ]),
                },
            ],
        }],
        empty_view: Some(EmptyView {
            title: "No split entries match".to_string(),
            description: Some("Press Esc to clear the query or change the type filter.".to_string()),
            icon: Some("0".to_string()),
            actions: ActionPanel::from_actions(vec![
                ActionItem::new("search.clear", "Clear Filter").shortcut("Enter"),
            ]),
        }),
    });

    NavFrame::new("demo.list-detail", view)
}

#[must_use]
pub fn build_grid_frame() -> NavFrame {
    let view = ViewNode::Grid(GridView {
        navigation_title: Some("Grid View: Tokens & Glyphs (Fixture)".to_string()),
        search_placeholder: Some("Filter palette tokens or symbols...".to_string()),
        filtering: FilterMode::Host,
        is_loading: false,
        loading_message: None,
        columns: 4,
        aspect_ratio: GridAspectRatio::Square,
        filter_dropdown: Some(FilterDropdown {
            tooltip: "Grid Filter".to_string(),
            selected_id: "all".to_string(),
            options: vec![
                FilterOption {
                    id: "all".to_string(),
                    title: "All Tiles".to_string(),
                },
                FilterOption {
                    id: "colors".to_string(),
                    title: "DESIGN.md Swatches".to_string(),
                },
                FilterOption {
                    id: "symbols".to_string(),
                    title: "Typographic Symbols".to_string(),
                },
            ],
        }),
        sections: vec![
            GridSection {
                title: Some("AIOS Palette Tokens (DESIGN.md)".to_string()),
                subtitle: Some("Verified WCAG AA".to_string()),
                items: vec![
                    GridItem {
                        id: "swatch.basalt".to_string(),
                        title: "Basalt Canvas".to_string(),
                        subtitle: Some("#131517".to_string()),
                        content: GridItemContent::ColorSwatch {
                            hex: "#131517".to_string(),
                        },
                        keywords: vec!["dark".into(), "canvas".into(), "basalt".into()],
                        filter_group: Some("colors".to_string()),
                        actions: ActionPanel::from_actions(vec![
                            ActionItem::new("hud.copied", "Copy #131517").shortcut("Enter"),
                        ]),
                    },
                    GridItem {
                        id: "swatch.alabaster".to_string(),
                        title: "Warm Alabaster".to_string(),
                        subtitle: Some("#F6F4EF".to_string()),
                        content: GridItemContent::ColorSwatch {
                            hex: "#F6F4EF".to_string(),
                        },
                        keywords: vec!["light".into(), "canvas".into(), "alabaster".into()],
                        filter_group: Some("colors".to_string()),
                        actions: ActionPanel::from_actions(vec![
                            ActionItem::new("hud.copied", "Copy #F6F4EF").shortcut("Enter"),
                        ]),
                    },
                    GridItem {
                        id: "swatch.copper-dark".to_string(),
                        title: "Signal Copper".to_string(),
                        subtitle: Some("#E87143".to_string()),
                        content: GridItemContent::ColorSwatch {
                            hex: "#E87143".to_string(),
                        },
                        keywords: vec!["accent".into(), "copper".into(), "dark".into()],
                        filter_group: Some("colors".to_string()),
                        actions: ActionPanel::from_actions(vec![
                            ActionItem::new("hud.copied", "Copy #E87143").shortcut("Enter"),
                        ]),
                    },
                    GridItem {
                        id: "swatch.copper-light".to_string(),
                        title: "Kiln Copper".to_string(),
                        subtitle: Some("#B54116".to_string()),
                        content: GridItemContent::ColorSwatch {
                            hex: "#B54116".to_string(),
                        },
                        keywords: vec!["accent".into(), "copper".into(), "light".into()],
                        filter_group: Some("colors".to_string()),
                        actions: ActionPanel::from_actions(vec![
                            ActionItem::new("hud.copied", "Copy #B54116").shortcut("Enter"),
                        ]),
                    },
                ],
            },
            GridSection {
                title: Some("Keyboard & Technical Glyphs".to_string()),
                subtitle: Some("Arrow keys navigate grid".to_string()),
                items: vec![
                    GridItem {
                        id: "sym.return".to_string(),
                        title: "Return Key".to_string(),
                        subtitle: Some("U+21B5".to_string()),
                        content: GridItemContent::Symbol {
                            glyph: "↵".to_string(),
                        },
                        keywords: vec!["enter".into(), "return".into(), "key".into()],
                        filter_group: Some("symbols".to_string()),
                        actions: ActionPanel::from_actions(vec![
                            ActionItem::new("hud.copied", "Copy Glyph ↵").shortcut("Enter"),
                        ]),
                    },
                    GridItem {
                        id: "sym.command".to_string(),
                        title: "Place of Interest".to_string(),
                        subtitle: Some("U+2318".to_string()),
                        content: GridItemContent::Symbol {
                            glyph: "⌘".to_string(),
                        },
                        keywords: vec!["super".into(), "meta".into(), "command".into()],
                        filter_group: Some("symbols".to_string()),
                        actions: ActionPanel::from_actions(vec![
                            ActionItem::new("hud.copied", "Copy Glyph ⌘").shortcut("Enter"),
                        ]),
                    },
                    GridItem {
                        id: "sym.left".to_string(),
                        title: "Back Arrow".to_string(),
                        subtitle: Some("U+2190".to_string()),
                        content: GridItemContent::Symbol {
                            glyph: "←".to_string(),
                        },
                        keywords: vec!["arrow".into(), "back".into(), "left".into()],
                        filter_group: Some("symbols".to_string()),
                        actions: ActionPanel::from_actions(vec![
                            ActionItem::new("hud.copied", "Copy Glyph ←").shortcut("Enter"),
                        ]),
                    },
                    GridItem {
                        id: "sym.section".to_string(),
                        title: "Section Sign".to_string(),
                        subtitle: Some("U+00A7".to_string()),
                        content: GridItemContent::Symbol {
                            glyph: "§".to_string(),
                        },
                        keywords: vec!["section".into(), "symbol".into()],
                        filter_group: Some("symbols".to_string()),
                        actions: ActionPanel::from_actions(vec![
                            ActionItem::new("hud.copied", "Copy Glyph §").shortcut("Enter"),
                        ]),
                    },
                ],
            },
        ],
        empty_view: Some(EmptyView {
            title: "No grid tiles match".to_string(),
            description: Some("Press Esc to clear your search query.".to_string()),
            icon: Some("0".to_string()),
            actions: ActionPanel::from_actions(vec![
                ActionItem::new("search.clear", "Clear Search").shortcut("Enter"),
            ]),
        }),
    });

    NavFrame::new("demo.grid", view)
}

#[must_use]
pub fn build_detail_frame() -> NavFrame {
    let compositor = detect_compositor();
    let compositor_name = match compositor {
        CompositorKind::Hyprland => "Hyprland",
        CompositorKind::Niri => "Niri",
        CompositorKind::GenericWayland => "Generic wlr-layer-shell",
    };

    let markdown = format!(
        "# Wayland Surface Inspector\n\n\
         AIOS renders its main command palette on a native Wayland `zwlr_layer_shell_v1` overlay surface using GPUI.\n\n\
         ## Active Runtime Details\n\n\
         - Detected compositor: `{compositor_name}`\n\
         - Surface namespace: `aios` (Overlay layer, Exclusive keyboard interactivity)\n\
         - HUD pill namespace: `aios-hud` (Overlay layer, None keyboard interactivity)\n\
         - Theme source: `org.freedesktop.portal.Settings` (`org.freedesktop.appearance.color-scheme`)\n\n\
         ### Quick IPC Commands\n\n\
         ```sh\n\
         aios daemon        # Start single-instance background host\n\
         aios toggle        # Reveal or hide the layer-shell launcher\n\
         aios open aios://  # Open a deep-link command view\n\
         ```\n\n\
         > Press `Ctrl+K` to inspect available actions, or `Esc` to return to the previous view."
    );

    let view = ViewNode::Detail(DetailView {
        navigation_title: Some("Detail + Metadata: Wayland Inspector".to_string()),
        is_loading: false,
        loading_message: None,
        markdown,
        metadata: vec![
            MetadataEntry::Label {
                title: "Compositor".to_string(),
                text: compositor_name.to_string(),
            },
            MetadataEntry::Label {
                title: "Protocol".to_string(),
                text: "wlr-layer-shell-v1".to_string(),
            },
            MetadataEntry::Label {
                title: "Keyboard Grab".to_string(),
                text: "Exclusive".to_string(),
            },
            MetadataEntry::Separator,
            MetadataEntry::Link {
                title: "IPC Socket".to_string(),
                text: "$XDG_RUNTIME_DIR/aios.sock".to_string(),
                target: "aios://status".to_string(),
            },
            MetadataEntry::TagList {
                title: "Capabilities".to_string(),
                tags: vec![
                    ("Pure Rust".to_string(), SemanticTone::Accent),
                    ("Local-First".to_string(), SemanticTone::Success),
                    ("Dual-Mode".to_string(), SemanticTone::Neutral),
                ],
            },
        ],
        actions: ActionPanel::from_actions(vec![
            ActionItem::new("hud.copied", "Copy Inspector Summary").shortcut("Enter"),
            ActionItem::new("theme.cycle", "Cycle Theme Mode").shortcut("Ctrl+Enter"),
            ActionItem::new("nav.pop", "Back to Root").shortcut("Esc"),
        ]),
    });

    NavFrame::new("demo.detail", view)
}

#[must_use]
pub fn build_form_frame() -> NavFrame {
    let view = ViewNode::Form(FormView {
        navigation_title: Some("Form View: Create Quicklink (Fixture)".to_string()),
        is_loading: false,
        enable_drafts: true,
        fields: vec![
            FormField::TextField {
                id: "name".to_string(),
                label: "Quicklink Name".to_string(),
                placeholder: Some("Enter a descriptive name (required)".to_string()),
                value: String::new(),
                required: true,
                error: None,
            },
            FormField::TextField {
                id: "url".to_string(),
                label: "URL or Local Path Template".to_string(),
                placeholder: Some("https://docs.rs/{query} (required)".to_string()),
                value: String::new(),
                required: true,
                error: None,
            },
            FormField::Dropdown {
                id: "open_with".to_string(),
                label: "Open With (Left/Right or Space to cycle)".to_string(),
                options: vec![
                    FilterOption {
                        id: "default".to_string(),
                        title: "Default Browser (xdg-open)".to_string(),
                    },
                    FilterOption {
                        id: "firefox".to_string(),
                        title: "Firefox".to_string(),
                    },
                    FilterOption {
                        id: "terminal".to_string(),
                        title: "Default Terminal".to_string(),
                    },
                ],
                selected_id: "default".to_string(),
                error: None,
            },
            FormField::PasswordField {
                id: "api_token".to_string(),
                label: "Optional Secret Token (Masked)".to_string(),
                placeholder: Some("Stored locally via Secret Service in Phase 4".to_string()),
                value: String::new(),
                required: false,
                error: None,
            },
            FormField::TextArea {
                id: "notes".to_string(),
                label: "Notes".to_string(),
                placeholder: Some("Optional multi-line usage notes...".to_string()),
                value: String::new(),
                required: false,
                error: None,
            },
            FormField::DatePicker {
                id: "review_date".to_string(),
                label: "Review Date (YYYY-MM-DD)".to_string(),
                value: String::new(),
                error: None,
            },
            FormField::Checkbox {
                id: "fallback".to_string(),
                label: "Fallback Command".to_string(),
                title: "Offer this quicklink when Root Search has no matches".to_string(),
                checked: false,
                error: None,
            },
            FormField::Separator,
            FormField::Description {
                title: "Keyboard Tips".to_string(),
                text: "Use Tab / Shift+Tab to move focus between fields. Press Enter or Ctrl+Enter to validate and submit.".to_string(),
            },
        ],
        actions: ActionPanel::from_actions(vec![
            ActionItem::new("form.submit", "Save Quicklink").shortcut("Enter"),
            ActionItem::new("form.reset", "Reset Form Fields").shortcut("Ctrl+Enter"),
        ]),
    });

    NavFrame::new("demo.form", view)
}

#[must_use]
pub fn build_empty_state_frame() -> NavFrame {
    NavFrame::new(
        "demo.empty",
        ViewNode::Empty(EmptyView {
            title: "No clipboard entries recorded yet".to_string(),
            description: Some(
                "Copy text, code, or an image in any Wayland app to populate history. Press Esc to return."
                    .to_string(),
            ),
            icon: Some("CB".to_string()),
            actions: ActionPanel::from_actions(vec![
                ActionItem::new("nav.pop", "Return to Workbench").shortcut("Enter"),
            ]),
        }),
    )
}

#[must_use]
pub fn build_loading_state_frame() -> NavFrame {
    NavFrame::new(
        "demo.loading",
        ViewNode::List(ListView {
            navigation_title: Some("Loading State (Fixture)".to_string()),
            search_placeholder: Some("Indexing desktop applications...".to_string()),
            filtering: FilterMode::Host,
            is_loading: true,
            loading_message: Some(
                "Scanning XDG .desktop entries and resolving icon theme cache...".to_string(),
            ),
            is_showing_detail: false,
            filter_dropdown: None,
            inline_arguments: Vec::new(),
            sections: Vec::new(),
            empty_view: Some(EmptyView {
                title: "Indexing desktop applications".to_string(),
                description: Some(
                    "The 2px header progress track signals active background work. Press Esc to return."
                        .to_string(),
                ),
                icon: Some("...".to_string()),
                actions: ActionPanel::from_actions(vec![
                    ActionItem::new("nav.pop", "Cancel & Return").shortcut("Enter"),
                ]),
            }),
        }),
    )
}

#[must_use]
pub fn build_error_state_frame() -> NavFrame {
    NavFrame::new(
        "demo.error",
        ViewNode::Error(ErrorView {
            title: "Plugin execution interrupted".to_string(),
            message:
                "Simulated WASM plugin exceeded its 250ms fuel budget and was isolated safely."
                    .to_string(),
            detail: Some(
                "plugin_id=demo.slow-worker  reason=epoch_timeout  host_status=healthy".to_string(),
            ),
            actions: ActionPanel::from_actions(vec![
                ActionItem::new("error.retry", "Retry Plugin Command").shortcut("Enter"),
                ActionItem::new("nav.pop", "Return to Workbench").shortcut("Ctrl+Enter"),
            ]),
        }),
    )
}

/// Dispatches an action ID inside the Phase 2 Workbench and returns any host [`ShellEffect`].
pub fn dispatch_demo_action(state: &mut LauncherState, action_id: &str) -> ShellEffect {
    match action_id {
        "open.list-detail" => {
            state.push(build_split_list_detail_frame());
            ShellEffect::None
        }
        "open.grid" => {
            state.push(build_grid_frame());
            ShellEffect::None
        }
        "open.detail" => {
            state.push(build_detail_frame());
            ShellEffect::None
        }
        "open.form" => {
            state.push(build_form_frame());
            ShellEffect::None
        }
        "open.empty" => {
            state.push(build_empty_state_frame());
            ShellEffect::None
        }
        "open.loading" => {
            state.push(build_loading_state_frame());
            ShellEffect::None
        }
        "open.error" => {
            state.push(build_error_state_frame());
            ShellEffect::None
        }
        "search.clear" => {
            let frame = state.current_mut();
            frame.search_query.clear();
            frame.selected_index = 0;
            frame.clamp_selection();
            ShellEffect::None
        }
        "nav.pop" => {
            state.pop();
            ShellEffect::None
        }
        "theme.cycle" => ShellEffect::CycleTheme,
        "theme.dark" => ShellEffect::SetTheme(aios_core::ThemeMode::Dark),
        "theme.light" => ShellEffect::SetTheme(aios_core::ThemeMode::Light),
        "theme.system" => ShellEffect::SetTheme(aios_core::ThemeMode::System),
        "toast.info" => {
            state.show_toast(
                ToastStyle::Success,
                "Action confirmed",
                Some("Toast rendered in footer without layout shift.".to_string()),
            );
            ShellEffect::None
        }
        "hud.ping" => ShellEffect::ShowHud(HudMessage {
            title: "AIOS HUD Overlay Active".to_string(),
        }),
        "hud.copied" => {
            state.show_toast(ToastStyle::Success, "Copied fixture value", None);
            ShellEffect::ShowHud(HudMessage {
                title: "Copied to Clipboard".to_string(),
            })
        }
        "overlay.confirm" => {
            state.open_confirm_alert(ConfirmAlert {
                id: "confirm.delete-demo".to_string(),
                title: "Delete selected fixture item?".to_string(),
                message: "This exercises the destructive ConfirmAlert modal. No real user files are modified.".to_string(),
                primary_action: "Confirm Delete".to_string(),
                dismiss_action: "Cancel".to_string(),
                destructive: true,
            });
            ShellEffect::None
        }
        "confirm.delete-demo" => {
            state.overlay = None;
            state.show_toast(
                ToastStyle::Success,
                "Confirmed destructive alert",
                Some("Modal dismissed cleanly.".to_string()),
            );
            ShellEffect::None
        }
        "form.submit" => match state.validate_active_form() {
            Ok(values) => {
                let name = values
                    .iter()
                    .find(|(k, _)| k == "name")
                    .map(|(_, v)| v.as_str())
                    .unwrap_or("Quicklink");
                state.show_toast(
                    ToastStyle::Success,
                    format!("Saved \"{name}\""),
                    Some("All form fields passed validation.".to_string()),
                );
                ShellEffect::ShowHud(HudMessage {
                    title: format!("Saved {name}"),
                })
            }
            Err(err_count) => {
                state.show_toast(
                    ToastStyle::Failure,
                    format!("{err_count} validation error(s)"),
                    Some("Fill required fields highlighted below.".to_string()),
                );
                ShellEffect::None
            }
        },
        "form.reset" => {
            let fresh = build_form_frame();
            *state.current_mut() = fresh;
            state.show_toast(ToastStyle::Success, "Form fields reset", None);
            ShellEffect::None
        }
        "error.retry" => {
            state.show_toast(
                ToastStyle::Success,
                "Recovery retry dispatched",
                Some("Host remained responsive after plugin error isolation.".to_string()),
            );
            ShellEffect::None
        }
        other => {
            if let Some(ActiveOverlay::ActionPanel { .. }) = state.overlay {
                state.overlay = None;
            }
            ShellEffect::ActionTriggered(other.to_string())
        }
    }
}

/// Navigates to a deep-link URI such as `aios://search?q=screenshot`, `aios://demo/grid`,
/// `aios://demo/form`, or `aios://demo/detail`.
pub fn open_deep_link(state: &mut LauncherState, uri: &str) {
    let normalized = uri.trim().trim_start_matches("aios://");
    let (route, query_str) = match normalized.split_once('?') {
        Some((r, q)) => (r.trim_end_matches('/'), Some(q)),
        None => (normalized.trim_end_matches('/'), None),
    };
    state.pop_to_root();
    match route {
        "search" => {
            if let Some(qs) = query_str {
                let mut query_text: Option<String> = None;
                let mut filter_id: Option<String> = None;
                for pair in qs.split('&') {
                    if pair.is_empty() {
                        continue;
                    }
                    let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
                    let key = percent_decode(k);
                    let val = percent_decode(v);
                    match key.as_str() {
                        "q" | "query" => query_text = Some(val),
                        "filter" => filter_id = Some(val),
                        _ => {}
                    }
                }
                let root = state.current_mut();
                if let Some(q) = query_text {
                    root.search_query = q;
                }
                if let ViewNode::List(list) = &mut root.view
                    && let Some(dropdown) = &mut list.filter_dropdown
                {
                    if let Some(fid) = filter_id
                        && dropdown.options.iter().any(|o| o.id == fid)
                    {
                        dropdown.selected_id = fid;
                    } else if dropdown.options.iter().any(|o| o.id == "all") {
                        dropdown.selected_id = "all".to_string();
                    }
                }
                root.selected_index = 0;
                root.header_focus = crate::state::HeaderFocus::Search;
                root.clamp_selection();
            }
        }
        "workbench" | "demo" | "demo/workbench" => state.push(build_root_showcase_frame()),
        "demo/list-detail" | "list-detail" => state.push(build_split_list_detail_frame()),
        "demo/grid" | "grid" => state.push(build_grid_frame()),
        "demo/detail" | "detail" => state.push(build_detail_frame()),
        "demo/form" | "form" => state.push(build_form_frame()),
        "demo/empty" | "empty" => state.push(build_empty_state_frame()),
        "demo/loading" | "loading" => state.push(build_loading_state_frame()),
        "demo/error" | "error" => state.push(build_error_state_frame()),
        "root" | "" => {}
        _ => {
            state.show_toast(
                ToastStyle::Failure,
                format!("No handler installed for {uri}"),
                Some(
                    "Supported URIs: aios://search?q=..., aios://settings, aios://demo/grid"
                        .to_string(),
                ),
            );
        }
    }
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                if let (Some(hi), Some(lo)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2])) {
                    out.push((hi << 4) | lo);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
