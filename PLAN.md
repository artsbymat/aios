# AIOS (All in One Search) Master Plan

## Project Summary

AIOS is a Pure Rust, Wayland-native command launcher and multi-view plugin workbench inspired by Raycast's depth and extensibility.

Confirmed technical pillars:

- **Language**: Pure Rust (Cargo workspace)
- **UI Engine**: GPUI (`WindowKind::LayerShell` over `wlr-layer-shell-unstable-v1`)
- **Target Wayland Compositors**: Hyprland and Niri (first-class IPC adapters)
- **Fuzzy Search Engine**: Clean-room internal crate `aios-fuzzy` modeled on `skim`'s scoring and query engine architecture (no `skim` crate dependency)
- **Plugin Architecture**: Hybrid model combining Built-in Native Rust Plugins, Sandboxed External WASM Component Plugins (`wasmtime` + WIT), and Script Commands
- **Themes**: Dual-mode Dark, Light, and System adaptive (via XDG Desktop Portal), governed by [DESIGN.md](DESIGN.md)
- **Scope (v1)**: 100% local-first and offline-first. No built-in AI Chat and no cloud sync in core (AI Chat is designed to plug in cleanly later as an external plugin)

---

## Cargo Workspace Topology (Planned)

```text
aios/
  Cargo.toml                 # Workspace manifest
  DESIGN.md                  # FigCraft design system & tokens
  PLAN.md                    # This master plan
  AGENTS.md                  # Agent routing & guardrails
  docs/
    ARCHITECTURE.md          # System, GPUI, Wayland, persistence architecture
    PLUGIN_SPEC.md           # Hybrid WASM + Multi-View declarative UI spec
    FUZZY_SPEC.md            # Clean-room skim-inspired fuzzy engine spec
  wit/
    aios-plugin.wit          # WebAssembly Component Model contract
  crates/
    aios-fuzzy/              # Pure Rust fuzzy matcher, query parser, frecency ranker
    aios-core/               # Domain models, view AST, config, SQLite storage, actions
    aios-wayland/            # Layer-shell helpers, Hyprland & Niri IPC, clipboard, portal theme
    aios-plugin-sdk/         # Guest & native plugin ergonomic Rust API + view builders
    aios-plugins/            # First-party Pure Rust plugins suite (Phase 5, decoupled from core)
    aios-plugin-host/        # Wasmtime host runtime, capability sandbox, first-party & WASM registry, settings
    aios-scripts/            # Standalone Script Commands runner, discovery, inline cache & manager UI
    aios-builtin/            # Core built-in providers (Root Search orchestrator, Application Launcher)
    aios-ui/                 # GPUI theme system, reusable components, 6 view renderers
    aios-daemon/             # Background daemon (Unix socket IPC, clipboard watcher, refresh scheduler)
    aios-app/                # Main GPUI binary + CLI client (`aios toggle`, `aios open`, etc.)
```

---

## Progress Tracker

| Phase   | Milestone                                                                                  | Status          |
| ------- | ------------------------------------------------------------------------------------------ | --------------- |
| Phase 0 | Workspace Bootstrap & Foundation                                                           | `[x]` Completed |
| Design  | FigCraft Design System (`DESIGN.md`, `fig/aios.figcraft`) & Official Brand Kit (`assets/`) | `[x]` Completed |
| Phase 1 | Clean-Room Fuzzy Engine (`aios-fuzzy` + `instantMENU`-style parallel execution)            | `[x]` Completed |
| Phase 2 | Wayland Shell, GPUI Core UI & Dual-Mode Theme Engine                                       | `[x]` Completed |
| Phase 3 | Root Search Orchestrator & Application Launcher (Core MVP)                                 | `[x]` Completed |
| Phase 4    | Hybrid Plugin Runtime (WASM + Script Commands + SDK)                                       | `[x]` Completed |
| Phase 5.1  | Plugin: Inline Calculator & Converter (+ Root Search live inline hook)                     | `[x]` Completed |
| Phase 5.2  | Plugin: Quicklinks (+ Fallback Command integration)                                        | `[ ]` Pending   |
| Phase 5.3  | Plugin: Snippets (`List+Detail` + `Form` + placeholders + import/export)                   | `[ ]` Pending   |
| Phase 5.4  | Plugin: Emoji & Symbols Picker (`Grid` + recents + skin tone)                              | `[ ]` Pending   |
| Phase 5.5  | Plugin: Window Management (`Hyprland` & `Niri` IPC adapters + WIT capability)              | `[ ]` Pending   |
| Phase 5.6  | Plugin: System Commands (Power, Audio/PipeWire, Brightness, MPRIS, Theme)                  | `[ ]` Pending   |
| Phase 5.7  | Plugin: Clipboard History (Wayland `data-control` daemon + blob store + privacy rules)     | `[ ]` Pending   |
| Phase 5.8  | Plugin: File Search & Preview (High-speed walker + `aios-fuzzy` + preview pane)            | `[ ]` Pending   |
| Phase 5.9  | Plugin: Process Manager (`/proc` live list + `SIGTERM`/`SIGKILL` confirm)                  | `[ ]` Pending   |
| Phase 5.10 | Plugin: Color Picker & Converter (XDG Portal eyedropper + `Grid` + HEX/RGB/HSL/OKLCH)      | `[ ]` Pending   |
| Phase 5.11 | Plugin: Focus & Timer (Countdown / Pomodoro + desktop notifications)                       | `[ ]` Pending   |
| Phase 5.12 | Bridge: Menu Bar / Status Bar (SNI tray + Waybar `aios status --json` stream)              | `[ ]` Pending   |
| Phase 5.13 | Plugin: Floating Notes / Scratchpad (Separate GPUI `XDG TopLevel` window)                  | `[ ]` Pending   |
| Phase 6    | Polish, Packaging & Plugin Ecosystem Readiness                                             | `[ ]` Pending   |

---

## Phased Execution Roadmap

### Phase 0: Workspace Bootstrap & Foundation `[DONE]`

**Goal**: Establish the multi-crate Rust workspace, shared types, configuration/storage layer, and CI/test harness.

- [x] Initialize Cargo workspace and crate skeletons matching the planned topology.
- [x] Define XDG Base Directory paths (`~/.config/aios`, `~/.local/share/aios`, `~/.cache/aios`, `$XDG_RUNTIME_DIR/aios.sock`).
- [x] Implement `aios-core` configuration loader (`config.toml`), SQLite WAL database migrations (frecency, clipboard, snippets, quicklinks, notes, plugin KV store), and structured logging (`tracing`).
- [x] **Design & Brand System**: Finalize [DESIGN.md](DESIGN.md) token specification, FigCraft UI artboards (`fig/aios.figcraft`), and production Wayland/GPUI brand assets (`assets/`).
- [x] **Exit Criteria**: `cargo workspace` compiles cleanly; SQLite migrations and config round-trip unit tests pass.

### Phase 1: Clean-Room Fuzzy Engine (`aios-fuzzy`) `[DONE]`

**Goal**: Build a fast, zero-`skim`-dependency fuzzy search engine matching `skim`'s ergonomics and ranking accuracy.

- [x] Implement `SkimV2`-inspired Smith-Waterman DP scorer with character bonus tables and affine gap penalties (`gap_start`, `gap_extend`).
- [x] Implement `MatchResult` returning both numeric score and exact matched character indices for UI highlighting.
- [x] Implement query parser and composite match engines: space (`AND`), `|` (`OR`), `'` (exact), `^` (prefix), `$` (suffix), `!` (inverse), smart-case matching.
- [x] Implement frecency ranker (exponential time decay + hit count + query-to-item affinity + pin boost).
- [x] Add adaptive parallel execution engine (inspired by `instantMENU`'s parallel search/filter/render pipeline): `<= 1.5ms` inline fast path for small lists, automatic `rayon` background handoff for large corpora, progressive Top-K snapshot updates, latest-wins query replacement, atomic per-keystroke cancellation, and benchmarks (target: < 5ms for 10,000 items, < 25ms for 100,000 items).
- [x] **Exit Criteria**: Comprehensive unit test suite covering Unicode, camelCase, path boundaries, query operators, and benchmark suite passing (`106.5 ns` single item, `4.04 ms` 10k parallel, `13.06 ms` 100k parallel).

### Phase 2: Wayland Shell, GPUI Core UI & Dual-Mode Theme Engine `[DONE]`

**Goal**: Deliver an instant-toggle Wayland overlay window on Hyprland and Niri with full `DESIGN.md` token fidelity.

- [x] Implement `aios-wayland` single-instance daemon + Unix socket IPC (`aios toggle`, `aios show`, `aios hide`, `aios open <uri>` including `aios://search?q=...&filter=...`, and hybrid `aios dmenu` with automatic standalone fallback).
- [x] Implement XDG Desktop Portal `color-scheme` listener for live Dark / Light / System switching.
- [x] Implement GPUI `WindowKind::LayerShell` overlay (`Layer::Overlay`, `KeyboardInteractivity::Exclusive`, namespace `"aios"`), centered with configurable top offset and monitor-aware focus.
- [x] Implement `aios-ui` theme tokens directly from [DESIGN.md](DESIGN.md) and core shell chrome:
  - [x] Header Search Bar + Inline Argument pills + Filter Dropdown + 2px loading bar
  - [x] Navigation Stack (`Push`, `Pop`, `PopToRoot`)
  - [x] Universal Footer Bar + Searchable `ActionPanel` popover (`Ctrl+K`)
  - [x] Feedback overlays: `Toast`, `HUD` (separate layer-shell overlay pill), `ConfirmAlert` modal
  - [x] Mandatory Empty, Loading, and Error view components
- [x] Implement **all 4 interactive declarative View renderers** in `aios-ui`:
  - [x] `List` (+ split `List.Item.Detail` Markdown/Metadata pane)
  - [x] `Grid` (multi-column aspect-locked tiles)
  - [x] `Detail` (Markdown + syntax highlighting + Metadata sidebar)
  - [x] `Form` (all field types + validation + keyboard focus order)
- [x] **Exit Criteria**: Launcher toggles cleanly via Hyprland and Niri keybinds, switches live between Dark and Light modes, and renders demo fixtures for all 4 views + ActionPanel using keyboard only (`30/30` workspace tests + WCAG AA dark/light assertions passing).

### Phase 3: Root Search Orchestrator & Application Launcher (Core MVP) `[DONE]`

**Goal**: Make AIOS a fast, lean, daily-driver Wayland application launcher backed by `aios-fuzzy` and SQLite frecency, keeping core strictly minimal while all additional productivity tools live as external plugins.

- [x] **Root Search Orchestrator**: Unified `aios-fuzzy` search across applications, core commands, aliases, pinned items, SQLite frecency/query-prefix affinity, and configurable Fallback Commands.
- [x] **Application Launcher**: XDG `.desktop` scanner (native, Flatpak, AppImage), XDG icon theme resolver, detached process launcher, and actions (Launch, Launch in Terminal, Copy Exec Command, Copy `.desktop` Path, Show `.desktop` Detail, Pin/Unpin, Hide App).
- [x] **Exit Criteria**: AIOS replaces rofi/wofi/fuzzel for instant application launching with live frecency ranking, icons, aliases, and full keyboard-only navigation on Hyprland and Niri (`38/38` workspace tests, zero clippy warnings, `81/81` XDG desktop icons resolved via `aios doctor`).

*Architectural Scope Note*: Inline Calculator & Converter, Quicklinks, Window Management (Hyprland & Niri), and System Commands are intentionally decoupled from core and implemented as **External Plugins** on top of the Phase 4 Hybrid Plugin Runtime (tracked in Phase 5).

### Phase 4: Hybrid Plugin Runtime (WASM + Script Commands + SDK) `[DONE]`

**Goal**: Enable third-party and user-authored multi-view plugins safely in Pure Rust.

- [x] Finalize `wit/aios-plugin.wit` interface covering all 6 command modes (`List`, `Grid`, `Detail`, `Form`, `NoView`, `MenuBar`), actions, navigation, preferences, `LocalStorage`, `Cache`, clipboard, HTTP/FS capabilities, and `Toast`/`HUD`/`Alert`.
- [x] Implement `aios-plugin-host` using `wasmtime` with WebAssembly Component Model:
  - [x] Capability permission checker (`aios-plugin.toml`)
  - [x] Epoch/fuel interruption timeouts so misbehaving plugins never freeze the UI
  - [x] Crash isolation (plugin panic renders clean ErrorView with logs + reload action)
  - [x] Hot-reload watcher for local plugin development (`aios plugin dev <path>`)
- [x] Implement `aios-plugin-sdk` crate so plugin authors write idiomatic Rust declarative views compiled to `wasm32-wasip2`.
- [x] Implement **Script Commands** runner (metadata header parser, `compact`/`fullOutput`/`inline`/`silent` modes, arguments, confirmation prompts).
- [x] Implement **Settings & Extension Manager View** (built using AIOS's own multi-view UI): manage themes, aliases, hotkeys, fallback commands, plugin preferences, capability approvals, and local plugin install/uninstall.
- [x] **Exit Criteria**: Reference external WASM plugin (`com.aios.palette-grid` `Grid+Form` with `icon.svg`), isolated List+Detail + capability-gate WASM fixtures, Script Commands (`system-uptime.sh`, `disk-usage.sh`), and hybrid `aios dmenu` (`stdin -> stdout` via daemon IPC + standalone fallback) run end-to-end inside AIOS with full isolation.

### Phase 5: First-Party External Plugins Suite (1 Sub-Phase per Plugin) `[PENDING]`

**Goal**: Deliver Raycast's productivity suite as modular external plugins running on the Phase 4 Hybrid Plugin Runtime (WASM + host Wayland capabilities), split into 1 independent milestone per plugin so each plugin can be manually checked in Dark & Light themes before moving on.

#### Phase 5.1: Inline Calculator & Converter Plugin `[DONE]`
- [x] Add lightweight dynamic inline query hook in Root Search (`aios-builtin`) + `PluginEvent::SearchChanged` forwarding in `LauncherWindow` so math/unit expressions preview live without opening a separate view first (hybrid auto-detect + optional `=` exclusive prefix with sub-microsecond fast-gate).
- [x] Build first-party plugin (`com.aios.calculator` in `crates/aios-plugins`): live math expressions, hex/bin/octal & bitwise, units (length, weight, temp, data SI/IEC, time, speed, area, CSS `px/rem/em/pt`), date/timezone math, cached offline currency converter, configurable preferences (`base_font_px`, `decimal_places`, `default_currency`), and SQLite calculation history.
- [x] **Manual Check & Exit Criteria**: Typing `24 * 12`, `=pi`, `0xff to dec`, `100 cm to in`, `24 px to rem`, `100 usd to idr`, `today + 14d`, `now in tokyo`, or opening Calculator works offline with instant copy (`Enter`), `Ctrl+Enter` drill-down, complete Empty/Error states, and Dark/Light parity.

#### Phase 5.2: Quicklinks Plugin `[PENDING]`
- [ ] Build external WASM plugin (`com.aios.quicklinks`): custom URL/path templates with `{query}` / `{argument}` placeholders, `List` + `Form` editor (create/edit/delete), and Fallback Command integration in Root Search.
- [ ] **Manual Check & Exit Criteria**: Creating a quicklink via `Form`, launching with inline arguments, and triggering from Root Search Fallback Commands open the expected URL/path cleanly.

#### Phase 5.3: Snippets Plugin `[PENDING]`
- [ ] Build external WASM plugin (`com.aios.snippets`): snippet manager (`List+Detail` + `Form` editor), dynamic placeholders (`{clipboard}`, `{date}`, `{time}`, `{cursor}`, `{argument}`), instant copy/paste from launcher, and JSON/TOML import/export.
- [ ] **Manual Check & Exit Criteria**: Creating, searching, expanding dynamic placeholders, copying to clipboard, and importing/exporting snippets work cleanly in Dark and Light themes.

#### Phase 5.4: Emoji & Symbols Picker Plugin `[PENDING]`
- [ ] Build external WASM plugin (`com.aios.emoji`): `Grid` view for Unicode emoji, math/currency symbols, and Nerd Font glyphs with category filter, recent/frequent tracking in `LocalStorage`, and skin tone preference.
- [ ] **Manual Check & Exit Criteria**: Searching and navigating the `Grid` keyboard-only, changing skin tone preference, and copying an emoji/glyph update recents and close with HUD feedback.

#### Phase 5.5: Window Management Plugin (Hyprland & Niri) `[PENDING]`
- [ ] Implement `CompositorAdapter` in `aios-wayland` for **Hyprland** (`.socket.sock` + `.socket2.sock`) and **Niri** (`$NIRI_SOCKET` via `niri-ipc`) with graceful fallback on other Wayland compositors.
- [ ] Expose capability-gated `compositor` host API in `wit/aios-plugin.wit` and `aios-plugin-host`.
- [ ] Build external WASM plugin (`com.aios.window-management`): focus window, move/resize/snap, fullscreen, toggle floating, switch/move workspace, move monitor (plus Niri column consume/expel, maximize/center column, overview toggle).
- [ ] **Manual Check & Exit Criteria**: Listing open windows, focusing, snapping, and moving workspaces/columns work natively on Hyprland and Niri with keyboard-only controls.

#### Phase 5.6: System Commands Plugin `[PENDING]`
- [ ] Expose capability-gated system/media host APIs (PipeWire/PulseAudio volume & sink switcher, brightness, MPRIS2 D-Bus media controls, session power actions, XDG Trash).
- [ ] Build external WASM plugin (`com.aios.system-commands`): Lock, Suspend, Reboot, Shutdown, Logout (with `ConfirmAlert`), Empty Trash, volume/sink switcher, brightness, MPRIS media controls, and Dark/Light theme toggle.
- [ ] **Manual Check & Exit Criteria**: Destructive power/trash actions require `ConfirmAlert`, audio/brightness/MPRIS commands execute cleanly, and missing optional services show crisp `ErrorView`/Toast feedback.

#### Phase 5.7: Clipboard History Plugin `[PENDING]`
- [ ] Implement Pure Rust Wayland clipboard daemon (`ext-data-control-v1` / `zwlr-data-control-v1`) in `aios-wayland` modeled on `Rolv-Apneseth/clipvault`: byte-for-byte preservation, BLAKE3 deduplication, content-addressed blob storage (`~/.local/share/aios/blobs/`), min/max entry size & age housekeeping, regex ignore patterns, and sensitive-app active-window blacklist (using `CompositorAdapter` from Phase 5.5).
- [ ] Build external WASM plugin (`com.aios.clipboard`): `List+Detail` split preview for text, code, links, images, files, and color swatches, with pin/delete/clear and copy/paste actions.
- [ ] **Manual Check & Exit Criteria**: Text and image copies are captured without leaking memory, sensitive-app copies are excluded, and searching/pinning/pasting works cleanly in Dark and Light themes.

#### Phase 5.8: File Search & Preview Plugin `[PENDING]`
- [ ] Expose capability-gated high-speed file walker (`fs-walk` / `fs-list-dir` / `fs-open` / `fs-trash`) backed by `aios-fuzzy` parallel filtering.
- [ ] Build external WASM plugin (`com.aios.file-search`): fast file search, directory drill-down, split preview pane (text/code/image/metadata), and open/reveal/copy path/trash actions.
- [ ] **Manual Check & Exit Criteria**: Searching large directories stays responsive without UI stalls or memory leaks, previews render cleanly, and drill-down + trash confirmation work keyboard-only.

#### Phase 5.9: Process Manager (Kill Process) Plugin `[PENDING]`
- [ ] Expose capability-gated `/proc` process inspection and signal host API (`SIGTERM`/`SIGKILL`).
- [ ] Build external WASM plugin (`com.aios.process-manager`): live `/proc` process list with tabular CPU/RAM accessories, sort/filter options, and `SIGTERM`/`SIGKILL` actions guarded by `ConfirmAlert`.
- [ ] **Manual Check & Exit Criteria**: Listing processes, sorting by CPU/RAM, and terminating a test process with confirmation work accurately with zero crashes on short-lived PIDs.

#### Phase 5.10: Color Picker & Converter Plugin `[PENDING]`
- [ ] Expose XDG Desktop Portal `org.freedesktop.portal.Screenshot.PickColor` host capability.
- [ ] Build external WASM plugin (`com.aios.color-picker`): screen eyedropper trigger, color history `Grid`, and instant HEX/RGB/HSL/OKLCH conversion & copy actions.
- [ ] **Manual Check & Exit Criteria**: Picking a color on Wayland records it in history `Grid` and copies formatted HEX/RGB/HSL/OKLCH values cleanly.

#### Phase 5.11: Focus & Timer Plugin `[PENDING]`
- [ ] Expose desktop notification (`org.freedesktop.Notifications`) and daemon timer scheduling host capability.
- [ ] Build external WASM plugin (`com.aios.focus-timer`): Countdown & Pomodoro sessions, pause/resume/reset, `Form` preset editor, and desktop notifications on completion.
- [ ] **Manual Check & Exit Criteria**: Starting, pausing, resuming, and completing a timer fires a native Wayland desktop notification and updates state while the launcher is closed.

#### Phase 5.12: Menu Bar / Status Bar Bridge `[PENDING]`
- [ ] Implement StatusNotifierItem (SNI) tray export over D-Bus and Waybar-compatible JSON stream (`aios status --json`) in `aios-wayland` / `aios-daemon` for `MenuBar` plugin outputs and active Focus timers.
- [ ] **Manual Check & Exit Criteria**: `aios status --json` streams live `MenuBar` / Focus timer updates cleanly for Waybar/Eww with zero polling leaks.

#### Phase 5.13: Floating Notes (Scratchpad) Plugin `[PENDING]`
- [ ] Add separate GPUI `XDG TopLevel` floating note window (`app_id: "aios-notes"`) in `aios-ui` with SQLite persistence.
- [ ] Build external WASM plugin (`com.aios.notes`): multi-note Markdown scratchpad, fuzzy note switcher, quick-capture action, and 60-day soft-delete trash.
- [ ] **Manual Check & Exit Criteria**: Opening, editing, switching, soft-deleting, and restoring notes work smoothly in a floating window alongside Hyprland/Niri rules in both Dark and Light themes.

### Phase 6: Polish, Packaging & Plugin Ecosystem Readiness `[PENDING]`

**Goal**: Production stability, performance hardening, packaging, and plugin author documentation.

- [ ] Startup & cold-toggle profiling (target: daemon memory < 40MB idle, toggle-to-first-frame < 16ms).
- [ ] Config & data export/import (`aios export` / `aios import`) for local backups.
- [ ] Local/Git plugin registry & `aios plugin install <source>` workflow.
- [ ] Packaging scripts for Arch Linux (`PKGBUILD` / AUR), Nix flake, and systemd user service (`aios-daemon.service`).
- [ ] Authoring guide for external WASM plugins (including blueprint for the future optional **AI Chat plugin**).
- [ ] **Exit Criteria**: Verified release builds, clean packaging definitions, and complete plugin authoring documentation.

---

## Key Technical Risks & Mitigations

| Risk                                                       | Impact                                                                          | Mitigation                                                                                                                                                                                             |
| ---------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| GPUI API churn across Zed releases                         | Build breakage during development                                               | Pin exact GPUI git rev/crate version in workspace `Cargo.toml`; isolate all GPUI primitives inside `aios-ui` behind internal view/theme adapters.                                                      |
| Layer-shell differences between Hyprland and Niri          | Focus grab, monitor placement, or blur inconsistencies                          | Test every UI milestone on both compositors; keep compositor-specific behavior strictly inside `aios-wayland` (`HyprlandAdapter` / `NiriAdapter`) with documented `layerrule` / `layer-rule` snippets. |
| Wayland global auto-paste / snippet expansion restrictions | Wayland forbids arbitrary synthetic keyboard input without privileged protocols | Support primary `CopyToClipboard` everywhere; provide optional paste backend via `zwp_virtual_keyboard_v1` / `wtype` / `ydotool` when permitted by the user's compositor.                              |
| WASM plugin UI round-trip latency                          | Sluggish keystrokes in plugin-controlled search views                           | Run `aios-fuzzy` natively on the host for `filtering: true` views; debounce `on_search_text_change` for remote/async plugins; keep WIT view payloads compact and diffed on host.                       |
| Clipboard daemon memory growth from large images           | High RAM usage over days of uptime                                              | Store clipboard binary blobs content-addressed on disk (`~/.local/share/aios/blobs/`) with configurable retention limit and thumbnail cache; keep only metadata in SQLite.                             |

---

## Explicitly Out of Scope for v1 (Core)

- **AI Chat / Quick AI in Core**: Deferred intentionally; will be built later as a standalone plugin using the standard Multi-View WASM Plugin API (`List+Detail`, streaming updates, `Form`, `Secrets`).
- **Cloud Sync / Accounts**: AIOS v1 is 100% local-first with file-based export/import.
- **X11 Support**: Strictly Wayland-only to avoid dual display-server complexity and leverage `wlr-layer-shell` cleanly.
- **Direct JS/TS Raycast Binary Compatibility**: Plugins target the AIOS WIT / WASM Component Model (allowing Rust or other languages compiling to WASM) plus Script Commands.
