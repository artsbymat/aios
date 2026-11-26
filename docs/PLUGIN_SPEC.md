# AIOS Multi-View Hybrid Plugin Specification

AIOS achieves Raycast-level extensibility in Pure Rust through a **three-tier Hybrid Plugin Architecture** where every tier renders into the exact same native GPUI Multi-View UI system.

---

## 1. Three Plugin Tiers

| Tier                                | Runtime                                                                           | Best Use Cases                                                                                                                                                     | Isolation                                                          |
| ----------------------------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------ |
| **Tier 1: Built-in Core**           | Compiled Rust crates (`aios-builtin`)                                             | Strictly minimal core: Root Search Orchestrator and XDG Application Launcher                                                                                       | In-process (trusted core)                                          |
| **Tier 2: External WASM Plugins**   | `wasmtime` + WebAssembly Component Model (`wit/aios-plugin.wit`, `wasm32-wasip2`) | First-party & community multi-view extensions (Calculator, Quicklinks, Hyprland/Niri Window Management, System Commands, Clipboard, Files, Notes, future AI Chat) | Full memory sandbox, explicit capability gates, fuel/epoch timeout |
| **Tier 3: Script Commands**         | Standalone `aios-scripts` crate (local executables / shell / Python / Rust scripts with metadata headers) | Fast personal automation without compiling WASM                                                                                                                    | Subprocess timeout, explicit confirmation option                   |

---

## 2. Declarative Multi-View Model (Shared Across Tiers)

A plugin never manipulates raw pixels or GPUI widgets directly. Instead, it returns a typed **View AST** (`ViewNode`) and handles typed **UI Events** (`PluginEvent`). This guarantees visual consistency with [DESIGN.md](../DESIGN.md), full Dark/Light/System theme support, and 100% keyboard accessibility across all plugins.

### Command Modes

1. **`view`**: Opens an interactive view pushed onto the AIOS navigation stack (`List`, `Grid`, `Detail`, `Form`).
2. **`no-view`**: Executes headless logic and returns `Toast`, `Hud`, or silent completion.
3. **`menu-bar`**: Runs on a background refresh interval and publishes a compact status item (title, icon, tooltip, hierarchical menu) to the Wayland StatusNotifierItem / Waybar IPC bridge.

### View Primitives

#### A. `ListView`

```text
ListView {
  navigation_title: Option<String>,
  search_placeholder: Option<String>,
  filtering: FilterMode,            // Host (aios-fuzzy) | Plugin (on_search_change)
  is_loading: bool,
  is_showing_detail: bool,          // Enables 40/60 split Markdown+Metadata pane
  dropdown: Option<SearchDropdown>, // Filter selector in header right slot
  sections: Vec<ListSection>,
  empty_view: Option<EmptyView>,
  pagination: Option<PaginationState>,
}

ListItem {
  id: String,
  title: String,
  subtitle: Option<String>,
  icon: Option<IconSource>,
  keywords: Vec<String>,            // Extra fuzzy search terms
  accessories: Vec<Accessory>,      // Text | Tag(color, icon) | Date | Icon
  detail: Option<ItemDetail>,       // Markdown + Metadata for split pane
  actions: ActionPanel,
}
```

#### B. `GridView`

```text
GridView {
  navigation_title: Option<String>,
  search_placeholder: Option<String>,
  filtering: FilterMode,
  is_loading: bool,
  columns: u8,                      // 1..=8
  aspect_ratio: AspectRatio,        // Square | Ratio3x2 | Ratio2x3 | Ratio4x3 | Ratio16x9
  fit: ImageFit,                    // Contain | Fill
  inset: GridInset,                 // None | Small | Medium | Large
  dropdown: Option<SearchDropdown>,
  sections: Vec<GridSection>,
  empty_view: Option<EmptyView>,
}

GridItem {
  id: String,
  content: GridContent,             // Icon | ImagePath | ColorSwatch(hex)
  title: Option<String>,
  subtitle: Option<String>,
  keywords: Vec<String>,
  actions: ActionPanel,
}
```

#### C. `DetailView`

```text
DetailView {
  navigation_title: Option<String>,
  is_loading: bool,
  markdown: String,
  metadata: Option<Vec<MetadataItem>>, // Label | Link | TagList | Separator
  actions: ActionPanel,
}
```

#### D. `FormView`

```text
FormView {
  navigation_title: Option<String>,
  is_loading: bool,
  enable_drafts: bool,
  fields: Vec<FormField>,
  actions: ActionPanel,             // Must include Action::SubmitForm
}

enum FormField {
  TextField { id, label, placeholder, value, error, info },
  PasswordField { id, label, placeholder, value, error },
  TextArea { id, label, placeholder, value, error },
  Checkbox { id, label, title, checked, error },
  Dropdown { id, label, value, sections, error },
  DatePicker { id, label, value, kind /* Date | DateTime */, error },
  FilePicker { id, label, paths, allow_multiple, can_choose_dirs, error },
  TagPicker { id, label, selected, options, error },
  Separator,
  Description { title, text },
}
```

#### E. Universal `ActionPanel` & Actions

Every `ListItem`, `GridItem`, `DetailView`, `FormView`, and `EmptyView` attaches an `ActionPanel`:

- Organized into ordered `ActionSection` groups and nested `ActionSubmenu` items.
- The first action is automatically the **Primary Action** (`Enter`); the second is the **Secondary Action** (`Ctrl+Enter`).
- Built-in Host-Handled Actions (execute directly without extra plugin round-trip):
  - `OpenUrl(url)`
  - `OpenPath { path, application }`
  - `ShowInFileManager(path)`
  - `CopyToClipboard { content, concealed }`
  - `PasteText(text)`
  - `Trash(paths)`
  - `PopView` / `PopToRoot`
  - `CreateQuicklink { name, template }`
- Plugin-Handled Actions (emit `PluginEvent::ActionTriggered { action_id, item_id, form_values }` back to the plugin):
  - `Callback { id, title, icon, style /* Default | Destructive */, shortcut, confirm_alert }`
  - `PushView { id, title, icon, shortcut }`
  - `SubmitForm { id, title, shortcut }`

---

## 3. WebAssembly Component Model Contract (`wit/aios-plugin.wit`)

External Tier-2 plugins compile to `wasm32-wasip2` components implementing the `aios:plugin` world.

```wit
package aios:plugin@0.1.0;

interface host-api {
    record toast {
        style: toast-style, // animated | success | failure
        title: string,
        message: option<string>,
    }

    enum toast-style { animated, success, failure }

    // Feedback & Window Control
    show-toast: func(t: toast);
    show-hud: func(title: string);
    close-main-window: func();
    pop-to-root: func();

    // Persistence (scoped per plugin_id by host)
    storage-get: func(key: string) -> option<list<u8>>;
    storage-set: func(key: string, val: list<u8>) -> result<_, string>;
    storage-remove: func(key: string) -> result<_, string>;
    cache-get: func(key: string) -> option<list<u8>>;
    cache-set: func(key: string, val: list<u8>, ttl-seconds: u64);

    // Preferences & Environment
    get-preference: func(name: string) -> option<string>;
    get-theme-mode: func() -> string; // "dark" | "light"

    // Capability-Gated Host Calls (checked against aios-plugin.toml + user grants)
    clipboard-read-text: func() -> result<option<string>, string>;
    clipboard-write-text: func(text: string) -> result<_, string>;
    secret-get: func(key: string) -> result<option<string>, string>;
    secret-set: func(key: string, val: string) -> result<_, string>;
    http-request: func(req: http-request-spec) -> result<http-response, string>;
}

interface lifecycle {
    // Invoked when a command starts
    init-command: func(command-id: string, args-json: string) -> result<string, string>;
    // Invoked on user interaction (action click, search change, dropdown change, form submit, pagination)
    handle-event: func(event-json: string) -> result<string, string>;
    // Invoked on scheduled background refresh (menu-bar / cache warming)
    background-refresh: func(command-id: string) -> result<string, string>;
    // Cleanup when view pops or plugin unloads
    dispose: func();
}

world aios-extension {
    import host-api;
    export lifecycle;
}
```

_(Note: Passing view trees and events as typed WIT records or compact MessagePack/JSON payloads over WIT keeps ABI evolution clean across plugin versions while enforcing strict host-side schema validation before rendering in GPUI.)_

---

## 4. Plugin Manifest (`aios-plugin.toml`)

Every external WASM plugin package ships an `aios-plugin.toml` alongside its compiled `plugin.wasm` and optional static icon assets:

```toml
[plugin]
id = "com.example.docker-manager"
name = "Docker Manager"
version = "0.1.0"
description = "Inspect and control local containers and images."
author = "example"
min_aios_version = "0.1.0"
entry = "plugin.wasm"
icon = "assets/icon.svg"

[capabilities]
# Host denies any capability not declared here and approved by the user
clipboard_write = true
http_hosts = ["unix:///var/run/docker.sock"]
fs_read = []
secrets = false

[[preferences]]
name = "socket_path"
title = "Docker Socket Path"
kind = "textfield"
default = "/var/run/docker.sock"
required = false

[[commands]]
id = "containers"
title = "Manage Containers"
subtitle = "Docker"
mode = "view"
default_view = "list"

[[commands]]
id = "prune-stopped"
title = "Prune Stopped Containers"
subtitle = "Docker"
mode = "no-view"
needs_confirmation = true
```

---

## 5. Sandbox, Capability Security & Crash Isolation

1. **Zero Ambient Authority**: WASM components receive no raw filesystem, socket, or environment access by default. Every external resource call flows through `host-api`, verified against both `aios-plugin.toml` declarations and user approval in `plugin_permissions`.
2. **CPU & Memory Guardrails**:
   - Wasmtime linear memory cap per plugin instance (default `64MB`, configurable).
   - Epoch interruption / fuel timeout (`5s` per synchronous UI event; configurable up to `30s` for explicit async background tasks).
3. **Fault Isolation**: A WASM trap, invalid view schema, or timeout never crashes AIOS. The host catches the error, logs diagnostics, and renders the standard `ErrorView` with `Retry` and `Open Plugin Logs` actions.
4. **Live Reload Dev Workflow**: `aios plugin dev ./my-plugin` watches `plugin.wasm` and `aios-plugin.toml`, recompiling the Wasmtime module and hot-swapping the active view state cleanly.

---

## 6. Script Commands (Standalone `aios-scripts` Runtime)

Script Commands run through their own dedicated crate (`aios-scripts` / `ScriptHost`), separate from the Wasmtime `aios-plugin-host` runtime, with their own Root Search section (`Script Commands`), filter option (`scripts`), manager view (`aios://scripts`), and CLI (`aios script list`, `aios script run <id> [args...]`, `aios script dir`).

Users can drop executable scripts into `~/.config/aios/scripts/` with structured metadata comments at the top of the file:

```bash
#!/usr/bin/env bash
# @aios.schemaVersion 1
# @aios.title Clear Cargo Target Cache
# @aios.mode fullOutput
# @aios.packageName Developer Tools
# @aios.icon drive-harddisk
# @aios.needsConfirmation true
# @aios.argument1 { "type": "text", "placeholder": "Project path", "optional": true }
```

`# @aios.icon` accepts:

- A local image file path (`.svg`, `.png`, `.jpg`, `.jpeg`, `.webp`): absolute, `~/...`, or relative to the script's directory (for example `./icons/disk.svg`).
- A system XDG icon theme name (for example `drive-harddisk`, `utilities-terminal`), resolved through the shared `IconResolver`.
- A short text monogram/glyph badge (for example `DF` or `SH`), used directly as a 24x24px badge fallback.

Supported Script Command modes:

- `silent`: Closes the main launcher immediately and shows an `HUD` pill with the final stdout line when stdout is non-empty. If the script writes no stdout, the launcher closes silently with no HUD overlay (ideal for screenshot or GUI workflows).
- `compact`: Runs in background, shows live `Toast` with stdout status.
- `fullOutput`: Renders stdout/stderr inside AIOS's native `DetailView` code block with `Copy Output` and in-place `Re-run Script` actions (preserving the last arguments). When `# @aios.refreshTime <N>s` (e.g. `2s`, `1m`, minimum `1s`) is declared, AIOS automatically re-runs the script off the UI thread at that interval without overlapping runs, updates the active `DetailView` in place, exposes a `Pause / Resume Auto-Refresh` action, auto-pauses after `3` consecutive failures, and stops immediately when the user leaves the view or closes the launcher.
- `inline`: Runs on host startup, reload, or manual execution and displays a single summary line directly in Root Search.

For ad-hoc shell pipelines that prefer a classic `dmenu`/`rofi -dmenu` prompt instead of separate Script Command files, AIOS also provides `aios dmenu` (`-p/--prompt`, `-l/--lines`, `--icon <ICON>`, `--show-icons`, `-i/--insensitive`, `-s/--sensitive`, `--case-mode <smart|insensitive|sensitive>`, `--allow-custom`, `--index`, `--theme`, `--standalone`; see `aios dmenu --help`), which routes through the running `aios daemon` over Unix socket IPC with automatic standalone fallback and supports rofi-style `Label\0icon\x1f<icon>` lines on stdin.

---

## 7. Future Extensibility Proof: Optional AI Chat Plugin

Because AI Chat is intentionally excluded from AIOS core (100% local-first v1), the plugin contract above already covers every primitive required to ship AI Chat as an external WASM plugin later without modifying core:

- **View**: `List + Split Detail` for conversation threads + `DetailView` Markdown rendering with code block copy actions, or `FormView` for custom prompt templates.
- **Streaming**: Plugin emits incremental `ViewPatch` events while reading chunked responses from `host-api::http-request` (targeting local Ollama `http://127.0.0.1:11434` or remote LLM endpoints allowed in `capabilities.http_hosts`).
- **Keys & State**: API keys stored securely via `secret-get`/`secret-set`; conversation history stored in `storage-set`.
- **Context**: Reads clipboard or active selection via capability-gated `clipboard-read-text`.

---

## 8. External Plugin Ecosystem, Bundled Defaults & Zero-Cost Plugin Store

AIOS ships Phase 5 productivity plugins as **external WebAssembly Component Model (`wasm32-wasip2`) bundles** using a **Two-Layer Discovery** model:

1. **Bundled Defaults Layer**: Shipped default bundles (such as `dist/plugins/com.aios.calculator/` or `/usr/share/aios/plugins/`) are loaded automatically out-of-the-box and can be cleanly enabled or disabled in SQLite (`plugin_state`) without deleting system files.
2. **User Installed / Upgraded Layer**: Plugins installed into `$XDG_DATA_HOME/aios/plugins/<id>/` always shadow bundled plugins of the same ID, allowing users to upgrade official plugins independently from the Plugin Store.
3. **Declarative Inline Fast-Gate (`[inline_trigger]`)**: External WASM plugins that provide live Root Search previews declare `[inline_trigger]` (`prefixes`, `match_numeric_expressions`, `leading_chars`, `leading_keywords`) in `aios-plugin.toml`. The host evaluates the fast-gate in `< 50 ns` so normal app queries never invoke Wasmtime.
4. **Developer Experience (`aios-plugin-sdk` + `aios plugin ...` CLI)**:
   - Enable `aios-plugin-sdk = { ..., features = ["guest"] }`, implement `AiosPlugin`, and invoke `aios_plugin_sdk::export_plugin!(MyPlugin);`.
   - CLI workflow: `aios plugin new <name>`, `aios plugin build .`, `aios plugin dev .`, `aios plugin pack .` (creates `.aios-plugin` `.tar.gz` + SHA-256 + registry TOML snippet).
5. **Zero-Cost GitHub-Native Registry & Plugin Store (`aios://store`)**:
   - Authors host `.aios-plugin` tarballs on **GitHub Releases** (automated by the scaffolded `.github/workflows/release.yml`) and submit 1 TOML file to `registry/plugins/<id>.toml`.
   - GitHub Actions verifies schema + SHA-256 and compiles `registry/index.json` for GitHub Pages / jsDelivr CDN (`$0/month`).
   - Users browse, search, install, update, and toggle plugins directly in **Plugin Store (`aios://store`)** or via `aios plugin search / install / update / enable / disable` (also supports `aios plugin install github:<owner>/<repo>` and offline `.aios-plugin` archives).

First-party plugin documentation:

- [Calculator & Converter (`com.aios.calculator`)](plugins/calculator.md): External `wasm32-wasip2` bundled plugin (`plugins/com.aios.calculator/`) with hybrid zero-prefix + `=` exclusive trigger, math/scientific functions, programmer bases & bitwise ops, unit/volume/CSS converter, date/timezone math, cached live currency converter, SQLite history, and configurable preferences.
