use std::fs;
use std::path::PathBuf;

use aios_core::AiosPaths;
use aios_plugin_sdk::{PluginResponse, ViewNode};
use aios_scripts::{ScriptHost, ScriptMode, discover_script_commands};
use tempfile::TempDir;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn temp_paths(tmp: &TempDir) -> AiosPaths {
    let base = tmp.path();
    let paths = AiosPaths {
        config_dir: base.join("config"),
        data_dir: base.join("data"),
        cache_dir: base.join("cache"),
        runtime_socket: base.join("aios.sock"),
    };
    paths.ensure_dirs().expect("ensure dirs");
    paths
}

#[test]
fn reference_script_commands_and_confirmation_flow_run_end_to_end() {
    let tmp = TempDir::new().expect("temp dir");
    let paths = temp_paths(&tmp);

    for file in ["system-uptime.sh", "disk-usage.sh"] {
        let src = repo_root().join("examples/scripts").join(file);
        let dst = paths.scripts_dir().join(file);
        fs::copy(&src, &dst).expect("copy script");
    }
    fs::write(
        paths.scripts_dir().join("danger-clean.sh"),
        "#!/usr/bin/env sh\n# @aios.schemaVersion 1\n# @aios.title Clean Temp Cache\n# @aios.mode silent\n# @aios.needsConfirmation true\necho \"Cleaned $1\"\n",
    )
    .expect("write confirm script");

    let discovered = discover_script_commands(&paths.scripts_dir());
    assert_eq!(discovered.len(), 3);
    assert!(
        discovered
            .iter()
            .any(|s| s.id == "script:system-uptime" && s.mode == ScriptMode::Inline)
    );
    assert!(discovered.iter().any(|s| s.id == "script:disk-usage"
        && s.mode == ScriptMode::FullOutput
        && s.arguments.len() == 1));

    let mut host = ScriptHost::new(paths);
    let entries = host.root_search_entries();
    assert_eq!(entries.len(), 3);
    assert!(entries.iter().all(|e| e.filter_group == "scripts"));

    let uptime_entry = entries
        .into_iter()
        .find(|e| e.id == "script:system-uptime")
        .expect("uptime root entry");
    assert!(uptime_entry.inline_summary.is_some());

    // Run `disk-usage` (fullOutput) with argument `/tmp` -> returns DetailView with Copy Output.
    let du_res = host.run_script("script:disk-usage", &["/tmp".to_string()], false);
    assert!(matches!(
        du_res.response,
        PluginResponse::Push { ref view, .. } if matches!(view.as_ref(), ViewNode::Detail(d) if d.markdown.contains("/tmp"))
    ));

    // Copy Output returns the captured output from the last run.
    let copy_out = host
        .dispatch_action("script.copy-output:script:disk-usage", &[])
        .expect("copy output");
    assert!(matches!(
        copy_out.response,
        PluginResponse::CopyToClipboard { ref text, .. } if text.contains("/tmp")
    ));

    // Run `danger-clean` -> first returns ConfirmAlert, then executes after confirmation.
    let confirm_req = host.run_script("script:danger-clean", &["cache".to_string()], false);
    let alert_id = match confirm_req.response {
        PluginResponse::Confirm { alert } => alert.id,
        other => panic!("expected ConfirmAlert, got {other:?}"),
    };
    let after_confirm = host
        .dispatch_action(&alert_id, &[])
        .expect("confirm execution");
    assert!(matches!(
        after_confirm.response,
        PluginResponse::CloseWindow { hud: Some(h) } if h.title == "Cleaned cache"
    ));
}

#[test]
fn script_manager_view_reload_and_copy_dir_work_independently() {
    let tmp = TempDir::new().expect("temp dir");
    let paths = temp_paths(&tmp);
    let mut host = ScriptHost::new(paths.clone());

    assert!(host.scripts().is_empty());

    // Open manager view before any scripts exist -> Management section is present.
    let open_res = host
        .dispatch_action("core.open-scripts", &[])
        .expect("open scripts manager");
    assert!(matches!(
        open_res.response,
        PluginResponse::Push { ref frame_id, ref view }
            if frame_id.as_deref() == Some("scripts.manager")
            && matches!(view.as_ref(), ViewNode::List(l) if l.sections.len() == 1)
    ));

    // Add a script and reload via `scripts.reload`.
    fs::write(
        paths.scripts_dir().join("hello.sh"),
        "#!/usr/bin/env sh\n# @aios.schemaVersion 1\n# @aios.title Hello Script\n# @aios.mode compact\necho \"hi\"\n",
    )
    .expect("write script");

    let reload_res = host
        .dispatch_action("scripts.reload", &[])
        .expect("reload scripts");
    assert_eq!(host.scripts().len(), 1);
    assert!(host.find_script("hello").is_some());
    assert!(host.find_script("script:hello").is_some());
    assert!(matches!(
        reload_res.response,
        PluginResponse::Render { ref view } if matches!(view.as_ref(), ViewNode::List(l) if l.sections.len() == 2)
    ));
    assert!(reload_res.toast.is_some());

    let copy_dir = host
        .dispatch_action("scripts.copy-dir", &[])
        .expect("copy dir");
    assert!(matches!(
        copy_dir.response,
        PluginResponse::CopyToClipboard { ref text, .. } if text == &paths.scripts_dir().display().to_string()
    ));
}

#[test]
fn script_custom_icons_resolve_relative_absolute_xdg_and_badge_fallback() {
    let tmp = TempDir::new().expect("temp dir");
    let paths = temp_paths(&tmp);
    let scripts_dir = paths.scripts_dir();
    let icons_dir = scripts_dir.join("icons");
    fs::create_dir_all(&icons_dir).expect("create icons dir");

    let rel_svg = icons_dir.join("disk.svg");
    fs::write(&rel_svg, "<svg xmlns=\"http://www.w3.org/2000/svg\"/>").expect("write rel svg");

    let abs_png = tmp.path().join("custom-tool.png");
    fs::write(&abs_png, b"\x89PNG\r\n\x1a\n").expect("write abs png");

    let xdg_svg = tmp.path().join("drive-harddisk.svg");
    fs::write(&xdg_svg, "<svg xmlns=\"http://www.w3.org/2000/svg\"/>").expect("write xdg svg");

    fs::write(
        scripts_dir.join("rel-icon.sh"),
        "#!/usr/bin/env sh\n# @aios.schemaVersion 1\n# @aios.title Disk Usage\n# @aios.icon ./icons/disk.svg\necho ok\n",
    )
    .expect("write rel script");
    fs::write(
        scripts_dir.join("abs-icon.sh"),
        format!(
            "#!/usr/bin/env sh\n# @aios.schemaVersion 1\n# @aios.title Custom Tool\n# @aios.icon {}\necho ok\n",
            abs_png.display()
        ),
    )
    .expect("write abs script");
    fs::write(
        scripts_dir.join("xdg-icon.sh"),
        "#!/usr/bin/env sh\n# @aios.schemaVersion 1\n# @aios.title Storage Check\n# @aios.icon drive-harddisk\necho ok\n",
    )
    .expect("write xdg script");
    fs::write(
        scripts_dir.join("badge-only.sh"),
        "#!/usr/bin/env sh\n# @aios.schemaVersion 1\n# @aios.title Badge Script\n# @aios.icon DF\necho ok\n",
    )
    .expect("write badge script");

    let xdg_target = xdg_svg.clone();
    let host = ScriptHost::with_icon_resolver(paths, move |name| {
        if name == "drive-harddisk" {
            Some(xdg_target.clone())
        } else {
            None
        }
    });

    let entries = host.root_search_entries();
    let by_id = |id: &str| {
        entries
            .iter()
            .find(|e| e.id == id)
            .unwrap_or_else(|| panic!("missing {id}"))
    };

    let rel_entry = by_id("script:rel-icon");
    assert_eq!(
        rel_entry.icon_path.as_deref(),
        Some(rel_svg.canonicalize().unwrap().to_str().unwrap())
    );
    assert_eq!(rel_entry.badge, "DU");

    let abs_entry = by_id("script:abs-icon");
    assert_eq!(
        abs_entry.icon_path.as_deref(),
        Some(abs_png.canonicalize().unwrap().to_str().unwrap())
    );
    assert_eq!(abs_entry.badge, "CT");

    let xdg_entry = by_id("script:xdg-icon");
    assert_eq!(xdg_entry.icon_path.as_deref(), xdg_svg.to_str());
    assert_eq!(xdg_entry.badge, "SC");

    let badge_entry = by_id("script:badge-only");
    assert_eq!(badge_entry.icon_path, None);
    assert_eq!(badge_entry.badge, "DF");

    // Verify Script Commands Manager view also carries resolved `icon_path`.
    let ViewNode::List(list) = host.build_manager_view() else {
        panic!("expected ListView for script manager");
    };
    let manager_items: Vec<_> = list.sections.iter().flat_map(|s| &s.items).collect();
    let mgr_xdg = manager_items
        .iter()
        .find(|i| i.id == "mgr-script:script:xdg-icon")
        .expect("manager xdg item");
    assert_eq!(mgr_xdg.icon_path.as_deref(), xdg_svg.to_str());
}
