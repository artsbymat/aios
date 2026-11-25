#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

TMP_XDG="$(mktemp -d)"
trap 'rm -rf "$TMP_XDG"' EXIT

export XDG_CONFIG_HOME="$TMP_XDG/config"
export XDG_DATA_HOME="$TMP_XDG/data"
export XDG_CACHE_HOME="$TMP_XDG/cache"
export XDG_RUNTIME_DIR="$TMP_XDG/run"

echo "[1/4] Rebuilding reference WASM plugin (palette-grid-wasm)..."
cargo run -q -p aios-plugin-host --example build_example_plugins

echo "[2/4] Installing palette-grid-wasm into isolated XDG plugin directory..."
cargo run -q -p aios-app -- plugin install examples/plugins/palette-grid-wasm
cargo run -q -p aios-app -- plugin list

echo "[3/4] Running Wasmtime persistence E2E test (save color -> SQLite -> reopen Grid)..."
cargo test -p aios-plugin-host --test plugin_runtime shipped_palette_grid_wasm_saves_custom_swatches_and_reloads -- --nocapture

echo "[4/4] Running UI navigation & keybind stability E2E test (enter/exit Create Custom Swatch x3 + form submit)..."
cargo test -p aios-ui --test ui_state palette_plugin_enter_exit_and_form_save_preserve_all_keybinds -- --nocapture

echo "ALL CHECKS PASSED!"
