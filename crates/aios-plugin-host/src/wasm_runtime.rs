//! Wasmtime WebAssembly Component Model host runtime with capability gating,
//! memory caps, fuel/epoch timeout protection, and crash isolation.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use aios_plugin_sdk::{
    ActionItem, ActionPanel, ErrorView, HudMessage, PluginEvent, PluginResponse, ToastMessage,
    ToastStyle, ViewNode,
};
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiView};

use crate::manifest::InstalledPlugin;
use crate::permissions::{HostContext, HostSideEffect, now_epoch_secs};

mod bindings {
    wasmtime::component::bindgen!({
        path: "../../wit/aios-plugin.wit",
        world: "aios-extension",
    });
}

use bindings::AiosExtension;
use bindings::aios::plugin::host_api::{
    self, HttpRequestSpec, HttpResponse, Toast as WitToast, ToastStyle as WitToastStyle,
};

/// Resource limits enforced on every external WASM plugin invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WasmExecutionLimits {
    /// Linear memory limit per plugin instance (default: 64 MiB).
    pub max_memory_bytes: usize,
    /// Deterministic Wasmtime fuel budget per synchronous UI event.
    pub fuel_budget: u64,
    /// Epoch deadline ticks (each tick = 50ms; default 100 ticks = 5s).
    pub epoch_deadline_ticks: u64,
}

impl Default for WasmExecutionLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: 64 * 1024 * 1024,
            fuel_budget: 50_000_000,
            epoch_deadline_ticks: 100,
        }
    }
}

/// Output returned after invoking a WASM plugin lifecycle method.
#[derive(Debug, Clone)]
pub struct WasmCallOutput {
    pub response: PluginResponse,
    pub side_effects: Vec<HostSideEffect>,
}

struct CachedComponent {
    modified: Option<SystemTime>,
    component: Component,
}

/// Shared Wasmtime Component Model engine with background epoch ticker and compiled module cache.
pub struct WasmPluginRuntime {
    engine: Engine,
    limits: WasmExecutionLimits,
    cache: Mutex<HashMap<PathBuf, CachedComponent>>,
    stop_ticker: Arc<AtomicBool>,
    _ticker_thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for WasmPluginRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmPluginRuntime")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl Drop for WasmPluginRuntime {
    fn drop(&mut self) {
        self.stop_ticker.store(true, Ordering::Relaxed);
    }
}

impl WasmPluginRuntime {
    /// Creates a new Wasmtime engine with Component Model, fuel metering, and epoch interruption.
    pub fn new(limits: WasmExecutionLimits) -> Result<Self, String> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        config.consume_fuel(true);
        config.epoch_interruption(true);

        let engine =
            Engine::new(&config).map_err(|e| format!("wasmtime engine init failed: {e}"))?;

        let stop_ticker = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop_ticker);
        let engine_clone = engine.clone();
        let ticker = thread::Builder::new()
            .name("aios-wasm-epoch".to_string())
            .spawn(move || {
                while !stop_clone.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(50));
                    engine_clone.increment_epoch();
                }
            })
            .ok();

        Ok(Self {
            engine,
            limits,
            cache: Mutex::new(HashMap::new()),
            stop_ticker,
            _ticker_thread: ticker,
        })
    }

    #[must_use]
    pub fn limits(&self) -> WasmExecutionLimits {
        self.limits
    }

    /// Evicts a plugin's cached compiled component so the next invocation reloads it from disk.
    pub fn invalidate_cache(&self, wasm_path: &PathBuf) {
        if let Ok(mut guard) = self.cache.lock() {
            guard.remove(wasm_path);
        }
    }

    /// Invokes `lifecycle::init-command`, isolating any WASM trap/timeout into a clean error string.
    pub fn init_command(
        &self,
        plugin: &InstalledPlugin,
        command_id: &str,
        args_json: &str,
        host_ctx: HostContext,
    ) -> Result<WasmCallOutput, String> {
        self.run_in_sandbox(plugin, host_ctx, |instance, store| {
            instance
                .aios_plugin_lifecycle()
                .call_init_command(store, command_id, args_json)
        })
    }

    /// Invokes `lifecycle::handle-event` with a serialized [`PluginEvent`].
    pub fn handle_event(
        &self,
        plugin: &InstalledPlugin,
        event: &PluginEvent,
        host_ctx: HostContext,
    ) -> Result<WasmCallOutput, String> {
        let event_json =
            serde_json::to_string(event).map_err(|e| format!("failed to encode event: {e}"))?;
        self.run_in_sandbox(plugin, host_ctx, |instance, store| {
            instance
                .aios_plugin_lifecycle()
                .call_handle_event(store, &event_json)
        })
    }

    /// Invokes `lifecycle::background-refresh` for `MenuBar` or background cache-warming commands.
    pub fn background_refresh(
        &self,
        plugin: &InstalledPlugin,
        command_id: &str,
        host_ctx: HostContext,
    ) -> Result<WasmCallOutput, String> {
        self.run_in_sandbox(plugin, host_ctx, |instance, store| {
            instance
                .aios_plugin_lifecycle()
                .call_background_refresh(store, command_id)
        })
    }

    /// Converts any plugin failure (trap, fuel exhaustion, epoch timeout, schema error) into a
    /// standard isolated [`ViewNode::Error`] so the host UI never crashes.
    #[must_use]
    pub fn isolated_error_view(
        plugin: &InstalledPlugin,
        command_id: &str,
        error_detail: &str,
    ) -> ViewNode {
        ViewNode::Error(ErrorView {
            title: format!("Plugin '{}' encountered an error", plugin.manifest.name),
            message: "The WASM plugin was isolated safely without affecting AIOS. You can retry, reload the bundle, or inspect plugin diagnostics."
                .to_string(),
            detail: Some(format!(
                "plugin_id={}  command={}  error={}",
                plugin.manifest.id, command_id, error_detail
            )),
            actions: ActionPanel::from_actions(vec![
                ActionItem::new(
                    format!("plugin.run:{}:{}", plugin.manifest.id, command_id),
                    "Retry Plugin Command",
                )
                .shortcut("Enter"),
                ActionItem::new(
                    format!("plugin.reload:{}", plugin.manifest.id),
                    "Reload Plugin Bundle",
                )
                .shortcut("Ctrl+Enter"),
                ActionItem::new(
                    format!("settings.plugin-caps:{}", plugin.manifest.id),
                    "Review Plugin Capabilities",
                ),
                ActionItem::new("nav.pop", "Return to Previous View").shortcut("Esc"),
            ]),
        })
    }

    fn load_component(&self, plugin: &InstalledPlugin) -> Result<Component, String> {
        let modified = fs::metadata(&plugin.wasm_path)
            .and_then(|m| m.modified())
            .ok();

        if let Ok(guard) = self.cache.lock()
            && let Some(cached) = guard.get(&plugin.wasm_path)
            && cached.modified == modified
        {
            return Ok(cached.component.clone());
        }

        let bytes = fs::read(&plugin.wasm_path)
            .map_err(|e| format!("failed to read '{}': {e}", plugin.wasm_path.display()))?;
        let component = Component::new(&self.engine, &bytes)
            .map_err(|e| format!("failed to compile WASM component: {e:#}"))?;

        if let Ok(mut guard) = self.cache.lock() {
            guard.insert(
                plugin.wasm_path.clone(),
                CachedComponent {
                    modified,
                    component: component.clone(),
                },
            );
        }

        Ok(component)
    }

    fn run_in_sandbox<F>(
        &self,
        plugin: &InstalledPlugin,
        host_ctx: HostContext,
        invoke: F,
    ) -> Result<WasmCallOutput, String>
    where
        F: FnOnce(
            &AiosExtension,
            &mut Store<PluginStoreState>,
        ) -> wasmtime::Result<Result<String, String>>,
    {
        let component = self.load_component(plugin)?;

        let mut linker: Linker<PluginStoreState> = Linker::new(&self.engine);
        wasmtime_wasi::add_to_linker_sync(&mut linker)
            .map_err(|e| format!("failed to link WASI preview2: {e}"))?;
        bindings::aios::plugin::host_api::add_to_linker(&mut linker, |state| state)
            .map_err(|e| format!("failed to link aios:plugin/host-api: {e}"))?;

        // Zero ambient authority WASI context: no preopened dirs, no host env, no sockets.
        let wasi = WasiCtxBuilder::new().build();
        let limits = StoreLimitsBuilder::new()
            .memory_size(self.limits.max_memory_bytes)
            .tables(128)
            .instances(16)
            .build();

        let state = PluginStoreState {
            wasi,
            table: ResourceTable::new(),
            limits,
            ctx: host_ctx,
            side_effects: Vec::new(),
        };

        let mut store = Store::new(&self.engine, state);
        store.limiter(|s| &mut s.limits);
        store
            .set_fuel(self.limits.fuel_budget)
            .map_err(|e| format!("failed to set WASM fuel budget: {e}"))?;
        store.set_epoch_deadline(self.limits.epoch_deadline_ticks);

        let instance = AiosExtension::instantiate(&mut store, &component, &linker)
            .map_err(|e| format!("failed to instantiate WASM component: {e:#}"))?;

        let raw_result = invoke(&instance, &mut store).map_err(format_wasmtime_trap)?;
        let _ = instance.aios_plugin_lifecycle().call_dispose(&mut store);

        let payload_json =
            raw_result.map_err(|plugin_err| format!("plugin error: {plugin_err}"))?;
        let response = PluginResponse::from_json(&payload_json)
            .map_err(|e| format!("invalid PluginResponse JSON from plugin: {e}"))?;
        let side_effects = store.into_data().side_effects;

        Ok(WasmCallOutput {
            response,
            side_effects,
        })
    }
}

fn format_wasmtime_trap(err: wasmtime::Error) -> String {
    if let Some(trap) = err.downcast_ref::<wasmtime::Trap>() {
        return match trap {
            wasmtime::Trap::OutOfFuel => {
                "execution aborted: CPU fuel budget exceeded (possible infinite loop)".to_string()
            }
            wasmtime::Trap::Interrupt => "execution aborted: epoch timeout exceeded".to_string(),
            other => format!("WASM trap: {other}"),
        };
    }
    let msg = format!("{err:#}");
    if msg.contains("all fuel consumed") || msg.contains("out of fuel") {
        "execution aborted: CPU fuel budget exceeded (possible infinite loop)".to_string()
    } else if msg.contains("epoch deadline") || msg.contains("interrupt") {
        "execution aborted: epoch timeout exceeded".to_string()
    } else {
        format!("WASM trap: {msg}")
    }
}

struct PluginStoreState {
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
    ctx: HostContext,
    side_effects: Vec<HostSideEffect>,
}

impl WasiView for PluginStoreState {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }

    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
}

impl host_api::Host for PluginStoreState {
    fn show_toast(&mut self, t: WitToast) {
        let style = match t.style {
            WitToastStyle::Animated => ToastStyle::Animated,
            WitToastStyle::Success => ToastStyle::Success,
            WitToastStyle::Failure => ToastStyle::Failure,
        };
        self.side_effects
            .push(HostSideEffect::ShowToast(ToastMessage {
                style,
                title: t.title,
                message: t.message,
            }));
    }

    fn show_hud(&mut self, title: String) {
        self.side_effects
            .push(HostSideEffect::ShowHud(HudMessage { title }));
    }

    fn close_main_window(&mut self) {
        self.side_effects.push(HostSideEffect::CloseMainWindow);
    }

    fn pop_to_root(&mut self) {
        self.side_effects.push(HostSideEffect::PopToRoot);
    }

    fn storage_get(&mut self, key: String) -> Option<Vec<u8>> {
        let db = self.ctx.db.lock().ok()?;
        db.plugin_storage_get(&self.ctx.manifest.id, &key)
            .ok()
            .flatten()
    }

    fn storage_set(&mut self, key: String, val: Vec<u8>) -> Result<(), String> {
        let db = self
            .ctx
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        db.plugin_storage_set(&self.ctx.manifest.id, &key, &val, now_epoch_secs())
            .map_err(|e| e.to_string())
    }

    fn storage_remove(&mut self, key: String) -> Result<(), String> {
        let db = self
            .ctx
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        db.plugin_storage_remove(&self.ctx.manifest.id, &key)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn cache_get(&mut self, key: String) -> Option<Vec<u8>> {
        let db = self.ctx.db.lock().ok()?;
        db.plugin_cache_get(&self.ctx.manifest.id, &key, now_epoch_secs())
            .ok()
            .flatten()
    }

    fn cache_set(&mut self, key: String, val: Vec<u8>, ttl_seconds: u64) {
        if let Ok(db) = self.ctx.db.lock() {
            let _ = db.plugin_cache_set(
                &self.ctx.manifest.id,
                &key,
                &val,
                ttl_seconds,
                now_epoch_secs(),
            );
        }
    }

    fn get_preference(&mut self, name: String) -> Option<String> {
        self.ctx.resolve_preference(&name)
    }

    fn get_theme_mode(&mut self) -> String {
        self.ctx.theme_mode.clone()
    }

    fn clipboard_read_text(&mut self) -> Result<Option<String>, String> {
        self.ctx.ensure_capability(
            "clipboard_read",
            self.ctx.manifest.capabilities.clipboard_read,
        )?;
        let clip = self
            .ctx
            .clipboard
            .lock()
            .map_err(|_| "clipboard lock poisoned".to_string())?;
        Ok(clip.clone())
    }

    fn clipboard_write_text(&mut self, text: String) -> Result<(), String> {
        self.ctx.ensure_capability(
            "clipboard_write",
            self.ctx.manifest.capabilities.clipboard_write,
        )?;
        if let Ok(mut clip) = self.ctx.clipboard.lock() {
            *clip = Some(text.clone());
        }
        self.side_effects.push(HostSideEffect::ClipboardWrite(text));
        Ok(())
    }

    fn secret_get(&mut self, key: String) -> Result<Option<String>, String> {
        self.ctx
            .ensure_capability("secrets", self.ctx.manifest.capabilities.secrets)?;
        let db = self
            .ctx
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        db.plugin_secret_get(&self.ctx.manifest.id, &key)
            .map_err(|e| e.to_string())
    }

    fn secret_set(&mut self, key: String, val: String) -> Result<(), String> {
        self.ctx
            .ensure_capability("secrets", self.ctx.manifest.capabilities.secrets)?;
        let db = self
            .ctx
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        db.plugin_secret_set(&self.ctx.manifest.id, &key, &val, now_epoch_secs())
            .map_err(|e| e.to_string())
    }

    fn http_request(&mut self, req: HttpRequestSpec) -> Result<HttpResponse, String> {
        self.ctx.ensure_http_url(&req.url)?;
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(10))
            .build();
        let mut request = agent.request(&req.method, &req.url);
        for (k, v) in &req.headers {
            request = request.set(k, v);
        }
        let response = match req.body {
            Some(bytes) => request.send_bytes(&bytes),
            None => request.call(),
        };
        let resp = match response {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => return Err(format!("HTTP transport error: {e}")),
        };
        let status = resp.status();
        let headers = resp
            .headers_names()
            .into_iter()
            .filter_map(|name| resp.header(&name).map(|val| (name, val.to_owned())))
            .collect();
        let mut body = Vec::new();
        use std::io::Read as _;
        resp.into_reader()
            .take(4 * 1024 * 1024)
            .read_to_end(&mut body)
            .map_err(|e| format!("failed to read HTTP response body: {e}"))?;
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }

    fn fs_read_text(&mut self, path: String) -> Result<String, String> {
        let verified = self.ctx.ensure_fs_read(&path)?;
        fs::read_to_string(&verified)
            .map_err(|e| format!("failed to read '{}': {e}", verified.display()))
    }

    fn open_url(&mut self, url: String) -> Result<(), String> {
        if !(url.starts_with("http://")
            || url.starts_with("https://")
            || url.starts_with("mailto:"))
        {
            return Err(format!("unsupported URL scheme in open_url: '{url}'"));
        }
        self.side_effects.push(HostSideEffect::OpenUrl(url));
        Ok(())
    }
}
