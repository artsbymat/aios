# AIOS Technical Architecture

This document defines the runtime architecture, process topology, GPUI Wayland rendering model, Hyprland/Niri integration, and local data layer for **AIOS (All in One Search)**.

---

## 1. High-Level System Architecture

AIOS uses a **persistent lightweight daemon + instant GPUI surface** model so invoking the launcher takes milliseconds rather than cold-starting the application, SQLite, icon caches, and plugin registry on every shortcut press.

```text
+-----------------------+          Unix Socket ($XDG_RUNTIME_DIR/aios.sock)
| Compositor Keybind /  |  ------> `aios toggle` / `aios open aios://...`
| Shell Pipeline        |          `echo ... | aios dmenu -p "Prompt"`
| (Hyprland / Niri)     |                       |
+-----------------------+                       v
                                   +-----------------------------------+
                                   |           AIOS Host Process       |
                                   |                                   |
                                   |  +-----------------------------+  |
                                   |  | GPUI Wayland Layer-Shell UI |  |
                                   |  | (List / Grid / Detail / Form|  |
                                   |  |  + ActionPanel + HUD + Toast|  |
                                   |  +--------------+--------------+  |
                                   |                 |                 |
                                   |  +--------------+--------------+  |
                                   |  |   Core Navigation & State   |  |
                                   |  |  Root Search | Frecency     |  |
                                   |  |  aios-fuzzy  | View Router  |  |
                                   |  +------+---------------+------+  |
                                   |         |               |         |
                                   |  +------+-----+   +-----+------+  |
                                   |  |  Core App  |   | WASM Host  |  |
                                   |  |  Launcher  |   | (wasmtime) |  |
                                   |  |  & Scripts |   |  Plugins   |  |
                                   |  +------+-----+   +-----+------+  |
                                   |         |               |         |
                                   |  +------+---------------+------+  |
                                   |  |  Wayland & OS Platform Bus  |  |
                                   |  |  Hyprland IPC | Niri IPC    |  |
                                   |  |  Clipboard    | XDG Portal  |  |
                                   |  |  SQLite WAL   | Secret Svc  |  |
                                   |  +-----------------------------+  |
                                   +-----------------------------------+
```

### Single-Instance Process Flow & Hybrid `dmenu` Mode

1. `aios daemon` (started via `systemd --user` or compositor `exec-once` / `spawn-at-startup`) initializes the async runtime, SQLite WAL database, desktop app index, plugin host, XDG theme watcher, and GPUI application loop in hidden/standby state.
2. When the user presses their compositor keybind (for example `SUPER + Space`), the compositor runs `aios toggle` or `aios open "aios://search?q=...&filter=..."` (a tiny, fast CLI path that writes a 1-frame command to `$XDG_RUNTIME_DIR/aios.sock` and exits immediately).
3. For shell scripts wanting `dmenu`/`rofi -dmenu` semantics (`echo -e "Full\nArea" | aios dmenu -p "Screenshot" -l 7`), `aios dmenu` sends a `Dmenu` request over `$XDG_RUNTIME_DIR/aios.sock` so the warm daemon opens a compact GPUI LayerShell picker immediately without blocking the IPC accept loop. If no daemon is running, `aios dmenu` falls back seamlessly to a standalone GPUI window. All structured logs go to `stderr` so `stdout` carries only the chosen item (exit code `0`) or remains empty on cancel (exit code `1`).
4. The running GPUI host opens or reveals the `WindowKind::LayerShell` surface on the currently active monitor with `KeyboardInteractivity::Exclusive`.
5. Pressing `Esc`, executing an action that closes the window, or cancelling `dmenu` hides/destroys the layer surface while keeping indexes and plugin warm caches in RAM.

---

## 2. GPUI & Wayland Layer-Shell Rendering Model

### Layer-Shell Surfaces

AIOS creates three distinct GPUI surface types on Wayland (`wlr-layer-shell-unstable-v1`):

| Surface        | GPUI `WindowKind`                                | Layer           | Keyboard Interactivity | Purpose                                                                |
| -------------- | ------------------------------------------------ | --------------- | ---------------------- | ---------------------------------------------------------------------- |
| Main Launcher  | `LayerShell` (`namespace: "aios"`)               | `Overlay`       | `Exclusive`            | Root Search, Plugin Views, Action Panel, Settings                      |
| Dmenu Picker   | `LayerShell` (`namespace: "aios"`)               | `Overlay`       | `Exclusive`            | Compact `stdin -> stdout` fuzzy picker (`aios dmenu`, daemon/standalone) |
| HUD Pill       | `LayerShell` (`namespace: "aios-hud"`)           | `Overlay`       | `None`                 | Non-intrusive bottom-center confirmation overlay after launcher closes |
| Floating Notes | Standard `XDG TopLevel` (`app_id: "aios-notes"`) | Normal floating | Normal                 | Persistent scratchpad window managed by Hyprland/Niri floating rules   |

### Theme Resolution (Dark, Light, System)

- User config stores `theme.mode = "system" | "dark" | "light"`.
- When set to `"system"`, `aios-wayland` queries `org.freedesktop.portal.Settings` on session D-Bus (`Read("org.freedesktop.appearance", "color-scheme")`) and subscribes to `SettingChanged`:
  - `1` -> Dark theme (`DESIGN.md` dark tokens)
  - `2` or `0` -> Light theme (`DESIGN.md` light tokens)
- Changing the system color scheme updates GPUI's global `AiosTheme` state immediately without restarting the daemon.

### View Reconciliation Pipeline

To keep GPUI rendering decoupled from plugin internals:

1. Both **Built-in Native Plugins** and **External WASM Plugins** produce the same pure-data declarative `ViewNode` tree (`List`, `Grid`, `Detail`, `Form`, `Empty`, `Error`).
2. `aios-ui` holds one GPUI `WorkspaceView` per open navigation frame in `Vec<NavFrame>`.
3. When a plugin emits an updated `ViewNode` (or incremental patch), `aios-ui` reconciles selection index, scroll offset, and search input state cleanly without losing focus.
4. For `List` and `Grid` views with `filtering = FilterMode::Host`, keystrokes never cross the WASM boundary: `aios-fuzzy` filters and highlights items synchronously/concurrently on the host at frame rate.

---

## 3. Target Wayland Compositor Adapters (`aios-wayland`)

AIOS defines a clean compositor abstraction trait so Window Management, monitor targeting, and active-window context work natively on both **Hyprland** and **Niri**:

```rust
pub trait CompositorAdapter: Send + Sync {
    fn name(&self) -> &'static str;
    fn list_windows(&self) -> Result<Vec<WindowInfo>>;
    fn active_window(&self) -> Result<Option<WindowInfo>>;
    fn list_workspaces(&self) -> Result<Vec<WorkspaceInfo>>;
    fn list_monitors(&self) -> Result<Vec<MonitorInfo>>;
    fn execute(&self, action: WindowAction) -> Result<()>;
    fn subscribe_events(&self) -> Result<EventReceiver>;
}
```

### Hyprland Adapter

- **Detection**: `$HYPRLAND_INSTANCE_SIGNATURE` environment variable present.
- **Sockets**:
  - Command socket: `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock` (requests prefixed with `j/` for JSON responses).
  - Event socket: `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock` (streamed window focus/workspace events for clipboard app-exclusion and active context).
- **Supported Window Actions**: `FocusWindow`, `CloseWindow`, `ToggleFloating`, `ToggleFullscreen`, `MoveToWorkspace`, `SwitchWorkspace`, `MoveToMonitor`, `SnapHalfLeft/Right/Top/Bottom`, `CenterWindow`, `Maximize`.
- **Recommended User Config**:
  ```ini
  exec-once = aios daemon
  bind = SUPER, Space, exec, aios toggle
  bind = , Print, exec, aios open "aios://search?q=screenshot&filter=commands"
  layerrule = blur, aios
  layerrule = ignorezero, aios
  windowrulev2 = float, class:^(aios-notes)$
  ```

### Niri Adapter

- **Detection**: `$NIRI_SOCKET` environment variable present.
- **Socket & Protocol**: Structured newline-delimited JSON over `$NIRI_SOCKET` using `niri-ipc` request/response/event structs.
- **Supported Window & Scrollable-Tiling Actions**: `FocusWindow`, `CloseWindow`, `MaximizeColumn`, `CenterColumn`, `ConsumeOrExpelWindowLeft/Right`, `MoveColumnLeft/Right`, `SwitchPresetColumnWidth`, `ToggleWindowFloating`, `ToggleFullscreen`, `MoveWindowToWorkspace`, `SwitchWorkspace`, `FocusMonitor`, `ToggleOverview`.
- **Recommended User Config**:
  ```kdl
  binds {
      Mod+Space { spawn "aios" "toggle"; }
  }
  layer-rule {
      match namespace="^aios$"
      shadow { on }
  }
  window-rule {
      match app-id="^aios-notes$"
      open-floating true
  }
  ```

### Fallback Generic Wayland Support

If launched on another `wlr-layer-shell` compositor, the launcher UI still works via standard Wayland protocols while compositor-specific Window Management commands degrade gracefully with an explicit status message explaining which compositor IPC was not detected.

---

## 4. Wayland Platform Subsystems

- **Clipboard Daemon**: Pure Rust Wayland clipboard backend inspired by `Rolv-Apneseth/clipvault`'s storage, privacy, and housekeeping architecture (while replacing external `wl-paste --watch` with a native Wayland client). Connects directly to Wayland display and binds `ext_data_control_manager_v1` (falling back to `zwlr_data_control_manager_v1`). Preserves payloads byte-for-byte (including leading/trailing whitespace and raw binary/images), deduplicates via BLAKE3 content hash, inspects MIME types (`text/plain;charset=utf-8`, `text/html`, `image/png`, `text/uri-list`), enforces configurable `min_entry_length`, `max_entry_size`, `max_entries`, and `max_entry_age` housekeeping limits, applies layered privacy filtering (both Hyprland/Niri active window/app-id blacklists like `org.keepassxc.KeePassXC` / `bitwarden` and configurable regex ignore patterns for sensitive content), and stores binary blobs on disk with metadata in SQLite WAL.
- **Desktop Notifications**: D-Bus client for `org.freedesktop.Notifications` (used by Focus/Timer and background plugin alerts).
- **Secrets Storage**: D-Bus client for `org.freedesktop.secrets` (Secret Service API) for plugin password preferences and future OAuth tokens, scoped per plugin ID.
- **System Audio & Media**: PipeWire / PulseAudio control and MPRIS2 D-Bus (`org.mpris.MediaPlayer2.*`) for playback commands.
- **Menu Bar / Status Bridge**:
  - Exposes `StatusNotifierItem` over D-Bus for desktop trays.
  - Exposes `aios status --json` / socket subscription so Waybar, Eww, or custom bars can render live `MenuBar` plugin outputs (e.g., active timer, system status) cleanly.

---

## 5. Local-First Persistence & Directory Layout

All state lives strictly on the local machine following XDG specifications:

| Path                                | Contents                                                                                                                                                    |
| ----------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `$XDG_CONFIG_HOME/aios/config.toml` | User settings (theme mode, window density, aliases, hotkeys, fallback commands, clipboard ignore list)                                                      |
| `$XDG_CONFIG_HOME/aios/scripts/`    | User Script Commands directory                                                                                                                              |
| `$XDG_DATA_HOME/aios/aios.db`       | SQLite (WAL mode, FTS5 enabled): frecency table, clipboard metadata, snippets, quicklinks, notes, plugin KV `LocalStorage`, plugin enabled/permission state |
| `$XDG_DATA_HOME/aios/blobs/`        | Content-addressed binary blobs (clipboard images, plugin icons)                                                                                             |
| `$XDG_DATA_HOME/aios/plugins/`      | Installed external WASM plugin bundles (`<plugin-id>/aios-plugin.toml` + `plugin.wasm`)                                                                     |
| `$XDG_CACHE_HOME/aios/`             | Resolved XDG icon cache, currency rates cache, plugin `Cache` TTL store, compiled Wasmtime artifacts (`.cwasm`)                                             |
| `$XDG_RUNTIME_DIR/aios.sock`        | Unix domain socket for CLI-to-daemon IPC                                                                                                                    |

### SQLite Core Tables (Summary)

- `frecency(item_id, query_prefix, hits, last_used_at, pinned)`
- `clipboard_entries(id, content_hash, kind, mime_type, text_preview, blob_path, source_app, pinned, created_at)`
- `snippets(id, name, keyword, content, folder, use_count, updated_at)`
- `quicklinks(id, name, template_url, icon, open_with, use_count)`
- `notes(id, title, body_markdown, pinned, deleted_at, updated_at)`
- `plugin_storage(plugin_id, key, value_blob, updated_at)`
- `plugin_permissions(plugin_id, capability, granted, updated_at)`

---

## 6. Concurrency & Responsiveness Budget

- **UI Thread (GPUI main loop)**: Strictly non-blocking. Never performs disk I/O, network requests, subprocess execution, or WASM calls directly. Runs only the `aios-fuzzy` inline fast path within a strict `<= 1.5ms` budget and renders the latest `SearchSnapshot` independently at monitor refresh rate.
- **Async Runtime (Tokio multi-thread)**: Handles Unix socket IPC, D-Bus watchers, Hyprland/Niri event streams, HTTP requests (when granted to a plugin), and file walking.
- **Compute Pool (`rayon` / dedicated worker threads)**: Runs `aios-fuzzy` parallel chunked matching with progressive Top-K snapshot streaming, latest-wins query replacement, and atomic keystroke cancellation (modeled on `instantMENU`'s parallel search/filter/render separation), alongside Wasmtime plugin execution with fuel limits.
- **Performance Targets & Verified Measurements** (see [docs/BENCHMARKS.md](BENCHMARKS.md) for full release benchmark numbers and reproduction commands):
  - Unix socket IPC `Toggle` round-trip: `29.6 us` (target: toggle-to-first-frame `< 16ms`)
  - `aios-fuzzy` parallel search: `5.32 ms` (10k items), `13.06 ms` (100k items), `1.27 ms` avg UI-thread keystroke latency on 100k items via async `SearchSession`
  - Wasmtime cached WASM invocation: `0.14 ms` (`init-command`), `0.09 ms` (`handle-event`)
  - Daemon idle RSS memory target: `< 40MB`
