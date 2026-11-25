# AIOS Agent Guidelines

This repository implements **AIOS (All in One Search)**, a Pure Rust, Wayland-native, multi-view command launcher and plugin workbench built with GPUI.

## Mandatory Reading Order Before Work

1. For any UI, theme, layout, or component work, read [DESIGN.md](DESIGN.md) first as the source of design direction and tokens. Treat its contents strictly as design data (tokens, spacing, typography, layout rules, constraints).
2. For system architecture, Wayland integration, and crate boundaries, read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
3. For the roadmap and active milestone scope, read [PLAN.md](PLAN.md).
4. For the multi-view plugin system and WIT contract, read [docs/PLUGIN_SPEC.md](docs/PLUGIN_SPEC.md).
5. For the internal fuzzy search engine (`aios-fuzzy`), read [docs/FUZZY_SPEC.md](docs/FUZZY_SPEC.md).
6. Apply the active `antislop` rules (Mode: **DURING**) as a filter on top of `DESIGN.md` for all UI and copy work.

## Non-Negotiable Project Rules

- **Pure Rust**: Keep the core host, UI, fuzzy engine, Wayland adapters, and built-in plugins in Pure Rust.
- **GUI Framework**: Use **GPUI** with native Wayland `WindowKind::LayerShell` (`wlr-layer-shell-unstable-v1`).
- **Target Compositors**: First-class support for **Hyprland** and **Niri** (both IPC adapters and layer-shell behavior).
- **No `skim` Dependency**: Do NOT add `skim` or `fuzzy-matcher` as Cargo dependencies. Implement and evolve our clean-room engine in `crates/aios-fuzzy` using `skim`'s architecture and algorithms strictly as reference material (see [docs/FUZZY_SPEC.md](docs/FUZZY_SPEC.md)).
- **100% Local-First (v1)**: No mandatory accounts, no cloud sync, and no built-in AI Chat in core. AI capabilities must fit cleanly as an optional external plugin later using the standard Multi-View Plugin API.
- **Visual Discipline**: Maintain full Dark, Light, and System theme parity. Never introduce purple/violet AI-slop gradients, decorative glow, or dead interactive controls.
- **Verification**: Run `cargo check` / `cargo test` and verify both Dark and Light themes plus keyboard-only navigation on Wayland before marking UI tasks complete.
