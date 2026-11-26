//! Ergonomic WASM Component Model guest runtime (`wasm32-wasip2`) for external AIOS plugins.
//!
//! Enable `aios-plugin-sdk = { ..., features = ["guest"] }` in your `cdylib` plugin crate,
//! implement [`AiosPlugin`], and invoke [`crate::export_plugin!`].

use crate::{InlinePluginResult, PluginEvent, PluginResponse, ToastStyle};

#[allow(unsafe_code, clippy::all, missing_debug_implementations)]
#[doc(hidden)]
pub mod wit {
    wit_bindgen::generate!({
        path: "../../wit/aios-plugin.wit",
        world: "aios-extension",
        pub_export_macro: true,
        export_macro_name: "__export_aios_extension",
    });
}

/// Trait implemented by external WASM plugins compiled to `wasm32-wasip2`.
pub trait AiosPlugin: Default + 'static {
    /// Called when the user launches a command (`lifecycle::init-command`).
    fn init_command(&self, command_id: &str, args: &[String]) -> PluginResponse;

    /// Called for interactive view events (`SearchTextChanged`, `ActionTriggered`, `FormSubmitted`, etc.).
    fn handle_event(&self, command_id: &str, event: PluginEvent) -> PluginResponse;

    /// Optional live Root Search inline evaluator (triggered when `[inline_trigger]` fast-gate matches).
    fn evaluate_inline(&self, _query: &str) -> Option<InlinePluginResult> {
        None
    }

    /// Optional handler for inline Root Search actions (e.g. `Enter` copy-and-close).
    fn handle_inline_action(&self, _action_id: &str) -> Option<PluginResponse> {
        None
    }

    /// Optional periodic background refresh handler (`lifecycle::background-refresh`).
    fn background_refresh(&self, _command_id: &str) -> PluginResponse {
        PluginResponse::Noop
    }
}

/// Safe Rust wrappers around `aios:plugin/host-api` imports.
pub mod host {
    use super::wit::aios::plugin::host_api;
    use crate::ToastStyle;

    pub fn show_toast(style: ToastStyle, title: &str, message: Option<&str>) {
        let wit_style = match style {
            ToastStyle::Animated => host_api::ToastStyle::Animated,
            ToastStyle::Success => host_api::ToastStyle::Success,
            ToastStyle::Failure => host_api::ToastStyle::Failure,
        };
        host_api::show_toast(&host_api::Toast {
            style: wit_style,
            title: title.to_owned(),
            message: message.map(str::to_owned),
        });
    }

    pub fn show_hud(title: &str) {
        host_api::show_hud(title);
    }

    pub fn close_main_window() {
        host_api::close_main_window();
    }

    pub fn pop_to_root() {
        host_api::pop_to_root();
    }

    #[must_use]
    pub fn storage_get(key: &str) -> Option<Vec<u8>> {
        host_api::storage_get(key)
    }

    pub fn storage_set(key: &str, val: &[u8]) -> Result<(), String> {
        host_api::storage_set(key, val)
    }

    pub fn storage_remove(key: &str) -> Result<(), String> {
        host_api::storage_remove(key)
    }

    #[must_use]
    pub fn cache_get(key: &str) -> Option<Vec<u8>> {
        host_api::cache_get(key)
    }

    pub fn cache_set(key: &str, val: &[u8], ttl_seconds: u64) {
        host_api::cache_set(key, val, ttl_seconds);
    }

    #[must_use]
    pub fn preference(name: &str) -> Option<String> {
        host_api::get_preference(name)
    }

    #[must_use]
    pub fn theme_mode() -> String {
        host_api::get_theme_mode()
    }

    pub fn clipboard_read_text() -> Result<Option<String>, String> {
        host_api::clipboard_read_text()
    }

    pub fn clipboard_write_text(text: &str) -> Result<(), String> {
        host_api::clipboard_write_text(text)
    }

    pub fn secret_get(key: &str) -> Result<Option<String>, String> {
        host_api::secret_get(key)
    }

    pub fn secret_set(key: &str, val: &str) -> Result<(), String> {
        host_api::secret_set(key, val)
    }

    pub fn http_get(url: &str, headers: &[(&str, &str)]) -> Result<(u16, Vec<u8>), String> {
        let req = host_api::HttpRequestSpec {
            method: "GET".to_string(),
            url: url.to_owned(),
            headers: headers
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            body: None,
        };
        let resp = host_api::http_request(&req)?;
        Ok((resp.status, resp.body))
    }

    pub fn fs_read_text(path: &str) -> Result<String, String> {
        host_api::fs_read_text(path)
    }

    pub fn open_url(url: &str) -> Result<(), String> {
        host_api::open_url(url)
    }
}

#[doc(hidden)]
pub fn dispatch_init<P: AiosPlugin>(command_id: &str, args_json: &str) -> Result<String, String> {
    let args: Vec<String> = serde_json::from_str(args_json).unwrap_or_default();
    let plugin = P::default();
    plugin
        .init_command(command_id, &args)
        .to_json()
        .map_err(|e| format!("failed to encode PluginResponse: {e}"))
}

#[doc(hidden)]
pub fn dispatch_event<P: AiosPlugin>(event_json: &str) -> Result<String, String> {
    let event: PluginEvent =
        serde_json::from_str(event_json).map_err(|e| format!("invalid PluginEvent JSON: {e}"))?;
    let plugin = P::default();
    let response = match event {
        PluginEvent::InlineQuery { query } => PluginResponse::InlineResult {
            result: plugin.evaluate_inline(&query).map(Box::new),
        },
        PluginEvent::InlineAction { action_id } => plugin
            .handle_inline_action(&action_id)
            .unwrap_or(PluginResponse::Noop),
        other => {
            let cmd = match &other {
                PluginEvent::SearchTextChanged { command_id, .. }
                | PluginEvent::ActionTriggered { command_id, .. }
                | PluginEvent::DropdownChanged { command_id, .. }
                | PluginEvent::FormSubmitted { command_id, .. }
                | PluginEvent::ConfirmAccepted { command_id, .. }
                | PluginEvent::BackgroundRefresh { command_id } => command_id.clone(),
                PluginEvent::InlineQuery { .. } | PluginEvent::InlineAction { .. } => String::new(),
            };
            plugin.handle_event(&cmd, other)
        }
    };
    response
        .to_json()
        .map_err(|e| format!("failed to encode PluginResponse: {e}"))
}

#[doc(hidden)]
pub fn dispatch_background_refresh<P: AiosPlugin>(command_id: &str) -> Result<String, String> {
    let plugin = P::default();
    plugin
        .background_refresh(command_id)
        .to_json()
        .map_err(|e| format!("failed to encode PluginResponse: {e}"))
}

/// Exports a [`crate::guest::AiosPlugin`] type as a WebAssembly Component Model `aios-extension`.
#[macro_export]
macro_rules! export_plugin {
    ($plugin_ty:ty) => {
        struct __AiosGuestBridge;

        impl $crate::guest::wit::exports::aios::plugin::lifecycle::Guest for __AiosGuestBridge {
            fn init_command(command_id: String, args_json: String) -> Result<String, String> {
                $crate::guest::dispatch_init::<$plugin_ty>(&command_id, &args_json)
            }

            fn handle_event(event_json: String) -> Result<String, String> {
                $crate::guest::dispatch_event::<$plugin_ty>(&event_json)
            }

            fn background_refresh(command_id: String) -> Result<String, String> {
                $crate::guest::dispatch_background_refresh::<$plugin_ty>(&command_id)
            }

            fn dispose() {}
        }

        $crate::guest::wit::__export_aios_extension!(__AiosGuestBridge with_types_in $crate::guest::wit);
    };
}

// Keep ToastStyle imported cleanly for doc links.
const _: Option<ToastStyle> = None;
