//! Declarative Settings & Plugins views built with AIOS's own Multi-View UI AST.

use std::collections::HashMap;

use aios_core::{AiosConfig, AiosPaths};
use aios_plugin_sdk::{
    ActionItem, ActionPanel, ActionPanelBuilder, ConfirmAlert, EmptyView, FilterOption, FormField,
    FormViewBuilder, ListItemBuilder, ListViewBuilder, MetadataEntry, PreferenceKind, SemanticTone,
    ViewNode,
};

use crate::manifest::InstalledPlugin;

/// Builds the main Settings & Plugins `List + Split Detail` view.
#[must_use]
pub fn build_settings_home_view(
    config: &AiosConfig,
    paths: &AiosPaths,
    plugins: &[InstalledPlugin],
    permissions_by_plugin: &HashMap<String, HashMap<String, bool>>,
) -> ViewNode {
    let mut builder = ListViewBuilder::new("AIOS Settings & Plugins")
        .placeholder("Search settings, WASM plugins, or capabilities...")
        .showing_detail(true)
        .filter_dropdown(
            "Filter Settings",
            "all",
            vec![
                FilterOption {
                    id: "all".to_string(),
                    title: "All Settings".to_string(),
                },
                FilterOption {
                    id: "plugins".to_string(),
                    title: "WASM Plugins".to_string(),
                },
                FilterOption {
                    id: "general".to_string(),
                    title: "General & Config".to_string(),
                },
            ],
        );

    // Section 1: General & Appearance
    let aliases_md = if config.aliases.is_empty() {
        "No custom aliases configured.".to_string()
    } else {
        config
            .aliases
            .iter()
            .map(|(k, v)| format!("- `{k}` -> `{v}`"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let fallbacks_md = if config.fallback_commands.is_empty() {
        "No fallback commands configured.".to_string()
    } else {
        config
            .fallback_commands
            .iter()
            .map(|fb| format!("- **{}** (`{}`): `{}`", fb.title, fb.id, fb.template))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let general_items = vec![
        ListItemBuilder::new("settings:appearance", "Appearance & Theme Mode")
            .subtitle(format!("Current: {:?}", config.theme.mode))
            .icon("TH")
            .filter_group("general")
            .keyword("theme")
            .keyword("dark")
            .keyword("light")
            .keyword("system")
            .accessory_tag(format!("{:?}", config.theme.mode), SemanticTone::Accent)
            .detail(
                format!(
                    "# Appearance & Theme\n\nSwitch between Basalt Dark, Warm Alabaster Light, or follow the live XDG Desktop Portal color scheme.\n\n- Active config mode: `{:?}`\n- Translucent root surface: `{}`\n- Reduced motion: `{}`",
                    config.theme.mode,
                    config.theme.translucent_root,
                    config.theme.reduced_motion
                ),
                vec![
                    MetadataEntry::Label {
                        title: "Config File".to_string(),
                        text: paths.config_file().display().to_string(),
                    },
                    MetadataEntry::Label {
                        title: "Window Size".to_string(),
                        text: format!("{}x{} px", config.window.width, config.window.default_height),
                    },
                ],
            )
            .actions(
                ActionPanelBuilder::new()
                    .action(ActionItem::new("core.cycle-theme", "Cycle Theme Mode").shortcut("Enter"))
                    .action(ActionItem::new("theme.dark", "Use Dark (Basalt)"))
                    .action(ActionItem::new("theme.light", "Use Light (Alabaster)"))
                    .action(ActionItem::new("theme.system", "Follow System Portal"))
                    .build(),
            )
            .build(),
        ListItemBuilder::new("settings:aliases-fallbacks", "Aliases & Fallback Commands")
            .subtitle(format!(
                "{} aliases, {} fallbacks",
                config.aliases.len(),
                config.fallback_commands.len()
            ))
            .icon("AL")
            .filter_group("general")
            .keyword("alias")
            .keyword("fallback")
            .keyword("hotkeys")
            .detail(
                format!(
                    "# Aliases & Fallback Commands\n\n## Aliases\n\n{aliases_md}\n\n## Fallback Commands\n\n{fallbacks_md}\n\nEdit `{}` to customize aliases, hotkeys, and fallback templates.",
                    paths.config_file().display()
                ),
                vec![
                    MetadataEntry::Label {
                        title: "Aliases".to_string(),
                        text: config.aliases.len().to_string(),
                    },
                    MetadataEntry::Label {
                        title: "Fallbacks".to_string(),
                        text: config.fallback_commands.len().to_string(),
                    },
                    MetadataEntry::Label {
                        title: "Hotkeys".to_string(),
                        text: config.hotkeys.len().to_string(),
                    },
                ],
            )
            .actions(
                ActionPanelBuilder::new()
                    .action(
                        ActionItem::new("settings.copy-config-path", "Copy config.toml Path")
                            .shortcut("Enter"),
                    )
                    .action(
                        ActionItem::new("settings.reload-plugins", "Reload WASM Plugins")
                            .shortcut("Ctrl+Enter"),
                    )
                    .build(),
            )
            .build(),
        ListItemBuilder::new("settings:install-local", "Install Local WASM Plugin...")
            .subtitle("Copy a local plugin folder (aios-plugin.toml + plugin.wasm)")
            .icon("IN")
            .filter_group("plugins")
            .keyword("install")
            .keyword("wasm")
            .keyword("extension")
            .accessory_tag("Local", SemanticTone::Success)
            .detail(
                format!(
                    "# Install Local WASM Plugin\n\nPoint AIOS to any directory containing a valid `aios-plugin.toml` manifest and its compiled `plugin.wasm` component.\n\nPlugins are installed into:\n\n```text\n{}\n```",
                    paths.plugins_dir().display()
                ),
                vec![MetadataEntry::Label {
                    title: "Plugins Directory".to_string(),
                    text: paths.plugins_dir().display().to_string(),
                }],
            )
            .actions(ActionPanel::from_actions(vec![
                ActionItem::new("settings.install-plugin-form", "Open Install Form")
                    .shortcut("Enter"),
            ]))
            .build(),
    ];

    builder = builder.section(Some("General & Management"), None, general_items);

    // Section 2: Installed WASM Plugins
    if !plugins.is_empty() {
        let plugin_items = plugins
            .iter()
            .map(|plugin| {
                let m = &plugin.manifest;
                let declared_caps = m.capabilities.declared_keys();
                let grants = permissions_by_plugin.get(&m.id);
                let approved_count = declared_caps
                    .iter()
                    .filter(|cap| grants.and_then(|g| g.get(*cap)).copied() == Some(true))
                    .count();

                let caps_summary = if declared_caps.is_empty() {
                    "Zero privileged capabilities requested (pure sandbox).".to_string()
                } else {
                    declared_caps
                        .iter()
                        .map(|cap| {
                            let state = if grants.and_then(|g| g.get(cap)).copied() == Some(true) {
                                "Approved"
                            } else {
                                "Pending / Denied"
                            };
                            format!("- `{cap}`: **{state}**")
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                };

                let cmds_summary = m
                    .commands
                    .iter()
                    .map(|c| format!("- **{}** (`{}` · `{:?}`)", c.title, c.id, c.mode))
                    .collect::<Vec<_>>()
                    .join("\n");

                let first_cmd = m.commands.first().map(|c| c.id.clone()).unwrap_or_default();

                let mut actions = ActionPanelBuilder::new().section(
                    Some("Plugin"),
                    vec![
                        ActionItem::new(
                            format!("settings.plugin-caps:{}", m.id),
                            "Manage Capability Approvals",
                        )
                        .shortcut("Enter"),
                        ActionItem::new(
                            format!("settings.plugin-prefs:{}", m.id),
                            "Configure Preferences",
                        )
                        .shortcut("Ctrl+Enter"),
                        ActionItem::new(
                            format!("plugin.run:{}:{}", m.id, first_cmd),
                            "Launch Primary Command",
                        ),
                        ActionItem::new(format!("plugin.reload:{}", m.id), "Reload WASM Bundle"),
                        ActionItem::new(
                            format!("settings.plugin-uninstall-prompt:{}", m.id),
                            "Uninstall Plugin",
                        )
                        .destructive(),
                    ],
                );
                if m.preferences.is_empty() {
                    actions = ActionPanelBuilder::new().section(
                        Some("Plugin"),
                        vec![
                            ActionItem::new(
                                format!("settings.plugin-caps:{}", m.id),
                                "Manage Capability Approvals",
                            )
                            .shortcut("Enter"),
                            ActionItem::new(
                                format!("plugin.run:{}:{}", m.id, first_cmd),
                                "Launch Primary Command",
                            )
                            .shortcut("Ctrl+Enter"),
                            ActionItem::new(
                                format!("plugin.reload:{}", m.id),
                                "Reload WASM Bundle",
                            ),
                            ActionItem::new(
                                format!("settings.plugin-uninstall-prompt:{}", m.id),
                                "Uninstall Plugin",
                            )
                            .destructive(),
                        ],
                    );
                }

                let mut item = ListItemBuilder::new(format!("ext:{}", m.id), m.name.clone())
                    .subtitle(format!("v{} · {}", m.version, m.id))
                    .icon("WM")
                    .filter_group("plugins")
                    .keyword("plugin")
                    .keyword("wasm")
                    .keyword(m.id.clone())
                    .accessory_tag(
                        format!("{approved_count}/{} caps", declared_caps.len()),
                        if approved_count == declared_caps.len() {
                            SemanticTone::Success
                        } else {
                            SemanticTone::Warning
                        },
                    )
                    .detail(
                        format!(
                            "# {}\n\n{}\n\n## Commands\n\n{}\n\n## Capabilities\n\n{}",
                            m.name,
                            if m.description.is_empty() {
                                "External WASM Component plugin."
                            } else {
                                m.description.as_str()
                            },
                            cmds_summary,
                            caps_summary
                        ),
                        vec![
                            MetadataEntry::Label {
                                title: "Plugin ID".to_string(),
                                text: m.id.clone(),
                            },
                            MetadataEntry::Label {
                                title: "Version".to_string(),
                                text: m.version.clone(),
                            },
                            MetadataEntry::Label {
                                title: "Author".to_string(),
                                text: if m.author.is_empty() {
                                    "Local".to_string()
                                } else {
                                    m.author.clone()
                                },
                            },
                            MetadataEntry::Link {
                                title: "WASM Entry".to_string(),
                                text: plugin.wasm_path.display().to_string(),
                                target: plugin.wasm_path.display().to_string(),
                            },
                        ],
                    )
                    .actions(actions.build());

                if let Some(icon_path) = &plugin.icon_path {
                    item = item.icon_path(icon_path.display().to_string());
                }
                item.build()
            })
            .collect();

        builder = builder.section(
            Some("Installed WASM Plugins"),
            Some(&format!("{} installed", plugins.len())),
            plugin_items,
        );
    }

    builder
        .empty_view(EmptyView {
            title: "No matching settings or plugins".to_string(),
            description: Some(
                "Press Esc to clear the filter or install a local WASM plugin.".to_string(),
            ),
            icon: Some("0".to_string()),
            actions: ActionPanel::from_actions(vec![
                ActionItem::new("search.clear", "Clear Search").shortcut("Enter"),
            ]),
        })
        .build()
}

/// Builds the Capability Approval manager list for one installed WASM plugin.
#[must_use]
pub fn build_plugin_capabilities_view(
    plugin: &InstalledPlugin,
    grants: &HashMap<String, bool>,
) -> ViewNode {
    let declared = plugin.manifest.capabilities.declared_keys();
    let items = declared
        .iter()
        .map(|cap| {
            let approved = grants.get(cap).copied().unwrap_or(false);
            let toggle_label = if approved {
                "Revoke Capability"
            } else {
                "Approve Capability"
            };
            let action_id = if approved {
                format!("settings.cap-set:{}:{}:deny", plugin.manifest.id, cap)
            } else {
                format!("settings.cap-set:{}:{}:grant", plugin.manifest.id, cap)
            };

            ListItemBuilder::new(format!("cap:{cap}"), cap.clone())
                .subtitle(if approved {
                    "Granted by user in SQLite plugin_permissions"
                } else {
                    "Denied by default until explicitly approved"
                })
                .icon(if approved { "OK" } else { "NO" })
                .accessory_tag(
                    if approved { "Approved" } else { "Denied" },
                    if approved {
                        SemanticTone::Success
                    } else {
                        SemanticTone::Danger
                    },
                )
                .actions(ActionPanel::from_actions(vec![
                    ActionItem::new(action_id, toggle_label).shortcut("Enter"),
                    ActionItem::new("nav.pop", "Back to Settings").shortcut("Esc"),
                ]))
                .build()
        })
        .collect();

    ListViewBuilder::new(format!("Capabilities: {}", plugin.manifest.name))
        .placeholder("Filter declared capabilities...")
        .section(
            Some("Declared in aios-plugin.toml"),
            Some(&format!("{} total", declared.len())),
            items,
        )
        .empty_view(EmptyView {
            title: "Zero capabilities requested".to_string(),
            description: Some(format!(
                "{} runs in a pure sandbox with no clipboard, filesystem, network, or secret access.",
                plugin.manifest.name
            )),
            icon: Some("OK".to_string()),
            actions: ActionPanel::from_actions(vec![
                ActionItem::new("nav.pop", "Back to Settings").shortcut("Enter"),
            ]),
        })
        .build()
}

/// Builds the Preferences `FormView` for one installed WASM plugin.
#[must_use]
pub fn build_plugin_preferences_form(
    plugin: &InstalledPlugin,
    current_values: &HashMap<String, String>,
) -> ViewNode {
    let mut form = FormViewBuilder::new(format!("Preferences: {}", plugin.manifest.name));

    if plugin.manifest.preferences.is_empty() {
        form = form.field(FormField::Description {
            title: "No Preferences Declared".to_string(),
            text: format!(
                "{} does not declare any [[preferences]] entries in aios-plugin.toml.",
                plugin.manifest.name
            ),
        });
    } else {
        for pref in &plugin.manifest.preferences {
            let value = current_values
                .get(&pref.name)
                .cloned()
                .or_else(|| pref.default.clone())
                .unwrap_or_default();

            let field = match pref.kind {
                PreferenceKind::Textfield => FormField::TextField {
                    id: pref.name.clone(),
                    label: pref.title.clone(),
                    placeholder: pref.description.clone().or_else(|| pref.default.clone()),
                    value,
                    required: pref.required,
                    error: None,
                },
                PreferenceKind::Password => FormField::PasswordField {
                    id: pref.name.clone(),
                    label: pref.title.clone(),
                    placeholder: pref.description.clone(),
                    value,
                    required: pref.required,
                    error: None,
                },
                PreferenceKind::Checkbox => FormField::Checkbox {
                    id: pref.name.clone(),
                    label: pref.title.clone(),
                    title: pref
                        .description
                        .clone()
                        .unwrap_or_else(|| pref.title.clone()),
                    checked: value.eq_ignore_ascii_case("true"),
                    error: None,
                },
                PreferenceKind::Dropdown => {
                    let selected_id = if !value.is_empty() {
                        value
                    } else {
                        pref.options
                            .first()
                            .map(|o| o.id.clone())
                            .unwrap_or_default()
                    };
                    FormField::Dropdown {
                        id: pref.name.clone(),
                        label: pref.title.clone(),
                        options: pref.options.clone(),
                        selected_id,
                        error: None,
                    }
                }
            };
            form = form.field(field);
        }
    }

    form.actions(ActionPanel::from_actions(vec![
        ActionItem::new(
            format!("settings.plugin-prefs-save:{}", plugin.manifest.id),
            "Save Preferences",
        )
        .shortcut("Enter"),
        ActionItem::new("nav.pop", "Cancel").shortcut("Esc"),
    ]))
    .build()
}

/// Builds the Install Local Plugin `FormView`.
#[must_use]
pub fn build_install_plugin_form(plugins_dir_hint: &str) -> ViewNode {
    FormViewBuilder::new("Install Local WASM Plugin")
        .field(FormField::TextField {
            id: "source_path".to_string(),
            label: "Plugin Bundle Directory".to_string(),
            placeholder: Some("/path/to/plugin-dir (contains aios-plugin.toml + plugin.wasm)".to_string()),
            value: String::new(),
            required: true,
            error: None,
        })
        .field(FormField::Description {
            title: "Install Target".to_string(),
            text: format!(
                "AIOS validates aios-plugin.toml and copies the bundle into {plugins_dir_hint}. Capabilities remain denied until approved."
            ),
        })
        .actions(ActionPanel::from_actions(vec![
            ActionItem::new("settings.install-plugin-submit", "Install Plugin").shortcut("Enter"),
            ActionItem::new("nav.pop", "Cancel").shortcut("Esc"),
        ]))
        .build()
}

/// Builds the destructive uninstall `ConfirmAlert` modal.
#[must_use]
pub fn build_uninstall_confirm_alert(plugin: &InstalledPlugin) -> ConfirmAlert {
    ConfirmAlert {
        id: format!("settings.plugin-uninstall-confirm:{}", plugin.manifest.id),
        title: format!("Uninstall \"{}\"?", plugin.manifest.name),
        message: format!(
            "Removes {} and purges all saved permissions, preferences, and local storage for '{}'.",
            plugin.root_dir.display(),
            plugin.manifest.id
        ),
        primary_action: "Uninstall Plugin".to_string(),
        dismiss_action: "Cancel".to_string(),
        destructive: true,
    }
}
