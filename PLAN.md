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
    aios-plugin-host/        # Wasmtime host runtime, capability sandbox, script runner, registry
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
| Phase 4 | Hybrid Plugin Runtime (WASM + Script Commands + SDK)                                       | `[x]` Completed |
| Phase 5 | First-Party External Plugins Suite (Calc, WM, Clipboard, Files, Snippets, etc.)            | `[ ]` Pending     |
| Phase 6 | Polish, Packaging & Plugin Ecosystem Readiness                                             | `[ ]` Pending   |

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

- [x] Implement `aios-wayland` single-instance daemon + Unix socket IPC (`aios toggle`, `aios show`, `aios hide`, `aios open <uri>`).
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
- [x] **Exit Criteria**: At least two reference external WASM plugins (`com.aios.quick-notes` `List+Detail`, `com.aios.palette-grid` `Grid+Form`) and two Script Commands (`system-uptime.sh`, `disk-usage.sh`) run end-to-end inside AIOS with full isolation (`46/46` workspace tests, zero clippy warnings).

### Phase 5: First-Party External Plugins Suite `[PENDING]`

**Goal**: Deliver Raycast's productivity suite as modular external plugins running on the Phase 4 Hybrid Plugin Runtime (WASM + Script Commands + host Wayland capabilities).

- [ ] **Inline Calculator & Converter Plugin**: Live math expressions, hex/bin/octal, units (length, weight, temp, data, CSS), date/timezone math, cached offline currency converter.
- [ ] **Quicklinks Plugin**: Custom URL/path templates with `{query}` / `{argument}` placeholders and Fallback Command integration.
- [ ] **Window Management Plugin (Hyprland & Niri)**:
  - [ ] Hyprland IPC capability (`.socket.sock` + `.socket2.sock`): focus window, move/resize/snap, fullscreen, toggle floating, switch/move workspace, move monitor.
  - [ ] Niri IPC capability (`$NIRI_SOCKET` via `niri-ipc`): focus window/column, consume/expel, maximize/center column, switch/move workspace, overview toggle, monitor switching.
- [ ] **System Commands Plugin**: Lock, Suspend, Reboot, Shutdown, Logout, Empty Trash, PipeWire/PulseAudio volume & sink switcher, brightness, MPRIS media controls, Dark/Light theme toggle.
- [ ] **Clipboard History Plugin**: Pure Rust Wayland daemon (`ext-data-control-v1` / `zwlr_data_control_v1`) modeled on `Rolv-Apneseth/clipvault`'s storage & privacy design (byte-for-byte preservation, min/max entry size & age housekeeping, regex ignore patterns + sensitive-app window blacklist) supporting text, code, links, images, files, and color swatches; split preview pane; pin/delete; copy/paste action.
- [ ] **File Search & Preview Plugin**: High-speed file walker + `aios-fuzzy` filtering, directory drill-down, preview pane (text/code/image/metadata), open/reveal/copy path/trash.
- [ ] **Snippets Plugin**: Snippet manager (`List+Detail` + `Form` editor), dynamic placeholders (`{clipboard}`, `{date}`, `{time}`, `{cursor}`, `{argument}`), instant copy/paste from launcher, import/export JSON/TOML.
- [ ] **Emoji & Symbols Picker Plugin**: `Grid` view for Unicode emoji, math/currency symbols, and Nerd Font glyphs with recent/frequent tracking and skin tone preference.
- [ ] **Floating Notes (Scratchpad) Plugin**: Separate GPUI top-level/layer floating note window, multi-note Markdown editor, fuzzy note switcher, 60-day soft-delete trash.
- [ ] **Process Manager (Kill Process) Plugin**: Live `/proc` process list with tabular CPU/RAM accessories, `SIGTERM`/`SIGKILL` actions with confirmation.
- [ ] **Color Picker & Converter Plugin**: XDG Desktop Portal / compositor screen eyedropper, color history `Grid`, HEX/RGB/HSL/OKLCH conversion.
- [ ] **Focus & Timer Plugin**: Countdown / Pomodoro sessions with desktop notifications (`org.freedesktop.Notifications`) and status bar integration.
- [ ] **Menu Bar / Status Bar Bridge**: StatusNotifierItem (SNI) tray host export + Waybar JSON IPC stream (`aios status --json`) for `MenuBar` plugins and active Focus timers.
- [ ] **Exit Criteria**: All first-party external plugins run cleanly offline through the plugin runtime with complete Empty/Loading/Error states in both Dark and Light themes.

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
