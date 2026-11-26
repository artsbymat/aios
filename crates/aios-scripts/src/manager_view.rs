//! Dedicated declarative Script Commands Manager view (`List + Split Detail`),
//! completely separate from WASM Plugins & Settings.

use std::collections::HashMap;

use aios_core::AiosPaths;
use aios_plugin_sdk::{
    ActionItem, ActionPanel, ActionPanelBuilder, EmptyView, FilterOption, ListItemBuilder,
    ListViewBuilder, MetadataEntry, SemanticTone, ViewNode,
};

use crate::runner::{ScriptCommand, ScriptMode};

/// Builds the dedicated Script Commands Manager `List + Split Detail` view.
#[must_use]
pub fn build_scripts_manager_view(
    paths: &AiosPaths,
    scripts: &[ScriptCommand],
    inline_summaries: &HashMap<String, String>,
) -> ViewNode {
    let scripts_dir_display = paths.scripts_dir().display().to_string();

    let mut builder = ListViewBuilder::new("Script Commands")
        .placeholder("Search script commands by title, mode, or package...")
        .showing_detail(true)
        .filter_dropdown(
            "Filter Script Mode",
            "all",
            vec![
                FilterOption {
                    id: "all".to_string(),
                    title: "All Scripts".to_string(),
                },
                FilterOption {
                    id: "silent".to_string(),
                    title: "Silent".to_string(),
                },
                FilterOption {
                    id: "compact".to_string(),
                    title: "Compact".to_string(),
                },
                FilterOption {
                    id: "fullOutput".to_string(),
                    title: "Full Output".to_string(),
                },
                FilterOption {
                    id: "inline".to_string(),
                    title: "Inline".to_string(),
                },
            ],
        );

    if !scripts.is_empty() {
        let script_items = scripts
            .iter()
            .map(|sc| {
                let inline_summary = inline_summaries.get(&sc.id);
                let mut detail_md = format!(
                    "# {}\n\n{}\n\n- Mode: `{}`\n- Needs confirmation: `{}`\n- Arguments: `{}`",
                    sc.title,
                    sc.description
                        .as_deref()
                        .unwrap_or("Local script command discovered in scripts directory."),
                    sc.mode.label(),
                    sc.needs_confirmation,
                    sc.arguments.len()
                );
                if sc.mode == ScriptMode::FullOutput
                    && let Some(secs) = sc.refresh_time_secs
                {
                    detail_md.push_str(&format!("\n- Auto-refresh: `every {secs}s`"));
                }
                if let Some(summary) = inline_summary {
                    detail_md.push_str(&format!("\n- Latest inline output: `{summary}`"));
                }
                if !sc.arguments.is_empty() {
                    detail_md.push_str("\n\n## Arguments\n");
                    for arg in &sc.arguments {
                        let req = if arg.required { "required" } else { "optional" };
                        detail_md
                            .push_str(&format!("\n- `{}` — {} ({req})", arg.id, arg.placeholder));
                    }
                }

                let mut metadata = vec![
                    MetadataEntry::Label {
                        title: "Script ID".to_string(),
                        text: sc.id.clone(),
                    },
                    MetadataEntry::Label {
                        title: "Mode".to_string(),
                        text: sc.mode.label().to_string(),
                    },
                ];
                if sc.mode == ScriptMode::FullOutput
                    && let Some(secs) = sc.refresh_time_secs
                {
                    metadata.push(MetadataEntry::Label {
                        title: "Auto-Refresh".to_string(),
                        text: format!("Every {secs}s"),
                    });
                }
                metadata.push(MetadataEntry::Link {
                    title: "Path".to_string(),
                    text: sc.path.display().to_string(),
                    target: sc.path.display().to_string(),
                });

                let mut item =
                    ListItemBuilder::new(format!("mgr-script:{}", sc.id), sc.title.clone())
                        .subtitle(
                            sc.package_name
                                .clone()
                                .unwrap_or_else(|| "Script Command".to_string()),
                        )
                        .icon(sc.badge_label())
                        .filter_group(sc.mode.label())
                        .keyword("script")
                        .keyword(sc.mode.label())
                        .accessory_tag(sc.mode.label(), SemanticTone::Neutral)
                        .detail(detail_md, metadata)
                        .actions(
                            ActionPanelBuilder::new()
                                .section(
                                    Some("Script"),
                                    vec![
                                        ActionItem::new(
                                            format!("script.run:{}", sc.id),
                                            "Run Script Command",
                                        )
                                        .shortcut("Enter"),
                                        ActionItem::new(
                                            format!("script.copy-path:{}", sc.id),
                                            "Copy Script Path",
                                        )
                                        .shortcut("Ctrl+Enter"),
                                        ActionItem::new("scripts.reload", "Reload Script Commands"),
                                    ],
                                )
                                .build(),
                        );
                if let Some(icon_path) = &sc.icon_path {
                    item = item.icon_path(icon_path.display().to_string());
                }
                if let Some(summary) = inline_summary {
                    item = item.accessory_text(summary.clone());
                }
                if let Some(pkg) = &sc.package_name {
                    item = item.keyword(pkg.clone());
                }
                item.build()
            })
            .collect();

        builder = builder.section(
            Some("Discovered Script Commands"),
            Some(&format!("{} discovered", scripts.len())),
            script_items,
        );
    }

    let management_items = vec![
        ListItemBuilder::new("scripts:reload", "Reload Script Commands")
            .subtitle(format!("Rescan {scripts_dir_display}"))
            .icon("RL")
            .keyword("reload")
            .keyword("refresh")
            .keyword("rescan")
            .detail(
                format!(
                    "# Reload Script Commands\n\nRescans `{scripts_dir_display}` for files declaring `# @aios.schemaVersion` and `# @aios.title` metadata headers, and refreshes all `inline` script summaries."
                ),
                vec![
                    MetadataEntry::Label {
                        title: "Discovered".to_string(),
                        text: scripts.len().to_string(),
                    },
                    MetadataEntry::Label {
                        title: "Directory".to_string(),
                        text: scripts_dir_display.clone(),
                    },
                ],
            )
            .actions(ActionPanel::from_actions(vec![
                ActionItem::new("scripts.reload", "Reload Script Commands").shortcut("Enter"),
                ActionItem::new("scripts.copy-dir", "Copy Scripts Directory Path")
                    .shortcut("Ctrl+Enter"),
            ]))
            .build(),
        ListItemBuilder::new("scripts:directory", "Scripts Directory & Template")
            .subtitle(scripts_dir_display.clone())
            .icon("DR")
            .keyword("directory")
            .keyword("folder")
            .keyword("template")
            .keyword("new")
            .detail(
                format!(
                    "# Scripts Directory\n\nPlace executable scripts in:\n\n```text\n{scripts_dir_display}\n```\n\n## Minimal Header Template\n\n```bash\n#!/usr/bin/env bash\n# @aios.schemaVersion 1\n# @aios.title My Script\n# @aios.mode compact\n# @aios.packageName Personal\necho \"Done\"\n```\n\nSupported modes: `silent`, `compact`, `fullOutput`, `inline`."
                ),
                vec![MetadataEntry::Link {
                    title: "Scripts Dir".to_string(),
                    text: scripts_dir_display.clone(),
                    target: scripts_dir_display,
                }],
            )
            .actions(ActionPanel::from_actions(vec![
                ActionItem::new("scripts.copy-dir", "Copy Scripts Directory Path")
                    .shortcut("Enter"),
                ActionItem::new("scripts.reload", "Reload Script Commands").shortcut("Ctrl+Enter"),
            ]))
            .build(),
    ];

    builder = builder.section(Some("Management"), None, management_items);

    builder
        .empty_view(EmptyView {
            title: "No matching script commands".to_string(),
            description: Some(
                "Press Esc to clear the filter, or add scripts with @aios.schemaVersion headers to your scripts directory."
                    .to_string(),
            ),
            icon: Some("0".to_string()),
            actions: ActionPanel::from_actions(vec![
                ActionItem::new("search.clear", "Clear Search").shortcut("Enter"),
                ActionItem::new("scripts.reload", "Reload Script Commands"),
            ]),
        })
        .build()
}
