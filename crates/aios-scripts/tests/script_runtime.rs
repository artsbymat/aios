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
