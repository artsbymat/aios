//! External `wasm32-wasip2` Component Model bundle for AIOS Calculator & Converter (`com.aios.calculator`).

use aios_plugin_sdk::prelude::*;
use aios_plugins::calculator::{
    CalcOptions, HISTORY_STORAGE_KEY, HistoryRecord, build_calculator_list_view,
    decode_calc_payload, evaluate_expression, evaluate_inline_with_opts, push_history_record,
};

#[derive(Default)]
struct CalculatorWasmPlugin;

impl CalculatorWasmPlugin {
    fn load_options() -> CalcOptions {
        CalcOptions::from_lookup(host::preference, host::storage_get)
    }

    fn load_history() -> Vec<HistoryRecord> {
        host::storage_get(HISTORY_STORAGE_KEY)
            .and_then(|bytes| serde_json::from_slice::<Vec<HistoryRecord>>(&bytes).ok())
            .unwrap_or_default()
    }

    fn save_history(records: &[HistoryRecord]) {
        if let Ok(bytes) = serde_json::to_vec(records) {
            let _ = host::storage_set(HISTORY_STORAGE_KEY, &bytes);
        }
    }

    fn record_history(expr: &str, opts: &CalcOptions) {
        if let Ok(eval) = evaluate_expression(expr, opts) {
            let mut history = Self::load_history();
            push_history_record(&mut history, &eval);
            Self::save_history(&history);
        }
    }

    fn render_view(query: &str) -> PluginResponse {
        let opts = Self::load_options();
        let history = Self::load_history();
        PluginResponse::render(build_calculator_list_view(query, &opts, &history, true))
    }
}

impl AiosPlugin for CalculatorWasmPlugin {
    fn init_command(&self, _command_id: &str, args: &[String]) -> PluginResponse {
        let initial_query = args.first().map(String::as_str).unwrap_or("");
        Self::render_view(initial_query)
    }

    fn handle_event(&self, _command_id: &str, event: PluginEvent) -> PluginResponse {
        match event {
            PluginEvent::SearchTextChanged { query, .. } => Self::render_view(&query),
            PluginEvent::ActionTriggered { action_id, .. } => {
                if let Some(encoded) = action_id.strip_prefix("calc.copy-save:")
                    && let Some((expr, result)) = decode_calc_payload(encoded)
                {
                    let opts = Self::load_options();
                    Self::record_history(&expr, &opts);
                    return PluginResponse::CopyToClipboard {
                        text: result.clone(),
                        hud_title: Some(format!("Copied {result}")),
                    };
                }
                if let Some(encoded) = action_id.strip_prefix("calc.copy-full:")
                    && let Some((expr, result)) = decode_calc_payload(encoded)
                {
                    let opts = Self::load_options();
                    Self::record_history(&expr, &opts);
                    let full = format!("{expr} = {result}");
                    return PluginResponse::CopyToClipboard {
                        text: full,
                        hud_title: Some(format!("Copied {expr} = {result}")),
                    };
                }
                if let Some(raw) = action_id.strip_prefix("calc.copy-raw:") {
                    return PluginResponse::CopyToClipboard {
                        text: raw.to_owned(),
                        hud_title: Some(format!("Copied {raw}")),
                    };
                }
                if let Some(encoded) = action_id.strip_prefix("calc.save:")
                    && let Some((expr, _)) = decode_calc_payload(encoded)
                {
                    let opts = Self::load_options();
                    Self::record_history(&expr, &opts);
                    return Self::render_view(&expr);
                }
                if let Some(del_id) = action_id.strip_prefix("calc.delete:") {
                    let mut history = Self::load_history();
                    history.retain(|h| h.id != del_id);
                    Self::save_history(&history);
                    return Self::render_view("");
                }
                if action_id == "calc.clear-history" {
                    let _ = host::storage_remove(HISTORY_STORAGE_KEY);
                    return Self::render_view("");
                }
                PluginResponse::Noop
            }
            _ => PluginResponse::Noop,
        }
    }

    fn evaluate_inline(&self, query: &str) -> Option<InlinePluginResult> {
        let opts = Self::load_options();
        evaluate_inline_with_opts(query, &opts, "icon.svg").map(|(res, _)| res)
    }

    fn handle_inline_action(&self, action_id: &str) -> Option<PluginResponse> {
        let encoded = action_id.strip_prefix("copy:")?;
        let (expr, result) = decode_calc_payload(encoded)?;
        let opts = Self::load_options();
        Self::record_history(&expr, &opts);
        Some(PluginResponse::CopyToClipboard {
            text: result.clone(),
            hud_title: Some(format!("Copied {result}")),
        })
    }
}

aios_plugin_sdk::export_plugin!(CalculatorWasmPlugin);
