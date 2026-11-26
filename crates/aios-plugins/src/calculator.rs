//! Phase 5.1: First-party Inline Calculator & Converter Plugin (`com.aios.calculator`).
//!
//! Features:
//! - Recursive-descent arithmetic & scientific math (`+ - * / % ^ !`, parentheses, implicit mult,
//!   percentages `20% of 450` / `100 + 15%`, constants `pi`/`e`/`tau`/`phi`, trig/log/round funcs)
//! - Programmer base literals & conversions (`0x`, `0b`, `0o`, `255 to hex/bin/oct`, bitwise ops)
//! - Unit converter (Length, Weight, Temperature, Data SI/IEC, Time, Speed, Area, CSS `px/rem/em/pt`)
//! - Pure Rust Date & Timezone math (`today + 14d`, `2026-12-31 - today`, `now in tokyo`, `14:30 wib to utc`)
//! - Offline Currency converter (26 built-in reference currencies + symbol prefixes `$49.99 to idr`)
//! - Sub-microsecond host fast-gate + optional `=` exclusive prefix + SQLite calculation history

use std::collections::HashMap;
#[cfg(feature = "host")]
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
#[cfg(feature = "host")]
use std::sync::{Arc, Mutex};
#[cfg(feature = "host")]
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "host")]
use aios_core::Database;
use aios_core::view::FilterMode;
#[cfg(feature = "host")]
use aios_plugin_sdk::{
    CommandMode, CommandSpec, DefaultViewKind, PluginCapabilities, PluginEvent, PluginManifest,
    PluginResponse, PreferenceKind, PreferenceSpec,
};
use aios_plugin_sdk::{
    ActionItem, ActionPanel, ActionPanelBuilder, EmptyView, InlinePluginResult, ListItemBuilder,
    ListViewBuilder, MetadataEntry, SemanticTone, ViewNode,
};
use serde::{Deserialize, Serialize};

#[cfg(feature = "host")]
use crate::{FirstPartyPlugin, PluginEnv};
use crate::now_epoch_secs;

pub const CALCULATOR_PLUGIN_ID: &str = "com.aios.calculator";
pub const CALCULATOR_COMMAND_ID: &str = "calculator";
pub const CALCULATOR_ICON_ASSET: &str = "asset://icons/plugins/calculator.svg";
pub const CALCULATOR_REFRESH_RATES_ACTION: &str = "plugin.calc.refresh-rates";
pub const HISTORY_STORAGE_KEY: &str = "history_v1";
pub const CUSTOM_RATES_STORAGE_KEY: &str = "currency_rates_v1";
pub const MAX_HISTORY_ENTRIES: usize = 50;
const DEFAULT_CURRENCY_REFRESH_SECS: u64 = 24 * 3600;
const MIN_CURRENCY_REFRESH_SECS: u64 = 300;
#[cfg(feature = "host")]
const FAILURE_RETRY_COOLDOWN_SECS: i64 = 600;
#[cfg(feature = "host")]
const HTTP_TIMEOUT_SECS: u64 = 5;

#[cfg(feature = "host")]
const FAWAZ_JSDELIVR_URL: &str =
    "https://cdn.jsdelivr.net/npm/@fawazahmed0/currency-api@latest/v1/currencies/usd.json";
#[cfg(feature = "host")]
const FAWAZ_PAGES_FALLBACK_URL: &str =
    "https://latest.currency-api.pages.dev/v1/currencies/usd.json";
#[cfg(feature = "host")]
const OPEN_ER_API_URL: &str = "https://open.er-api.com/v6/latest/USD";

#[cfg(feature = "host")]
static FX_FETCH_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "host")]
static FX_LAST_ATTEMPT_SECS: AtomicI64 = AtomicI64::new(0);

/// Persisted offline-first exchange rate snapshot stored in SQLite (`currency_rates_v1`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedCurrencyRates {
    pub updated_at: i64,
    pub date_label: String,
    pub source: String,
    pub rates: HashMap<String, f64>,
}

/// User-configurable calculator settings loaded from SQLite plugin preferences.
#[derive(Debug, Clone)]
pub struct CalcOptions {
    pub base_font_px: f64,
    pub decimal_places: usize,
    pub default_currency: String,
    pub custom_rates_usd: HashMap<String, f64>,
    pub rates_updated_at: Option<i64>,
    pub rates_date_label: Option<String>,
    pub rates_source: Option<String>,
    pub refresh_interval_secs: Option<u64>,
}

impl Default for CalcOptions {
    fn default() -> Self {
        Self {
            base_font_px: 16.0,
            decimal_places: 6,
            default_currency: "IDR".to_string(),
            custom_rates_usd: HashMap::new(),
            rates_updated_at: None,
            rates_date_label: None,
            rates_source: None,
            refresh_interval_secs: Some(DEFAULT_CURRENCY_REFRESH_SECS),
        }
    }
}

impl CalcOptions {
    /// Builds [`CalcOptions`] from raw preference + cached-rates lookup closures (works in both
    /// native host and `wasm32-wasip2` guest plugins).
    #[must_use]
    pub fn from_lookup<P, S>(mut get_pref: P, mut get_storage: S) -> Self
    where
        P: FnMut(&str) -> Option<String>,
        S: FnMut(&str) -> Option<Vec<u8>>,
    {
        let mut opts = Self::default();
        if let Some(raw) = get_pref("base_font_px")
            && let Ok(v) = raw.trim().parse::<f64>()
            && (4.0..=128.0).contains(&v)
        {
            opts.base_font_px = v;
        }
        if let Some(raw) = get_pref("decimal_places")
            && let Ok(v) = raw.trim().parse::<usize>()
        {
            opts.decimal_places = v.min(12);
        }
        if let Some(raw_bytes) = get_storage(CUSTOM_RATES_STORAGE_KEY)
            && let Some(cached) = decode_cached_currency_rates(&raw_bytes)
        {
            opts.custom_rates_usd = cached.rates;
            opts.rates_updated_at = Some(cached.updated_at);
            opts.rates_date_label = Some(cached.date_label);
            opts.rates_source = Some(cached.source);
        }
        if let Some(raw) = get_pref("default_currency") {
            let trimmed = raw.trim().to_ascii_uppercase();
            if lookup_currency_rate(&trimmed, &opts.custom_rates_usd).is_some() {
                opts.default_currency = trimmed;
            }
        }
        if let Some(raw) = get_pref("currency_refresh_interval") {
            opts.refresh_interval_secs = parse_refresh_interval_secs(&raw);
        }
        opts
    }

    #[cfg(feature = "host")]
    fn from_env(env: &PluginEnv<'_>) -> Self {
        Self::from_lookup(|k| env.preference(k), |k| env.storage_get(k))
    }
}

/// Parses `currency_refresh_interval` values (`off`/`0`, `6h`, `12h`, `24h`, `3d`, `7d`, `<N>m`).
/// Returns `None` when disabled (`off`/`0`), or `Some(secs)` clamped to at least 5 minutes.
#[must_use]
pub fn parse_refresh_interval_secs(raw: &str) -> Option<u64> {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() {
        return Some(DEFAULT_CURRENCY_REFRESH_SECS);
    }
    if matches!(s.as_str(), "off" | "0" | "none" | "disabled" | "never") {
        return None;
    }
    let (num_part, mult) = if let Some(n) = s.strip_suffix('d') {
        (n, 86_400u64)
    } else if let Some(n) = s.strip_suffix('h') {
        (n, 3_600u64)
    } else if let Some(n) = s.strip_suffix('m') {
        (n, 60u64)
    } else if let Some(n) = s.strip_suffix('s') {
        (n, 1u64)
    } else {
        // Plain number interpreted as hours for ergonomic settings input.
        (s.as_str(), 3_600u64)
    };
    let n = num_part.trim().parse::<u64>().ok()?;
    if n == 0 {
        return None;
    }
    Some(n.saturating_mul(mult).max(MIN_CURRENCY_REFRESH_SECS))
}

/// Returns `true` when cached currency rates are absent or older than `ttl_secs`.
#[must_use]
pub fn needs_currency_refresh(
    cached_updated_at: Option<i64>,
    ttl_secs: Option<u64>,
    now_secs: i64,
) -> bool {
    let Some(ttl) = ttl_secs else {
        return false;
    };
    let Some(updated_at) = cached_updated_at else {
        return true;
    };
    now_secs.saturating_sub(updated_at) >= i64::try_from(ttl).unwrap_or(i64::MAX)
}

/// Decodes cached live exchange rates from raw JSON bytes (supports legacy raw `HashMap`).
#[must_use]
pub fn decode_cached_currency_rates(raw_bytes: &[u8]) -> Option<CachedCurrencyRates> {
    if let Ok(cached) = serde_json::from_slice::<CachedCurrencyRates>(raw_bytes)
        && !cached.rates.is_empty()
    {
        return Some(cached);
    }
    if let Ok(rates) = serde_json::from_slice::<HashMap<String, f64>>(raw_bytes)
        && !rates.is_empty()
    {
        return Some(CachedCurrencyRates {
            updated_at: now_epoch_secs(),
            date_label: "custom".to_string(),
            source: "Custom SQLite rates".to_string(),
            rates,
        });
    }
    None
}

/// Loads cached live exchange rates from SQLite (compatible with legacy raw `HashMap` format).
#[cfg(feature = "host")]
#[must_use]
pub fn load_cached_currency_rates(env: &PluginEnv<'_>) -> Option<CachedCurrencyRates> {
    let raw_bytes = env.storage_get(CUSTOM_RATES_STORAGE_KEY)?;
    decode_cached_currency_rates(&raw_bytes)
}

/// Persists a `CachedCurrencyRates` snapshot to SQLite plugin storage.
#[cfg(feature = "host")]
pub fn save_cached_currency_rates(
    db: &Arc<Mutex<Database>>,
    cached: &CachedCurrencyRates,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(cached).map_err(|e| e.to_string())?;
    let guard = db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    guard
        .plugin_storage_set(
            CALCULATOR_PLUGIN_ID,
            CUSTOM_RATES_STORAGE_KEY,
            &bytes,
            cached.updated_at,
        )
        .map_err(|e| e.to_string())
}

/// Parses `@fawazahmed0/currency-api` `usd.json` (`{"date": "YYYY-MM-DD", "usd": {"idr": ...}}`).
pub fn parse_fawaz_currency_json(body: &str) -> Result<(String, HashMap<String, f64>), String> {
    let root: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("invalid JSON: {e}"))?;
    let date_label = root
        .get("date")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("latest")
        .to_string();
    let usd_map = root
        .get("usd")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "missing 'usd' rates object".to_string())?;

    let mut rates = HashMap::new();
    for &(sym, _, _) in OFFLINE_USD_RATES {
        let key = sym.to_ascii_lowercase();
        if let Some(val) = usd_map.get(&key).and_then(serde_json::Value::as_f64)
            && val.is_finite()
            && val > 0.0
        {
            rates.insert(sym.to_string(), val);
        }
    }
    rates.insert("USD".to_string(), 1.0);
    if rates.len() < 5 {
        return Err("currency payload contained too few valid rates".to_string());
    }
    Ok((date_label, rates))
}

/// Parses `open.er-api.com/v6/latest/USD` (`{"result": "success", "rates": {"IDR": ...}}`).
pub fn parse_open_er_api_json(body: &str) -> Result<(String, HashMap<String, f64>), String> {
    let root: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("invalid JSON: {e}"))?;
    if let Some(res) = root.get("result").and_then(serde_json::Value::as_str)
        && !res.eq_ignore_ascii_case("success")
    {
        return Err(format!("open.er-api returned status '{res}'"));
    }
    let rates_obj = root
        .get("rates")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "missing 'rates' object".to_string())?;

    let date_label = root
        .get("time_last_update_unix")
        .and_then(serde_json::Value::as_i64)
        .map(|ts| {
            let (y, m, d) = civil_from_days(ts.div_euclid(86_400));
            format!("{y:04}-{m:02}-{d:02}")
        })
        .unwrap_or_else(|| {
            let (y, m, d) = civil_from_days(today_days_utc());
            format!("{y:04}-{m:02}-{d:02}")
        });

    let mut rates = HashMap::new();
    for &(sym, _, _) in OFFLINE_USD_RATES {
        if let Some(val) = rates_obj.get(sym).and_then(serde_json::Value::as_f64)
            && val.is_finite()
            && val > 0.0
        {
            rates.insert(sym.to_string(), val);
        }
    }
    rates.insert("USD".to_string(), 1.0);
    if rates.len() < 5 {
        return Err("open.er-api payload contained too few valid rates".to_string());
    }
    Ok((date_label, rates))
}

/// Fetches the latest USD-based exchange rates using primary CDN (`fawazahmed0/currency-api`,
/// covering 26 fiat + crypto currencies) with automatic fallback to `pages.dev` and `open.er-api.com`.
#[cfg(feature = "host")]
pub fn fetch_latest_currency_rates() -> Result<CachedCurrencyRates, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .user_agent("aios-calculator/0.1")
        .build();
    let now = now_epoch_secs();
    let mut errors = Vec::new();

    for (url, source) in [
        (FAWAZ_JSDELIVR_URL, "fawazahmed0/currency-api"),
        (FAWAZ_PAGES_FALLBACK_URL, "fawazahmed0/currency-api"),
    ] {
        match agent.get(url).call() {
            Ok(resp) => match resp.into_string() {
                Ok(body) => match parse_fawaz_currency_json(&body) {
                    Ok((date_label, rates)) => {
                        return Ok(CachedCurrencyRates {
                            updated_at: now,
                            date_label,
                            source: source.to_string(),
                            rates,
                        });
                    }
                    Err(e) => errors.push(format!("{url}: {e}")),
                },
                Err(e) => errors.push(format!("{url}: read failed: {e}")),
            },
            Err(e) => errors.push(format!("{url}: {e}")),
        }
    }

    match agent.get(OPEN_ER_API_URL).call() {
        Ok(resp) => match resp.into_string() {
            Ok(body) => {
                let (date_label, rates) = parse_open_er_api_json(&body)?;
                return Ok(CachedCurrencyRates {
                    updated_at: now,
                    date_label,
                    source: "open.er-api.com".to_string(),
                    rates,
                });
            }
            Err(e) => errors.push(format!("{OPEN_ER_API_URL}: read failed: {e}")),
        },
        Err(e) => errors.push(format!("{OPEN_ER_API_URL}: {e}")),
    }

    Err(errors
        .into_iter()
        .last()
        .unwrap_or_else(|| "network unavailable".to_string()))
}

/// Immediately fetches and persists fresh exchange rates (used by manual "Refresh Exchange Rates Now"
/// off the UI thread). Preserves any existing crypto rates when falling back to fiat-only `open.er-api`.
#[cfg(feature = "host")]
pub fn refresh_currency_rates_now(db: &Arc<Mutex<Database>>) -> Result<CachedCurrencyRates, String> {
    let mut fetched = fetch_latest_currency_rates()?;
    // If fallback provider lacked crypto (BTC/ETH), keep previously cached values for missing codes.
    if let Ok(guard) = db.lock()
        && let Ok(Some(raw)) = guard.plugin_storage_get(CALCULATOR_PLUGIN_ID, CUSTOM_RATES_STORAGE_KEY)
        && let Ok(prev) = serde_json::from_slice::<CachedCurrencyRates>(&raw)
    {
        for (k, v) in prev.rates {
            fetched.rates.entry(k).or_insert(v);
        }
    }
    save_cached_currency_rates(db, &fetched)?;
    Ok(fetched)
}

#[cfg(feature = "host")]
struct InFlightGuard;
#[cfg(feature = "host")]
impl Drop for InFlightGuard {
    fn drop(&mut self) {
        FX_FETCH_IN_FLIGHT.store(false, Ordering::Release);
    }
}

/// Non-blocking Stale-While-Revalidate background refresh: checks SQLite cache TTL and failure
/// cooldown, then spawns at most one short-lived thread to refresh cached rates without blocking UI.
#[cfg(feature = "host")]
pub fn maybe_schedule_stale_currency_refresh(db: &Arc<Mutex<Database>>) {
    if cfg!(test) {
        return;
    }
    let env = PluginEnv::new(CALCULATOR_PLUGIN_ID, "system", Arc::clone(db));
    let opts = CalcOptions::from_env(&env);
    let now = now_epoch_secs();
    if !needs_currency_refresh(opts.rates_updated_at, opts.refresh_interval_secs, now) {
        return;
    }
    let last_attempt = FX_LAST_ATTEMPT_SECS.load(Ordering::Acquire);
    if last_attempt > 0 && now.saturating_sub(last_attempt) < FAILURE_RETRY_COOLDOWN_SECS {
        return;
    }
    if FX_FETCH_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    FX_LAST_ATTEMPT_SECS.store(now, Ordering::Release);

    let db_clone = Arc::clone(db);
    let spawn_res = std::thread::Builder::new()
        .name("aios-calc-fx".to_string())
        .spawn(move || {
            let _guard = InFlightGuard;
            match refresh_currency_rates_now(&db_clone) {
                Ok(cached) => {
                    tracing::info!(
                        source = %cached.source,
                        date = %cached.date_label,
                        currencies = cached.rates.len(),
                        "refreshed cached calculator exchange rates"
                    );
                }
                Err(err) => {
                    tracing::debug!(
                        error = %err,
                        "background exchange rate refresh skipped (offline or unreachable); keeping cached rates"
                    );
                }
            }
        });
    if spawn_res.is_err() {
        FX_FETCH_IN_FLIGHT.store(false, Ordering::Release);
    }
}

fn format_relative_age(age_secs: i64) -> String {
    let secs = age_secs.max(0);
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3_600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3_600)
    } else {
        format!("{}d ago", secs / 86_400)
    }
}

/// Evaluation category shown as a crisp neutral/semantic tag in Root Search & Calculator Detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CalcCategory {
    Arithmetic,
    Base,
    Unit,
    Currency,
    Date,
    Timezone,
}

impl CalcCategory {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Arithmetic => "Calculator",
            Self::Base => "Base",
            Self::Unit => "Unit",
            Self::Currency => "Currency",
            Self::Date => "Date",
            Self::Timezone => "Timezone",
        }
    }

    /// Embedded monochrome category icon used for Recent Calculations rows so the primary
    /// Signal Copper `calculator.svg` logo stays visually focused on Live Result & Root Search.
    #[must_use]
    pub const fn history_icon_asset(self) -> &'static str {
        match self {
            Self::Arithmetic => "asset://icons/plugins/calc-arithmetic.svg",
            Self::Base => "asset://icons/plugins/calc-base.svg",
            Self::Unit => "asset://icons/plugins/calc-unit.svg",
            Self::Currency => "asset://icons/plugins/calc-currency.svg",
            Self::Date => "asset://icons/plugins/calc-date.svg",
            Self::Timezone => "asset://icons/plugins/calc-timezone.svg",
        }
    }

    /// Relative bundle icon path used by external WASM `com.aios.calculator` bundles.
    #[must_use]
    pub const fn relative_icon_path(self) -> &'static str {
        match self {
            Self::Arithmetic => "icons/calc-arithmetic.svg",
            Self::Base => "icons/calc-base.svg",
            Self::Unit => "icons/calc-unit.svg",
            Self::Currency => "icons/calc-currency.svg",
            Self::Date => "icons/calc-date.svg",
            Self::Timezone => "icons/calc-timezone.svg",
        }
    }

    #[must_use]
    pub const fn badge_fallback(self) -> &'static str {
        match self {
            Self::Arithmetic => "=",
            Self::Base => "0x",
            Self::Unit => "UN",
            Self::Currency => "$",
            Self::Date => "DT",
            Self::Timezone => "TZ",
        }
    }
}

/// Evaluated calculation result with rich secondary metadata for the Split Detail view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalcEvaluation {
    pub expression: String,
    pub formatted: String,
    pub copy_value: String,
    pub category: CalcCategory,
    pub detail_subtitle: String,
    pub notes: Vec<(String, String)>,
}

/// One persisted calculation history entry stored under `history_v1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryRecord {
    pub id: String,
    pub expression: String,
    pub result: String,
    pub category: CalcCategory,
    pub subtitle: String,
    pub timestamp: i64,
}

/// Inserts `eval` at the top of `history` (deduplicating identical expression+result, capped at 50).
pub fn push_history_record(history: &mut Vec<HistoryRecord>, eval: &CalcEvaluation) {
    history.retain(|h| !(h.expression == eval.expression && h.result == eval.copy_value));
    let ts = now_epoch_secs();
    history.insert(
        0,
        HistoryRecord {
            id: format!("h-{ts}-{}", history.len()),
            expression: eval.expression.clone(),
            result: eval.copy_value.clone(),
            category: eval.category,
            subtitle: eval.detail_subtitle.clone(),
            timestamp: ts,
        },
    );
    history.truncate(MAX_HISTORY_ENTRIES);
}

/// Builds the full `Calculator & Converter` interactive `List + Split Detail` view.
/// When `use_relative_icons` is `true` (external WASM bundle), emits `"icons/calc-*.svg"` so
/// `PluginHost` resolves icons relative to the bundle directory.
#[must_use]
pub fn build_calculator_list_view(
    query: &str,
    opts: &CalcOptions,
    history: &[HistoryRecord],
    use_relative_icons: bool,
) -> ViewNode {
    let trimmed = query.trim();
    let cleaned = trimmed.strip_prefix('=').map(str::trim).unwrap_or(trimmed);
    let icon_for = |cat: CalcCategory| -> &'static str {
        if use_relative_icons {
            cat.relative_icon_path()
        } else {
            cat.history_icon_asset()
        }
    };

    let mut builder = ListViewBuilder::new("Calculator & Converter")
        .placeholder("Type expression (e.g. 24 * 12, 0xff to bin, 100 cm to in, 50 usd to idr, today + 14d)...")
        .filtering(FilterMode::Plugin)
        .showing_detail(true);

    if !cleaned.is_empty() {
        match evaluate_expression(cleaned, opts) {
            Ok(eval) => {
                let mut md = format!(
                    "# {}\n\nExpression:\n\n```text\n{}\n```\n",
                    eval.formatted, eval.expression
                );
                if !eval.notes.is_empty() {
                    md.push_str("\n## Breakdown\n");
                    for (k, v) in &eval.notes {
                        md.push_str(&format!("\n- **{k}**: `{v}`"));
                    }
                }

                let mut metadata = vec![
                    MetadataEntry::TagList {
                        title: "Type".to_string(),
                        tags: vec![(eval.category.label().to_string(), SemanticTone::Accent)],
                    },
                    MetadataEntry::Label {
                        title: "Copy Value".to_string(),
                        text: eval.copy_value.clone(),
                    },
                ];
                for (k, v) in eval.notes.iter().take(4) {
                    metadata.push(MetadataEntry::Label {
                        title: k.clone(),
                        text: v.clone(),
                    });
                }

                let encoded = encode_calc_payload(&eval.expression, &eval.copy_value);
                let live_item = ListItemBuilder::new("calc-live-result", eval.formatted.clone())
                    .subtitle(format!("{}  ·  {}", eval.expression, eval.detail_subtitle))
                    .icon(eval.category.badge_fallback())
                    .icon_path(icon_for(eval.category))
                    .accessory_tag(eval.category.label(), SemanticTone::Accent)
                    .detail(md, metadata)
                    .actions(
                        ActionPanelBuilder::new()
                            .section(
                                Some("Result"),
                                vec![
                                    ActionItem::new(
                                        format!("calc.copy-save:{encoded}"),
                                        format!("Copy {}", eval.copy_value),
                                    )
                                    .shortcut("Enter"),
                                    ActionItem::new(
                                        format!("calc.copy-full:{encoded}"),
                                        "Copy Expression = Result",
                                    )
                                    .shortcut("Ctrl+Enter"),
                                    ActionItem::new(
                                        format!("calc.save:{encoded}"),
                                        "Save to History",
                                    ),
                                ],
                            )
                            .section(
                                Some("Calculator"),
                                vec![
                                    ActionItem::new(
                                        CALCULATOR_REFRESH_RATES_ACTION,
                                        "Refresh Exchange Rates Now",
                                    ),
                                    ActionItem::new("calc.clear-history", "Clear History"),
                                    ActionItem::new("nav.pop", "Back to Root").shortcut("Esc"),
                                ],
                            )
                            .build(),
                    )
                    .build();

                builder = builder.section(
                    Some("Live Result"),
                    Some(eval.category.label()),
                    vec![live_item],
                );
            }
            Err(err_msg) => {
                let hint_item = ListItemBuilder::new("calc-parse-error", err_msg.clone())
                    .subtitle(format!("Input: {cleaned}"))
                    .icon("!")
                    .accessory_tag("Hint", SemanticTone::Warning)
                    .detail(
                        format!(
                            "# Incomplete or invalid expression\n\n`{cleaned}`\n\n> {err_msg}\n\n## Supported Examples\n\n- Arithmetic: `24 * 12`, `(45 + 5) / 2`, `20% of 450`, `sqrt(144)`\n- Bases: `0xff to dec`, `255 to bin`, `0b1010 + 0x10`\n- Units: `100 cm to in`, `72 f to c`, `1500 ml to l`, `1.5 gib to mb`, `24 px to rem`\n- Currency (cached): `100 usd to idr`, `$49.99 to idr`\n- Date & TZ: `today + 14d`, `2026-12-31 - today`, `now in tokyo`, `14:30 wib to utc`"
                        ),
                        vec![MetadataEntry::TagList {
                            title: "Status".to_string(),
                            tags: vec![("Parse Hint".to_string(), SemanticTone::Warning)],
                        }],
                    )
                    .actions(ActionPanel::from_actions(vec![
                        ActionItem::new("search.clear", "Clear Input").shortcut("Enter"),
                    ]))
                    .build();
                builder = builder.section(Some("Expression"), None, vec![hint_item]);
            }
        }
    }

    if !history.is_empty() {
        let hist_items = history
            .iter()
            .map(|rec| {
                let encoded = encode_calc_payload(&rec.expression, &rec.result);
                ListItemBuilder::new(rec.id.clone(), rec.result.clone())
                    .subtitle(rec.expression.clone())
                    .icon(rec.category.badge_fallback())
                    .icon_path(icon_for(rec.category))
                    .accessory_tag(rec.category.label(), SemanticTone::Neutral)
                    .detail(
                        format!(
                            "# {}\n\n```text\n{} = {}\n```\n\n{}",
                            rec.result, rec.expression, rec.result, rec.subtitle
                        ),
                        vec![
                            MetadataEntry::TagList {
                                title: "Type".to_string(),
                                tags: vec![(
                                    rec.category.label().to_string(),
                                    SemanticTone::Neutral,
                                )],
                            },
                            MetadataEntry::Label {
                                title: "Expression".to_string(),
                                text: rec.expression.clone(),
                            },
                        ],
                    )
                    .actions(
                        ActionPanelBuilder::new()
                            .section(
                                Some("History Entry"),
                                vec![
                                    ActionItem::new(
                                        format!("calc.copy-raw:{}", rec.result),
                                        format!("Copy {}", rec.result),
                                    )
                                    .shortcut("Enter"),
                                    ActionItem::new(
                                        format!("calc.copy-full:{encoded}"),
                                        "Copy Expression = Result",
                                    )
                                    .shortcut("Ctrl+Enter"),
                                    ActionItem::new(
                                        format!("calc.delete:{}", rec.id),
                                        "Remove Entry",
                                    ),
                                    ActionItem::new(
                                        CALCULATOR_REFRESH_RATES_ACTION,
                                        "Refresh Exchange Rates Now",
                                    ),
                                    ActionItem::new("calc.clear-history", "Clear All History"),
                                ],
                            )
                            .build(),
                    )
                    .build()
            })
            .collect();
        let count_label = format!("{} saved", history.len());
        builder = builder.section(Some("Recent Calculations"), Some(&count_label), hist_items);
    }

    builder
        .empty_view(EmptyView {
            title: "Calculator & Converter Ready".to_string(),
            description: Some(
                "Try: 24 * 12, 20% of 450, 0xff to bin, 100 cm to in, 1500 ml to l, 24 px to rem, 50 usd to idr, today + 14d, or now in tokyo."
                    .to_string(),
            ),
            icon: Some("=".to_string()),
            actions: ActionPanel::from_actions(vec![
                ActionItem::new(
                    CALCULATOR_REFRESH_RATES_ACTION,
                    "Refresh Exchange Rates Now",
                ),
                ActionItem::new("nav.pop", "Back to Root").shortcut("Esc"),
            ]),
        })
        .build()
}

/// Evaluates an inline Root Search query into an [`InlinePluginResult`] (shared by both
/// external WASM and fallback native providers).
#[must_use]
pub fn evaluate_inline_with_opts(
    query: &str,
    opts: &CalcOptions,
    icon_path: &str,
) -> Option<(InlinePluginResult, CalcCategory)> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (exclusive, expr) = if let Some(rest) = trimmed.strip_prefix('=') {
        (true, rest.trim())
    } else {
        if !might_be_calculator_query(trimmed) {
            return None;
        }
        (false, trimmed)
    };
    if expr.is_empty() {
        return None;
    }

    let eval = evaluate_expression(expr, opts).ok()?;

    // Avoid noisy inline cards for trivial bare numbers like "42" unless explicit `=` prefix was used.
    if !exclusive
        && eval.category == CalcCategory::Arithmetic
        && !has_math_operator_or_func(expr)
    {
        return None;
    }

    let category = eval.category;
    let encoded = encode_calc_payload(&eval.expression, &eval.copy_value);
    Some((
        InlinePluginResult {
            id: format!("inline:calc:{}", eval.copy_value),
            plugin_id: CALCULATOR_PLUGIN_ID.to_string(),
            command_id: CALCULATOR_COMMAND_ID.to_string(),
            section_title: "Calculator".to_string(),
            section_subtitle: Some(eval.category.label().to_string()),
            title: eval.formatted,
            subtitle: format!("{}  ·  {}", eval.expression, eval.detail_subtitle),
            badge: "=".to_string(),
            icon_path: Some(icon_path.to_owned()),
            tag_label: eval.category.label().to_string(),
            copy_value: eval.copy_value,
            copy_action_id: format!("plugin.inline:{CALCULATOR_PLUGIN_ID}:copy:{encoded}"),
            open_action_id: format!(
                "plugin.run:{CALCULATOR_PLUGIN_ID}:{CALCULATOR_COMMAND_ID}"
            ),
            open_action_title: "Open Calculator & History".to_string(),
            exclusive,
        },
        category,
    ))
}

/// First-party Inline Calculator & Converter fallback plugin (shadowed automatically when
/// external WASM `com.aios.calculator` bundle is installed/bundled).
#[cfg(feature = "host")]
#[derive(Debug, Default)]
pub struct CalculatorPlugin;

#[cfg(feature = "host")]
impl CalculatorPlugin {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    fn load_history(env: &PluginEnv<'_>) -> Vec<HistoryRecord> {
        env.storage_get(HISTORY_STORAGE_KEY)
            .and_then(|bytes| serde_json::from_slice::<Vec<HistoryRecord>>(&bytes).ok())
            .unwrap_or_default()
    }

    fn save_history(env: &PluginEnv<'_>, records: &[HistoryRecord]) {
        if let Ok(bytes) = serde_json::to_vec(records) {
            let _ = env.storage_set(HISTORY_STORAGE_KEY, &bytes);
        }
    }

    fn record_history(env: &PluginEnv<'_>, eval: &CalcEvaluation) {
        let mut history = Self::load_history(env);
        push_history_record(&mut history, eval);
        Self::save_history(env, &history);
    }

    fn build_view(query: &str, env: &PluginEnv<'_>) -> ViewNode {
        let opts = CalcOptions::from_env(env);
        let history = Self::load_history(env);
        if query.trim().trim_start_matches('=').trim().is_empty() {
            maybe_schedule_stale_currency_refresh(&env.db_arc());
        }
        build_calculator_list_view(query, &opts, &history, false)
    }
}

#[cfg(feature = "host")]
impl FirstPartyPlugin for CalculatorPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: CALCULATOR_PLUGIN_ID.to_string(),
            name: "Calculator & Converter".to_string(),
            version: "0.1.0".to_string(),
            description: "Inline math, hex/bin/octal, unit, CSS, date/timezone, and offline currency converter.".to_string(),
            author: "AIOS".to_string(),
            min_aios_version: Some("0.1.0".to_string()),
            icon: Some(CALCULATOR_ICON_ASSET.to_string()),
            entry: "builtin:calculator".to_string(),
            capabilities: PluginCapabilities {
                clipboard_write: true,
                ..PluginCapabilities::default()
            },
            inline_trigger: aios_plugin_sdk::InlineTriggerSpec::default(),
            preferences: vec![
                PreferenceSpec {
                    name: "base_font_px".to_string(),
                    title: "CSS Base Font Size (px)".to_string(),
                    description: Some("Root font size used for px <-> rem/em conversions (default: 16).".to_string()),
                    kind: PreferenceKind::Textfield,
                    default: Some("16".to_string()),
                    required: false,
                    options: Vec::new(),
                },
                PreferenceSpec {
                    name: "decimal_places".to_string(),
                    title: "Max Decimal Places".to_string(),
                    description: Some("Maximum decimal precision for floating-point results (0..12, default: 6).".to_string()),
                    kind: PreferenceKind::Textfield,
                    default: Some("6".to_string()),
                    required: false,
                    options: Vec::new(),
                },
                PreferenceSpec {
                    name: "default_currency".to_string(),
                    title: "Default Currency".to_string(),
                    description: Some("Target currency when converting bare symbol amounts like $50 (default: IDR).".to_string()),
                    kind: PreferenceKind::Textfield,
                    default: Some("IDR".to_string()),
                    required: false,
                    options: Vec::new(),
                },
                PreferenceSpec {
                    name: "currency_refresh_interval".to_string(),
                    title: "Exchange Rate Auto-Refresh".to_string(),
                    description: Some("Offline-first cache TTL before background refresh (off, 6h, 12h, 24h, 3d, 7d; default: 24h).".to_string()),
                    kind: PreferenceKind::Textfield,
                    default: Some("24h".to_string()),
                    required: false,
                    options: Vec::new(),
                },
            ],
            commands: vec![CommandSpec {
                id: CALCULATOR_COMMAND_ID.to_string(),
                title: "Calculator & Converter".to_string(),
                subtitle: Some("Math, bases, units, date/timezone & offline currency".to_string()),
                mode: CommandMode::View,
                default_view: DefaultViewKind::List,
                needs_confirmation: false,
                keywords: vec![
                    "calc".to_string(),
                    "calculator".to_string(),
                    "math".to_string(),
                    "convert".to_string(),
                    "unit".to_string(),
                    "currency".to_string(),
                    "hex".to_string(),
                    "bin".to_string(),
                    "timezone".to_string(),
                    "date".to_string(),
                ],
                arguments: Vec::new(),
                refresh_interval_secs: None,
            }],
        }
    }

    fn badge(&self) -> &'static str {
        "="
    }

    fn icon_asset(&self) -> Option<&'static str> {
        Some(CALCULATOR_ICON_ASSET)
    }

    fn init_command(
        &self,
        _command_id: &str,
        args: &[String],
        env: &PluginEnv<'_>,
    ) -> PluginResponse {
        let initial_query = args.first().map(String::as_str).unwrap_or("");
        PluginResponse::render(Self::build_view(initial_query, env))
    }

    fn handle_event(
        &self,
        _command_id: &str,
        event: &PluginEvent,
        env: &PluginEnv<'_>,
    ) -> PluginResponse {
        match event {
            PluginEvent::SearchTextChanged { query, .. } => {
                PluginResponse::render(Self::build_view(query, env))
            }
            PluginEvent::ActionTriggered { action_id, .. } => {
                if let Some(encoded) = action_id.strip_prefix("calc.copy-save:")
                    && let Some((expr, result)) = decode_calc_payload(encoded)
                {
                    let opts = CalcOptions::from_env(env);
                    if let Ok(eval) = evaluate_expression(&expr, &opts) {
                        Self::record_history(env, &eval);
                    }
                    return PluginResponse::CopyToClipboard {
                        text: result.clone(),
                        hud_title: Some(format!("Copied {result}")),
                    };
                }
                if let Some(encoded) = action_id.strip_prefix("calc.copy-full:")
                    && let Some((expr, result)) = decode_calc_payload(encoded)
                {
                    let opts = CalcOptions::from_env(env);
                    if let Ok(eval) = evaluate_expression(&expr, &opts) {
                        Self::record_history(env, &eval);
                    }
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
                    let opts = CalcOptions::from_env(env);
                    if let Ok(eval) = evaluate_expression(&expr, &opts) {
                        Self::record_history(env, &eval);
                    }
                    return PluginResponse::render(Self::build_view(&expr, env));
                }
                if let Some(del_id) = action_id.strip_prefix("calc.delete:") {
                    let mut history = Self::load_history(env);
                    history.retain(|h| h.id != del_id);
                    Self::save_history(env, &history);
                    return PluginResponse::render(Self::build_view("", env));
                }
                if action_id == "calc.clear-history" {
                    let _ = env.storage_remove(HISTORY_STORAGE_KEY);
                    return PluginResponse::render(Self::build_view("", env));
                }
                PluginResponse::Noop
            }
            _ => PluginResponse::Noop,
        }
    }

    fn evaluate_inline_query(
        &self,
        query: &str,
        env: &PluginEnv<'_>,
    ) -> Option<InlinePluginResult> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return None;
        }
        let (exclusive, expr) = if let Some(rest) = trimmed.strip_prefix('=') {
            (true, rest.trim())
        } else {
            if !might_be_calculator_query(trimmed) {
                return None;
            }
            (false, trimmed)
        };
        if expr.is_empty() {
            return None;
        }

        let opts = CalcOptions::from_env(env);
        let eval = evaluate_expression(expr, &opts).ok()?;
        if eval.category == CalcCategory::Currency {
            maybe_schedule_stale_currency_refresh(&env.db_arc());
        }

        // Avoid noisy inline cards for trivial bare numbers like "42" unless explicit `=` prefix was used.
        if !exclusive
            && eval.category == CalcCategory::Arithmetic
            && !has_math_operator_or_func(expr)
        {
            return None;
        }

        let encoded = encode_calc_payload(&eval.expression, &eval.copy_value);
        Some(InlinePluginResult {
            id: format!("inline:calc:{}", eval.copy_value),
            plugin_id: CALCULATOR_PLUGIN_ID.to_string(),
            command_id: CALCULATOR_COMMAND_ID.to_string(),
            section_title: "Calculator".to_string(),
            section_subtitle: Some(eval.category.label().to_string()),
            title: eval.formatted,
            subtitle: format!("{}  ·  {}", eval.expression, eval.detail_subtitle),
            badge: "=".to_string(),
            icon_path: Some(CALCULATOR_ICON_ASSET.to_string()),
            tag_label: eval.category.label().to_string(),
            copy_value: eval.copy_value,
            copy_action_id: format!("plugin.inline:{CALCULATOR_PLUGIN_ID}:copy:{encoded}"),
            open_action_id: format!(
                "plugin.run:{CALCULATOR_PLUGIN_ID}:{CALCULATOR_COMMAND_ID}"
            ),
            open_action_title: "Open Calculator & History".to_string(),
            exclusive,
        })
    }

    fn handle_inline_action(
        &self,
        action_id: &str,
        env: &PluginEnv<'_>,
    ) -> Option<PluginResponse> {
        let encoded = action_id.strip_prefix("copy:")?;
        let (expr, result) = decode_calc_payload(encoded)?;
        let opts = CalcOptions::from_env(env);
        if let Ok(eval) = evaluate_expression(&expr, &opts) {
            Self::record_history(env, &eval);
        }
        Some(PluginResponse::CopyToClipboard {
            text: result.clone(),
            hud_title: Some(format!("Copied {result}")),
        })
    }
}

#[must_use]
pub fn encode_calc_payload(expr: &str, result: &str) -> String {
    serde_json::to_string(&(expr, result)).unwrap_or_default()
}

#[must_use]
pub fn decode_calc_payload(raw: &str) -> Option<(String, String)> {
    serde_json::from_str::<(String, String)>(raw).ok()
}

/// Sub-microsecond fast-gate ensuring normal app searches (`"firefox"`, `"settings"`, `"code"`)
/// return `false` with zero allocations before any parser runs.
#[must_use]
pub fn might_be_calculator_query(raw: &str) -> bool {
    let s = raw.trim();
    if s.is_empty() {
        return false;
    }
    if s.starts_with('=') {
        return true;
    }
    let bytes = s.as_bytes();
    let first = bytes[0];
    if matches!(first, b'$' | b'(' | b'[' | b'-' | b'+')
        || s.starts_with('€')
        || s.starts_with('£')
        || s.starts_with('¥')
        || s.starts_with('π')
    {
        return true;
    }
    if first.is_ascii_digit() {
        // Require either a math/base/unit/conversion indicator so plain numbers like "2026" don't spam.
        return s.bytes().any(|b| {
            matches!(
                b,
                b'+' | b'-' | b'*' | b'/' | b'%' | b'^' | b'!' | b'&' | b'|' | b'<' | b'>' | b'x' | b'X' | b'b' | b'B' | b'o' | b'O' | b' ' | b'a'..=b'z' | b'A'..=b'Z'
            )
        });
    }
    let lower = s.to_ascii_lowercase();
    lower.starts_with("today")
        || lower.starts_with("tomorrow")
        || lower.starts_with("yesterday")
        || lower.starts_with("now ")
        || lower == "now"
        || lower.starts_with("sqrt(")
        || lower.starts_with("sin(")
        || lower.starts_with("cos(")
        || lower.starts_with("tan(")
        || lower.starts_with("log(")
        || lower.starts_with("ln(")
        || lower.starts_with("abs(")
        || lower.starts_with("pow(")
        || lower.starts_with("min(")
        || lower.starts_with("max(")
        || lower.starts_with("round(")
        || lower.starts_with("floor(")
        || lower.starts_with("ceil(")
}

fn has_math_operator_or_func(expr: &str) -> bool {
    let lower = expr.to_ascii_lowercase();
    if lower.starts_with("0x") || lower.starts_with("0b") || lower.starts_with("0o") {
        return true;
    }
    expr.bytes().any(|b| {
        matches!(
            b,
            b'+' | b'-' | b'*' | b'/' | b'%' | b'^' | b'!' | b'(' | b'&' | b'|' | b'<' | b'>'
        )
    }) || lower.contains(" of ")
        || lower.contains(" xor ")
        || matches!(lower.as_str(), "pi" | "π" | "e" | "tau" | "phi")
}

/// Evaluates any supported Calculator, Base, Unit, Currency, Date, or Timezone expression.
pub fn evaluate_expression(raw: &str, opts: &CalcOptions) -> Result<CalcEvaluation, String> {
    let expr = raw.trim().strip_prefix('=').map(str::trim).unwrap_or(raw.trim());
    if expr.is_empty() {
        return Err("Empty expression".to_string());
    }

    if let Some(res) = try_evaluate_date_or_timezone(expr) {
        return Ok(res);
    }
    if let Some(res) = try_evaluate_base_conversion(expr) {
        return Ok(res);
    }
    if let Some(res) = try_evaluate_currency(expr, opts) {
        return Ok(res);
    }
    if let Some(res) = try_evaluate_unit_conversion(expr, opts) {
        return Ok(res);
    }
    if let Some(res) = try_evaluate_percentage_phrase(expr, opts) {
        return Ok(res);
    }

    let val = eval_math(expr)?;
    if !val.is_finite() {
        return Err("Result is not finite".to_string());
    }
    let formatted = format_number(val, opts.decimal_places);
    let mut notes = Vec::new();
    let is_base_literal = {
        let lower = expr.to_ascii_lowercase();
        lower.contains("0x") || lower.contains("0b") || lower.contains("0o")
    };
    if val.fract().abs() < 1e-11 && val.abs() <= (i64::MAX as f64) {
        let iv = val.round() as i64;
        if iv >= 0 {
            let u = iv as u64;
            notes.push(("Hex".to_string(), format!("0x{u:X}")));
            notes.push(("Binary".to_string(), format!("0b{u:b}")));
            notes.push(("Octal".to_string(), format!("0o{u:o}")));
        }
    }

    let (category, detail_subtitle) = if is_base_literal {
        (CalcCategory::Base, "Base literal -> Decimal".to_string())
    } else {
        (CalcCategory::Arithmetic, "Arithmetic".to_string())
    };

    Ok(CalcEvaluation {
        expression: expr.to_string(),
        formatted: formatted.clone(),
        copy_value: formatted,
        category,
        detail_subtitle,
        notes,
    })
}

fn format_number(val: f64, max_decimals: usize) -> String {
    if val.abs() < 1e-12 {
        return "0".to_string();
    }
    // Snap near-integers (eliminates 0.1 + 0.2 = 0.30000000000000004 and 3.0000000000000004).
    let rounded_int = val.round();
    if (val - rounded_int).abs() < 1e-10 && val.abs() < 1e15 {
        return format!("{}", rounded_int as i64);
    }
    let formatted = format!("{val:.max_decimals$}");
    if formatted.contains('.') {
        let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
        if trimmed == "-0" {
            "0".to_string()
        } else {
            trimmed.to_string()
        }
    } else {
        formatted
    }
}

fn format_grouped_currency(val: f64, decimals: usize) -> String {
    let neg = val < 0.0;
    let abs = val.abs();
    let s = format!("{abs:.decimals$}");
    let (int_part, frac_part) = match s.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (s.as_str(), None),
    };
    let mut grouped = String::with_capacity(int_part.len() + int_part.len() / 3);
    for (idx, ch) in int_part.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let int_grouped: String = grouped.chars().rev().collect();
    let sign = if neg { "-" } else { "" };
    match frac_part {
        Some(f) => format!("{sign}{int_grouped}.{f}"),
        None => format!("{sign}{int_grouped}"),
    }
}

// ============================================================================
// 1. Percentage phrases (`20% of 450`, `100 + 15%`, `200 - 10%`)
// ============================================================================

fn try_evaluate_percentage_phrase(expr: &str, opts: &CalcOptions) -> Option<CalcEvaluation> {
    let lower = expr.to_ascii_lowercase();
    if let Some((left, right)) = split_keyword_ci(&lower, expr, " of ") {
        let pct_str = left.trim().strip_suffix('%')?.trim();
        let pct = eval_math(pct_str).ok()?;
        let base = eval_math(right.trim()).ok()?;
        let result = (pct / 100.0) * base;
        let formatted = format_number(result, opts.decimal_places);
        return Some(CalcEvaluation {
            expression: expr.to_string(),
            formatted: formatted.clone(),
            copy_value: formatted,
            category: CalcCategory::Arithmetic,
            detail_subtitle: format!("{pct}% of {}", format_number(base, opts.decimal_places)),
            notes: vec![
                ("Percentage".to_string(), format!("{pct}%")),
                ("Base".to_string(), format_number(base, opts.decimal_places)),
            ],
        });
    }

    // `base + X%` or `base - X%`
    if let Some(stripped) = expr.trim().strip_suffix('%') {
        for (op, is_add) in [('+', true), ('-', false)] {
            if let Some(pos) = stripped.rfind(op)
                && pos > 0
            {
                let lhs_str = stripped[..pos].trim();
                let rhs_str = stripped[pos + 1..].trim();
                if let (Ok(base), Ok(pct)) = (eval_math(lhs_str), eval_math(rhs_str)) {
                    let delta = base * (pct / 100.0);
                    let result = if is_add { base + delta } else { base - delta };
                    let formatted = format_number(result, opts.decimal_places);
                    return Some(CalcEvaluation {
                        expression: expr.to_string(),
                        formatted: formatted.clone(),
                        copy_value: formatted,
                        category: CalcCategory::Arithmetic,
                        detail_subtitle: format!(
                            "{} {} {pct}% ({})",
                            format_number(base, opts.decimal_places),
                            if is_add { "+" } else { "-" },
                            format_number(delta, opts.decimal_places)
                        ),
                        notes: vec![
                            ("Delta".to_string(), format_number(delta, opts.decimal_places)),
                        ],
                    });
                }
            }
        }
    }
    None
}

// ============================================================================
// 2. Programmer Base Conversions (`255 to hex`, `0xff in bin`, `0b1010 to dec`)
// ============================================================================

fn try_evaluate_base_conversion(expr: &str) -> Option<CalcEvaluation> {
    let lower = expr.to_ascii_lowercase();
    let (lhs, target_raw) = split_conversion_ci(&lower, expr)?;
    let target = target_raw.trim().to_ascii_lowercase();
    let radix = match target.as_str() {
        "hex" | "hexadecimal" | "base16" => 16u32,
        "bin" | "binary" | "base2" => 2u32,
        "oct" | "octal" | "base8" => 8u32,
        "dec" | "decimal" | "base10" => 10u32,
        _ => return None,
    };

    let val = eval_math(lhs.trim()).ok()?;
    if !val.is_finite() || val.fract().abs() > 1e-9 || val < 0.0 || val > (u64::MAX as f64) {
        return None;
    }
    let u = val.round() as u64;
    let formatted = match radix {
        16 => format!("0x{u:X}"),
        2 => format!("0b{u:b}"),
        8 => format!("0o{u:o}"),
        _ => format!("{u}"),
    };
    let notes = vec![
        ("Decimal".to_string(), format!("{u}")),
        ("Hex".to_string(), format!("0x{u:X}")),
        ("Binary".to_string(), format!("0b{u:b}")),
        ("Octal".to_string(), format!("0o{u:o}")),
    ];

    Some(CalcEvaluation {
        expression: expr.to_string(),
        formatted: formatted.clone(),
        copy_value: formatted,
        category: CalcCategory::Base,
        detail_subtitle: format!("Base-{radix} conversion"),
        notes,
    })
}

// ============================================================================
// 3. Unit Converter (Length, Weight, Temp, Data, Time, Speed, Area, CSS)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitDim {
    Length,
    Weight,
    Temp,
    Data,
    Time,
    Speed,
    Area,
    Volume,
    Css,
}

#[derive(Debug, Clone, Copy)]
struct UnitDef {
    dim: UnitDim,
    canonical: &'static str,
    /// Factor to dimension base unit (meters, grams, bytes, seconds, m/s, m², px).
    to_base: f64,
}

fn lookup_unit(raw: &str, base_font_px: f64) -> Option<UnitDef> {
    let trimmed = raw.trim();
    // Case-sensitive distinction for `b` (bit) vs `B` (byte).
    if trimmed == "B" {
        return Some(UnitDef {
            dim: UnitDim::Data,
            canonical: "B",
            to_base: 1.0,
        });
    }
    let s = trimmed.to_ascii_lowercase();
    let def = match s.as_str() {
        // Length (base: meter)
        "mm" | "millimeter" | "millimeters" => UnitDef { dim: UnitDim::Length, canonical: "mm", to_base: 0.001 },
        "cm" | "centimeter" | "centimeters" => UnitDef { dim: UnitDim::Length, canonical: "cm", to_base: 0.01 },
        "m" | "meter" | "meters" | "metre" | "metres" => UnitDef { dim: UnitDim::Length, canonical: "m", to_base: 1.0 },
        "km" | "kilometer" | "kilometers" => UnitDef { dim: UnitDim::Length, canonical: "km", to_base: 1000.0 },
        "in" | "inch" | "inches" => UnitDef { dim: UnitDim::Length, canonical: "in", to_base: 0.0254 },
        "ft" | "foot" | "feet" => UnitDef { dim: UnitDim::Length, canonical: "ft", to_base: 0.3048 },
        "yd" | "yard" | "yards" => UnitDef { dim: UnitDim::Length, canonical: "yd", to_base: 0.9144 },
        "mi" | "mile" | "miles" => UnitDef { dim: UnitDim::Length, canonical: "mi", to_base: 1609.344 },
        "nmi" => UnitDef { dim: UnitDim::Length, canonical: "nmi", to_base: 1852.0 },

        // Weight / Mass (base: gram)
        "mg" | "milligram" | "milligrams" => UnitDef { dim: UnitDim::Weight, canonical: "mg", to_base: 0.001 },
        "g" | "gram" | "grams" => UnitDef { dim: UnitDim::Weight, canonical: "g", to_base: 1.0 },
        "kg" | "kilogram" | "kilograms" => UnitDef { dim: UnitDim::Weight, canonical: "kg", to_base: 1000.0 },
        "t" | "tonne" | "tonnes" => UnitDef { dim: UnitDim::Weight, canonical: "t", to_base: 1_000_000.0 },
        "oz" | "ounce" | "ounces" => UnitDef { dim: UnitDim::Weight, canonical: "oz", to_base: 28.349_523_125 },
        "lb" | "lbs" | "pound" | "pounds" => UnitDef { dim: UnitDim::Weight, canonical: "lb", to_base: 453.592_37 },

        // Temperature (affine)
        "c" | "°c" | "celsius" => UnitDef { dim: UnitDim::Temp, canonical: "°C", to_base: 1.0 },
        "f" | "°f" | "fahrenheit" => UnitDef { dim: UnitDim::Temp, canonical: "°F", to_base: 2.0 },
        "k" | "kelvin" => UnitDef { dim: UnitDim::Temp, canonical: "K", to_base: 3.0 },

        // Data storage (base: byte)
        "b" | "bit" | "bits" => UnitDef { dim: UnitDim::Data, canonical: "bit", to_base: 0.125 },
        "byte" | "bytes" => UnitDef { dim: UnitDim::Data, canonical: "B", to_base: 1.0 },
        "kb" => UnitDef { dim: UnitDim::Data, canonical: "KB", to_base: 1_000.0 },
        "mb" => UnitDef { dim: UnitDim::Data, canonical: "MB", to_base: 1_000_000.0 },
        "gb" => UnitDef { dim: UnitDim::Data, canonical: "GB", to_base: 1_000_000_000.0 },
        "tb" => UnitDef { dim: UnitDim::Data, canonical: "TB", to_base: 1_000_000_000_000.0 },
        "pb" => UnitDef { dim: UnitDim::Data, canonical: "PB", to_base: 1_000_000_000_000_000.0 },
        "kib" => UnitDef { dim: UnitDim::Data, canonical: "KiB", to_base: 1_024.0 },
        "mib" => UnitDef { dim: UnitDim::Data, canonical: "MiB", to_base: 1_048_576.0 },
        "gib" => UnitDef { dim: UnitDim::Data, canonical: "GiB", to_base: 1_073_741_824.0 },
        "tib" => UnitDef { dim: UnitDim::Data, canonical: "TiB", to_base: 1_099_511_627_776.0 },

        // Time (base: second)
        "ms" | "millisecond" | "milliseconds" => UnitDef { dim: UnitDim::Time, canonical: "ms", to_base: 0.001 },
        "s" | "sec" | "secs" | "second" | "seconds" => UnitDef { dim: UnitDim::Time, canonical: "s", to_base: 1.0 },
        "min" | "mins" | "minute" | "minutes" => UnitDef { dim: UnitDim::Time, canonical: "min", to_base: 60.0 },
        "h" | "hr" | "hrs" | "hour" | "hours" => UnitDef { dim: UnitDim::Time, canonical: "h", to_base: 3600.0 },
        "d" | "day" | "days" => UnitDef { dim: UnitDim::Time, canonical: "d", to_base: 86_400.0 },
        "wk" | "week" | "weeks" => UnitDef { dim: UnitDim::Time, canonical: "wk", to_base: 604_800.0 },
        "yr" | "year" | "years" => UnitDef { dim: UnitDim::Time, canonical: "yr", to_base: 31_557_600.0 },

        // Speed (base: m/s)
        "m/s" | "mps" => UnitDef { dim: UnitDim::Speed, canonical: "m/s", to_base: 1.0 },
        "km/h" | "kph" | "kmh" => UnitDef { dim: UnitDim::Speed, canonical: "km/h", to_base: 1.0 / 3.6 },
        "mph" => UnitDef { dim: UnitDim::Speed, canonical: "mph", to_base: 0.44704 },
        "kn" | "knot" | "knots" => UnitDef { dim: UnitDim::Speed, canonical: "kn", to_base: 1852.0 / 3600.0 },

        // Area (base: m²)
        "mm2" | "mm²" => UnitDef { dim: UnitDim::Area, canonical: "mm²", to_base: 1e-6 },
        "cm2" | "cm²" => UnitDef { dim: UnitDim::Area, canonical: "cm²", to_base: 1e-4 },
        "m2" | "m²" | "sqm" => UnitDef { dim: UnitDim::Area, canonical: "m²", to_base: 1.0 },
        "km2" | "km²" => UnitDef { dim: UnitDim::Area, canonical: "km²", to_base: 1e6 },
        "ha" | "hectare" | "hectares" => UnitDef { dim: UnitDim::Area, canonical: "ha", to_base: 1e4 },
        "sqft" | "ft2" | "ft²" => UnitDef { dim: UnitDim::Area, canonical: "ft²", to_base: 0.092_903_04 },
        "acre" | "acres" => UnitDef { dim: UnitDim::Area, canonical: "acre", to_base: 4_046.856_422_4 },

        // Volume (base: liter)
        "ml" | "milliliter" | "milliliters" | "millilitre" | "millilitres" | "cc" | "cm3" | "cm³" => {
            UnitDef { dim: UnitDim::Volume, canonical: "mL", to_base: 0.001 }
        }
        "cl" | "centiliter" | "centiliters" | "centilitre" | "centilitres" => {
            UnitDef { dim: UnitDim::Volume, canonical: "cL", to_base: 0.01 }
        }
        "dl" | "deciliter" | "deciliters" | "decilitre" | "decilitres" => {
            UnitDef { dim: UnitDim::Volume, canonical: "dL", to_base: 0.1 }
        }
        "l" | "liter" | "liters" | "litre" | "litres" | "dm3" | "dm³" => {
            UnitDef { dim: UnitDim::Volume, canonical: "L", to_base: 1.0 }
        }
        "m3" | "m³" | "cbm" => UnitDef { dim: UnitDim::Volume, canonical: "m³", to_base: 1_000.0 },
        "tsp" | "teaspoon" | "teaspoons" => {
            UnitDef { dim: UnitDim::Volume, canonical: "tsp", to_base: 3.785_411_784 / 768.0 }
        }
        "tbsp" | "tablespoon" | "tablespoons" => {
            UnitDef { dim: UnitDim::Volume, canonical: "tbsp", to_base: 3.785_411_784 / 256.0 }
        }
        "floz" | "fl oz" | "fl_oz" | "fl-oz" => {
            UnitDef { dim: UnitDim::Volume, canonical: "fl oz", to_base: 3.785_411_784 / 128.0 }
        }
        "cup" | "cups" => {
            UnitDef { dim: UnitDim::Volume, canonical: "cup", to_base: 3.785_411_784 / 16.0 }
        }
        "pint" | "pints" => {
            UnitDef { dim: UnitDim::Volume, canonical: "pint", to_base: 3.785_411_784 / 8.0 }
        }
        "qt" | "quart" | "quarts" => {
            UnitDef { dim: UnitDim::Volume, canonical: "qt", to_base: 3.785_411_784 / 4.0 }
        }
        "gal" | "gallon" | "gallons" => {
            UnitDef { dim: UnitDim::Volume, canonical: "gal", to_base: 3.785_411_784 }
        }
        "in3" | "in³" | "cuin" => {
            UnitDef { dim: UnitDim::Volume, canonical: "in³", to_base: 0.016_387_064 }
        }
        "ft3" | "ft³" | "cuft" => {
            UnitDef { dim: UnitDim::Volume, canonical: "ft³", to_base: 28.316_846_592 }
        }

        // CSS (base: px)
        "px" => UnitDef { dim: UnitDim::Css, canonical: "px", to_base: 1.0 },
        "rem" => UnitDef { dim: UnitDim::Css, canonical: "rem", to_base: base_font_px },
        "em" => UnitDef { dim: UnitDim::Css, canonical: "em", to_base: base_font_px },
        "pt" => UnitDef { dim: UnitDim::Css, canonical: "pt", to_base: 96.0 / 72.0 },

        _ => return None,
    };
    Some(def)
}

fn try_evaluate_unit_conversion(expr: &str, opts: &CalcOptions) -> Option<CalcEvaluation> {
    let lower = expr.to_ascii_lowercase();
    let (lhs, rhs) = split_conversion_ci(&lower, expr)?;
    let (val_str, src_unit_str) = split_amount_and_unit(lhs.trim())?;
    let val = eval_math(val_str).ok()?;
    let src = lookup_unit(src_unit_str, opts.base_font_px)?;
    let dst = lookup_unit(rhs.trim(), opts.base_font_px)?;
    if src.dim != dst.dim {
        return None;
    }

    let converted = if src.dim == UnitDim::Temp {
        let celsius = match src.canonical {
            "°C" => val,
            "°F" => (val - 32.0) * 5.0 / 9.0,
            "K" => val - 273.15,
            _ => return None,
        };
        match dst.canonical {
            "°C" => celsius,
            "°F" => celsius * 9.0 / 5.0 + 32.0,
            "K" => celsius + 273.15,
            _ => return None,
        }
    } else {
        (val * src.to_base) / dst.to_base
    };

    let num_str = format_number(converted, opts.decimal_places);
    let formatted = format!("{num_str} {}", dst.canonical);
    let mut notes = vec![
        (
            "From".to_string(),
            format!("{} {}", format_number(val, opts.decimal_places), src.canonical),
        ),
        ("To".to_string(), formatted.clone()),
    ];
    if src.dim == UnitDim::Css {
        notes.push(("Base Font".to_string(), format!("{}px", opts.base_font_px)));
    }

    Some(CalcEvaluation {
        expression: expr.to_string(),
        formatted,
        copy_value: num_str,
        category: CalcCategory::Unit,
        detail_subtitle: format!("{} -> {}", src.canonical, dst.canonical),
        notes,
    })
}

// ============================================================================
// 4. Cached Offline Currency Converter (`100 usd to idr`, `$49.99 to idr`)
// ============================================================================

/// Built-in offline USD reference rates (approximate, clearly labeled as offline reference).
const OFFLINE_USD_RATES: &[(&str, f64, usize)] = &[
    ("USD", 1.0, 2),
    ("IDR", 16_200.0, 0),
    ("EUR", 0.92, 2),
    ("GBP", 0.79, 2),
    ("JPY", 154.0, 0),
    ("SGD", 1.34, 2),
    ("MYR", 4.42, 2),
    ("AUD", 1.54, 2),
    ("CAD", 1.39, 2),
    ("CHF", 0.88, 2),
    ("CNY", 7.24, 2),
    ("KRW", 1_390.0, 0),
    ("HKD", 7.78, 2),
    ("INR", 84.4, 2),
    ("THB", 34.5, 2),
    ("VND", 25_400.0, 0),
    ("PHP", 58.6, 2),
    ("NZD", 1.69, 2),
    ("SEK", 10.85, 2),
    ("NOK", 11.05, 2),
    ("BRL", 5.80, 2),
    ("MXN", 20.1, 2),
    ("AED", 3.67, 2),
    ("SAR", 3.75, 2),
    ("BTC", 0.000_010_5, 6),
    ("ETH", 0.000_31, 5),
];

fn lookup_currency_rate(code: &str, custom: &HashMap<String, f64>) -> Option<(&'static str, f64, usize)> {
    let upper = code.trim().to_ascii_uppercase();
    for &(sym, default_rate, decs) in OFFLINE_USD_RATES {
        if sym == upper {
            let rate = custom.get(sym).copied().unwrap_or(default_rate);
            return Some((sym, rate, decs));
        }
    }
    None
}

fn currency_source_metadata(
    src_sym: &str,
    dst_sym: &str,
    unit_rate: f64,
    opts: &CalcOptions,
) -> (String, Vec<(String, String)>) {
    let mut notes = vec![(
        "Rate".to_string(),
        format!("1 {src_sym} = {} {dst_sym}", format_number(unit_rate, 4)),
    )];
    if let (Some(source), Some(date_label)) = (&opts.rates_source, &opts.rates_date_label) {
        let subtitle = format!("{src_sym} -> {dst_sym} (cached {date_label})");
        notes.push(("Source".to_string(), format!("{source} ({date_label})")));
        if let Some(updated_at) = opts.rates_updated_at {
            notes.push((
                "Updated".to_string(),
                format_relative_age(now_epoch_secs().saturating_sub(updated_at)),
            ));
        }
        (subtitle, notes)
    } else {
        let subtitle = format!("{src_sym} -> {dst_sym} (offline fallback)");
        notes.push(("Source".to_string(), "Built-in offline fallback".to_string()));
        (subtitle, notes)
    }
}

fn try_evaluate_currency(expr: &str, opts: &CalcOptions) -> Option<CalcEvaluation> {
    let lower = expr.to_ascii_lowercase();

    // Case A: explicit conversion (`100 usd to idr`, `$49.99 in idr`, `€20 to usd`)
    if let Some((lhs, rhs)) = split_conversion_ci(&lower, expr) {
        let (amount, src_code) = parse_currency_amount(lhs.trim())?;
        let (src_sym, src_rate, _) = lookup_currency_rate(&src_code, &opts.custom_rates_usd)?;
        let (dst_sym, dst_rate, dst_decs) = lookup_currency_rate(rhs.trim(), &opts.custom_rates_usd)?;
        let usd = amount / src_rate;
        let result = usd * dst_rate;
        let formatted_num = format_grouped_currency(result, dst_decs);
        let formatted = format!("{formatted_num} {dst_sym}");
        let copy_value = format_number(result, dst_decs);
        let unit_rate = dst_rate / src_rate;
        let (detail_subtitle, notes) = currency_source_metadata(src_sym, dst_sym, unit_rate, opts);

        return Some(CalcEvaluation {
            expression: expr.to_string(),
            formatted,
            copy_value,
            category: CalcCategory::Currency,
            detail_subtitle,
            notes,
        });
    }

    // Case B: bare symbol amount (`$49.99`, `€20`, `£10`, `¥5000`) -> convert to `opts.default_currency`.
    if expr.starts_with('$') || expr.starts_with('€') || expr.starts_with('£') || expr.starts_with('¥') {
        let (amount, src_code) = parse_currency_amount(expr)?;
        if src_code.eq_ignore_ascii_case(&opts.default_currency) {
            return None;
        }
        let (src_sym, src_rate, _) = lookup_currency_rate(&src_code, &opts.custom_rates_usd)?;
        let (dst_sym, dst_rate, dst_decs) =
            lookup_currency_rate(&opts.default_currency, &opts.custom_rates_usd)?;
        let result = (amount / src_rate) * dst_rate;
        let formatted_num = format_grouped_currency(result, dst_decs);
        let formatted = format!("{formatted_num} {dst_sym}");
        let copy_value = format_number(result, dst_decs);
        let unit_rate = dst_rate / src_rate;
        let (detail_subtitle, notes) = currency_source_metadata(src_sym, dst_sym, unit_rate, opts);
        return Some(CalcEvaluation {
            expression: expr.to_string(),
            formatted,
            copy_value,
            category: CalcCategory::Currency,
            detail_subtitle,
            notes,
        });
    }

    None
}

fn parse_currency_amount(raw: &str) -> Option<(f64, String)> {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix('$') {
        let amt = eval_math(rest.trim()).ok()?;
        return Some((amt, "USD".to_string()));
    }
    if let Some(rest) = trimmed.strip_prefix('€') {
        let amt = eval_math(rest.trim()).ok()?;
        return Some((amt, "EUR".to_string()));
    }
    if let Some(rest) = trimmed.strip_prefix('£') {
        let amt = eval_math(rest.trim()).ok()?;
        return Some((amt, "GBP".to_string()));
    }
    if let Some(rest) = trimmed.strip_prefix('¥') {
        let amt = eval_math(rest.trim()).ok()?;
        return Some((amt, "JPY".to_string()));
    }
    let (val_str, unit_str) = split_amount_and_unit(trimmed)?;
    let amt = eval_math(val_str).ok()?;
    Some((amt, unit_str.to_ascii_uppercase()))
}

// ============================================================================
// 5. Pure Rust Date & Timezone Math
// ============================================================================

/// Howard Hinnant's exact civil-date-from-days algorithm (days since 1970-01-01).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = y + i64::from(m <= 2);
    (y, m as u32, d as u32)
}

/// Howard Hinnant's exact days-from-civil algorithm (days since 1970-01-01).
fn days_from_civil(y: i64, m: u32, d: u32) -> Option<i64> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = y - i64::from(m <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let m64 = u64::from(m);
    let d64 = u64::from(d);
    let doy = (153 * (if m64 > 2 { m64 - 3 } else { m64 + 9 }) + 2) / 5 + d64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + (doe as i64) - 719_468;
    // Validate calendar date didn't overflow month length (e.g. Feb 30).
    let (cy, cm, cd) = civil_from_days(days);
    if cy == y + i64::from(m <= 2) && cm == m && cd == d {
        Some(days)
    } else {
        None
    }
}

fn weekday_name(days_since_epoch: i64) -> &'static str {
    // 1970-01-01 was Thursday (index 4).
    const NAMES: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    let idx = ((days_since_epoch + 4).rem_euclid(7)) as usize;
    NAMES[idx]
}

fn parse_iso_date(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.trim().split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let y = parts[0].parse::<i64>().ok()?;
    let m = parts[1].parse::<u32>().ok()?;
    let d = parts[2].parse::<u32>().ok()?;
    days_from_civil(y, m, d)
}

fn today_days_utc() -> i64 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    secs.div_euclid(86_400)
}

fn lookup_tz_offset_mins(raw: &str) -> Option<(&'static str, i64)> {
    let s = raw.trim().to_ascii_lowercase();
    let named = match s.as_str() {
        "utc" | "gmt" | "z" => ("UTC", 0),
        "wib" | "jakarta" | "bangkok" | "hanoi" | "ict" => ("WIB (UTC+7)", 7 * 60),
        "wita" | "singapore" | "sg" | "sgt" | "kl" | "hongkong" | "hk" | "beijing" | "shanghai" | "taipei" | "perth" => ("WITA/SGT (UTC+8)", 8 * 60),
        "wit" | "tokyo" | "jst" | "seoul" | "kst" => ("JST/WIT (UTC+9)", 9 * 60),
        "sydney" | "aest" => ("AEST (UTC+10)", 10 * 60),
        "auckland" | "nzst" => ("NZST (UTC+12)", 12 * 60),
        "ist" | "india" | "mumbai" | "delhi" => ("IST (UTC+5:30)", 5 * 60 + 30),
        "dubai" | "gst" => ("GST (UTC+4)", 4 * 60),
        "london" | "bst" => ("London (UTC+0)", 0),
        "berlin" | "paris" | "cet" | "amsterdam" | "madrid" | "rome" => ("CET (UTC+1)", 60),
        "nyc" | "newyork" | "est" | "et" => ("EST (UTC-5)", -5 * 60),
        "edt" => ("EDT (UTC-4)", -4 * 60),
        "cst" | "chicago" => ("CST (UTC-6)", -6 * 60),
        "mst" | "denver" => ("MST (UTC-7)", -7 * 60),
        "pst" | "pt" | "sf" | "la" => ("PST (UTC-8)", -8 * 60),
        "pdt" => ("PDT (UTC-7)", -7 * 60),
        _ => {
            let rest = s.strip_prefix("utc").or_else(|| s.strip_prefix("gmt"))?;
            let sign = if rest.starts_with('+') {
                1
            } else if rest.starts_with('-') {
                -1
            } else {
                return None;
            };
            let digits = &rest[1..];
            let (h, m) = if let Some((hs, ms)) = digits.split_once(':') {
                (hs.parse::<i64>().ok()?, ms.parse::<i64>().ok()?)
            } else {
                (digits.parse::<i64>().ok()?, 0)
            };
            if !(0..=14).contains(&h) || !(0..=59).contains(&m) {
                return None;
            }
            let total = sign * (h * 60 + m);
            return Some(("Custom Offset", total));
        }
    };
    Some(named)
}

fn format_tz_offset(mins: i64) -> String {
    let sign = if mins >= 0 { '+' } else { '-' };
    let abs = mins.abs();
    let h = abs / 60;
    let m = abs % 60;
    if m == 0 {
        format!("UTC{sign}{h}")
    } else {
        format!("UTC{sign}{h}:{m:02}")
    }
}

fn try_evaluate_date_or_timezone(expr: &str) -> Option<CalcEvaluation> {
    let lower = expr.trim().to_ascii_lowercase();

    // 1. `now in <tz>` / `time in <tz>`
    if let Some(tz_raw) = lower
        .strip_prefix("now in ")
        .or_else(|| lower.strip_prefix("now to "))
        .or_else(|| lower.strip_prefix("time in "))
    {
        let (tz_label, offset_mins) = lookup_tz_offset_mins(tz_raw)?;
        let now_secs = now_epoch_secs() + offset_mins * 60;
        let days = now_secs.div_euclid(86_400);
        let day_secs = now_secs.rem_euclid(86_400);
        let (y, m, d) = civil_from_days(days);
        let hh = day_secs / 3600;
        let mm = (day_secs % 3600) / 60;
        let formatted = format!("{hh:02}:{mm:02} ({y:04}-{m:02}-{d:02})");
        return Some(CalcEvaluation {
            expression: expr.to_string(),
            formatted: formatted.clone(),
            copy_value: format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}"),
            category: CalcCategory::Timezone,
            detail_subtitle: format!("{tz_label} · {}", format_tz_offset(offset_mins)),
            notes: vec![
                ("Timezone".to_string(), tz_label.to_string()),
                ("Offset".to_string(), format_tz_offset(offset_mins)),
                ("Day".to_string(), weekday_name(days).to_string()),
            ],
        });
    }

    // 2. `HH:MM <src_tz> to <dst_tz>` (e.g. `14:30 wib to utc`)
    if let Some((lhs, rhs)) = split_conversion_ci(&lower, expr) {
        let mut lhs_parts = lhs.split_whitespace();
        if let (Some(time_part), Some(src_tz_raw), None) =
            (lhs_parts.next(), lhs_parts.next(), lhs_parts.next())
            && let Some((hh_s, mm_s)) = time_part.split_once(':')
            && let (Ok(hh), Ok(mm)) = (hh_s.parse::<i64>(), mm_s.parse::<i64>())
            && (0..=23).contains(&hh)
            && (0..=59).contains(&mm)
        {
            let (src_label, src_off) = lookup_tz_offset_mins(src_tz_raw)?;
            let (dst_label, dst_off) = lookup_tz_offset_mins(rhs.trim())?;
            let total_mins = hh * 60 + mm - src_off + dst_off;
            let day_shift = total_mins.div_euclid(1440);
            let wrapped = total_mins.rem_euclid(1440);
            let out_h = wrapped / 60;
            let out_m = wrapped % 60;
            let shift_suffix = match day_shift {
                0 => String::new(),
                1 => " (+1d)".to_string(),
                -1 => " (-1d)".to_string(),
                n if n > 0 => format!(" (+{n}d)"),
                n => format!(" ({n}d)"),
            };
            let formatted = format!("{out_h:02}:{out_m:02} {dst_label}{shift_suffix}");
            return Some(CalcEvaluation {
                expression: expr.to_string(),
                formatted,
                copy_value: format!("{out_h:02}:{out_m:02}"),
                category: CalcCategory::Timezone,
                detail_subtitle: format!("{src_label} -> {dst_label}"),
                notes: vec![
                    ("From".to_string(), format!("{hh:02}:{mm:02} {src_label}")),
                    ("To".to_string(), format!("{out_h:02}:{out_m:02} {dst_label}{shift_suffix}")),
                ],
            });
        }
    }

    // 3. Date literals (`today`, `tomorrow`, `yesterday`, `now`)
    if matches!(lower.as_str(), "today" | "tomorrow" | "yesterday" | "now") {
        let base = today_days_utc();
        let days = match lower.as_str() {
            "tomorrow" => base + 1,
            "yesterday" => base - 1,
            _ => base,
        };
        let (y, m, d) = civil_from_days(days);
        let iso = format!("{y:04}-{m:02}-{d:02}");
        return Some(CalcEvaluation {
            expression: expr.to_string(),
            formatted: format!("{iso} ({})", weekday_name(days)),
            copy_value: iso,
            category: CalcCategory::Date,
            detail_subtitle: weekday_name(days).to_string(),
            notes: vec![("Weekday".to_string(), weekday_name(days).to_string())],
        });
    }

    // 4. Date arithmetic (`today + 14d`, `2026-01-01 + 100d`, `2026-12-31 - today`)
    for (sep, sign) in [(" + ", 1i64), (" - ", -1i64), ("+", 1i64), ("-", -1i64)] {
        // Skip hyphen inside ISO date `YYYY-MM-DD` when scanning for bare `-`.
        let split_pos = if sep == "-" {
            // Find `-` after position 10 (past `YYYY-MM-DD`) or after keyword `today`/`tomorrow`.
            let start = if lower.starts_with("today") {
                5
            } else if lower.starts_with("tomorrow") {
                8
            } else if lower.starts_with("yesterday") {
                9
            } else if expr.len() > 10 && parse_iso_date(&expr[..10]).is_some() {
                10
            } else {
                continue;
            };
            expr[start..].find('-').map(|p| start + p)
        } else {
            expr.find(sep)
        };

        let Some(pos) = split_pos else {
            continue;
        };
        let lhs = expr[..pos].trim();
        let rhs = expr[pos + sep.len()..].trim();
        let Some(lhs_days) = parse_date_operand(lhs) else {
            continue;
        };

        // Date - Date -> difference in days/weeks
        if sign == -1
            && let Some(rhs_days) = parse_date_operand(rhs)
        {
            let diff = lhs_days - rhs_days;
            let weeks = diff / 7;
            let rem_days = (diff % 7).abs();
            let formatted = format!("{diff} days");
            let detail = if weeks != 0 {
                format!("{weeks}w {rem_days}d")
            } else {
                "Date difference".to_string()
            };
            return Some(CalcEvaluation {
                expression: expr.to_string(),
                formatted: formatted.clone(),
                copy_value: format!("{diff}"),
                category: CalcCategory::Date,
                detail_subtitle: detail.clone(),
                notes: vec![
                    ("Days".to_string(), format!("{diff}")),
                    ("Weeks".to_string(), detail),
                ],
            });
        }

        // Date +/- Duration (`14d`, `2w`, `3m`, `1y`)
        if let Some(delta_days) = parse_date_duration_days(rhs) {
            let target = lhs_days + sign * delta_days;
            let (y, m, d) = civil_from_days(target);
            let iso = format!("{y:04}-{m:02}-{d:02}");
            let wd = weekday_name(target);
            return Some(CalcEvaluation {
                expression: expr.to_string(),
                formatted: format!("{iso} ({wd})"),
                copy_value: iso,
                category: CalcCategory::Date,
                detail_subtitle: format!("{} {} days", if sign > 0 { "+" } else { "-" }, delta_days),
                notes: vec![
                    ("Weekday".to_string(), wd.to_string()),
                    ("Delta".to_string(), format!("{} days", sign * delta_days)),
                ],
            });
        }
    }

    None
}

fn parse_date_operand(raw: &str) -> Option<i64> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "today" | "now" => Some(today_days_utc()),
        "tomorrow" => Some(today_days_utc() + 1),
        "yesterday" => Some(today_days_utc() - 1),
        other => parse_iso_date(other),
    }
}

fn parse_date_duration_days(raw: &str) -> Option<i64> {
    let s = raw.trim().to_ascii_lowercase();
    let (num_str, unit) = split_amount_and_unit(&s)?;
    let n = num_str.trim().parse::<i64>().ok()?;
    let mult = match unit.trim() {
        "d" | "day" | "days" => 1,
        "w" | "wk" | "wks" | "week" | "weeks" => 7,
        "m" | "mo" | "mos" | "month" | "months" => 30,
        "y" | "yr" | "yrs" | "year" | "years" => 365,
        _ => return None,
    };
    Some(n.saturating_mul(mult))
}

// ============================================================================
// 6. String Split Helpers
// ============================================================================

fn split_keyword_ci<'a>(lower: &str, orig: &'a str, needle: &str) -> Option<(&'a str, &'a str)> {
    let idx = lower.find(needle)?;
    Some((&orig[..idx], &orig[idx + needle.len()..]))
}

fn split_conversion_ci<'a>(lower: &str, orig: &'a str) -> Option<(&'a str, &'a str)> {
    for kw in [" to ", " in ", " -> ", " => "] {
        if let Some(pair) = split_keyword_ci(lower, orig, kw) {
            return Some(pair);
        }
    }
    None
}

fn split_amount_and_unit(s: &str) -> Option<(&str, &str)> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Support two-word units such as `16 fl oz`.
    let lower = trimmed.to_ascii_lowercase();
    if let Some(prefix_len) = lower.strip_suffix(" fl oz").map(str::len) {
        let lhs = trimmed[..prefix_len].trim();
        if !lhs.is_empty() {
            return Some((lhs, "fl oz"));
        }
    }
    // Find last whitespace first (e.g. `100 cm`, `(10 + 5) kg`).
    if let Some(ws) = trimmed.rfind(char::is_whitespace) {
        let lhs = trimmed[..ws].trim();
        let rhs = trimmed[ws..].trim();
        if !lhs.is_empty() && !rhs.is_empty() {
            return Some((lhs, rhs));
        }
    }
    // Otherwise split attached numeric prefix from unit suffix (e.g. `24px`, `1.5gib`, `72f`, `14d`).
    let mut split_idx = 0;
    for (idx, ch) in trimmed.char_indices() {
        if ch.is_ascii_digit() || matches!(ch, '.' | '-' | '+' | '_') {
            split_idx = idx + ch.len_utf8();
        } else {
            break;
        }
    }
    if split_idx > 0 && split_idx < trimmed.len() {
        Some((&trimmed[..split_idx], &trimmed[split_idx..]))
    } else {
        None
    }
}

// ============================================================================
// 7. Recursive-Descent Math & Bitwise Parser
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Bang,
    LParen,
    RParen,
    Comma,
    BitAnd,
    BitOr,
    Shl,
    Shr,
}

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        if ch.is_whitespace() || ch == '_' {
            i += 1;
            continue;
        }
        // Hex / Bin / Octal integer literals
        if ch == '0' && i + 1 < chars.len() && matches!(chars[i + 1], 'x' | 'X' | 'b' | 'B' | 'o' | 'O') {
            let radix = match chars[i + 1] {
                'x' | 'X' => 16,
                'b' | 'B' => 2,
                _ => 8,
            };
            i += 2;
            let mut digits = String::new();
            while i < chars.len() {
                let c = chars[i];
                if c == '_' {
                    i += 1;
                    continue;
                }
                if c.is_digit(radix) {
                    digits.push(c);
                    i += 1;
                } else {
                    break;
                }
            }
            if digits.is_empty() {
                return Err("Invalid base literal".to_string());
            }
            let parsed = u64::from_str_radix(&digits, radix)
                .map_err(|_| "Integer literal overflow".to_string())?;
            out.push(Token::Number(parsed as f64));
            continue;
        }

        if ch.is_ascii_digit() || (ch == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit())
        {
            let mut num = String::new();
            let mut seen_dot = false;
            while i < chars.len() {
                let c = chars[i];
                if c == '_' || c == ',' {
                    // Allow thousand commas only if followed by 3 digits (otherwise comma is arg separator).
                    if c == ','
                        && !(i + 3 < chars.len()
                            && chars[i + 1].is_ascii_digit()
                            && chars[i + 2].is_ascii_digit()
                            && chars[i + 3].is_ascii_digit())
                    {
                        break;
                    }
                    i += 1;
                    continue;
                }
                if c.is_ascii_digit() {
                    num.push(c);
                    i += 1;
                } else if c == '.' && !seen_dot {
                    seen_dot = true;
                    num.push(c);
                    i += 1;
                } else {
                    break;
                }
            }
            // Scientific notation `1e-3`, `2.5e6`
            if i < chars.len() && matches!(chars[i], 'e' | 'E') {
                let save = i;
                i += 1;
                let mut exp = String::from('e');
                if i < chars.len() && matches!(chars[i], '+' | '-') {
                    exp.push(chars[i]);
                    i += 1;
                }
                let exp_start = exp.len();
                while i < chars.len() && chars[i].is_ascii_digit() {
                    exp.push(chars[i]);
                    i += 1;
                }
                if exp.len() > exp_start {
                    num.push_str(&exp);
                } else {
                    i = save;
                }
            }
            let parsed = num
                .parse::<f64>()
                .map_err(|_| format!("Invalid number '{num}'"))?;
            out.push(Token::Number(parsed));
            continue;
        }

        if ch.is_alphabetic() || ch == 'π' {
            let mut ident = String::new();
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == 'π') {
                ident.push(chars[i]);
                i += 1;
            }
            out.push(Token::Ident(ident.to_ascii_lowercase()));
            continue;
        }

        match ch {
            '+' => {
                out.push(Token::Plus);
                i += 1;
            }
            '-' | '−' => {
                out.push(Token::Minus);
                i += 1;
            }
            '*' | '×' => {
                if ch == '*' && i + 1 < chars.len() && chars[i + 1] == '*' {
                    out.push(Token::Caret);
                    i += 2;
                } else {
                    out.push(Token::Star);
                    i += 1;
                }
            }
            '/' | '÷' => {
                out.push(Token::Slash);
                i += 1;
            }
            '%' => {
                out.push(Token::Percent);
                i += 1;
            }
            '^' => {
                out.push(Token::Caret);
                i += 1;
            }
            '!' => {
                out.push(Token::Bang);
                i += 1;
            }
            '(' | '[' => {
                out.push(Token::LParen);
                i += 1;
            }
            ')' | ']' => {
                out.push(Token::RParen);
                i += 1;
            }
            ',' => {
                out.push(Token::Comma);
                i += 1;
            }
            '&' => {
                out.push(Token::BitAnd);
                i += 1;
            }
            '|' => {
                out.push(Token::BitOr);
                i += 1;
            }
            '<' if i + 1 < chars.len() && chars[i + 1] == '<' => {
                out.push(Token::Shl);
                i += 2;
            }
            '>' if i + 1 < chars.len() && chars[i + 1] == '>' => {
                out.push(Token::Shr);
                i += 2;
            }
            other => return Err(format!("Unexpected character '{other}'")),
        }
    }

    Ok(out)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn bump(&mut self) -> Option<Token> {
        let tok = self.tokens.get(self.pos).cloned();
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    fn parse_bitor(&mut self) -> Result<f64, String> {
        let mut lhs = self.parse_bitxor()?;
        while matches!(self.peek(), Some(Token::BitOr)) {
            self.bump();
            let rhs = self.parse_bitxor()?;
            lhs = ((lhs as i64) | (rhs as i64)) as f64;
        }
        Ok(lhs)
    }

    fn parse_bitxor(&mut self) -> Result<f64, String> {
        let mut lhs = self.parse_bitand()?;
        while matches!(self.peek(), Some(Token::Ident(s)) if s == "xor") {
            self.bump();
            let rhs = self.parse_bitand()?;
            lhs = ((lhs as i64) ^ (rhs as i64)) as f64;
        }
        Ok(lhs)
    }

    fn parse_bitand(&mut self) -> Result<f64, String> {
        let mut lhs = self.parse_shift()?;
        while matches!(self.peek(), Some(Token::BitAnd)) {
            self.bump();
            let rhs = self.parse_shift()?;
            lhs = ((lhs as i64) & (rhs as i64)) as f64;
        }
        Ok(lhs)
    }

    fn parse_shift(&mut self) -> Result<f64, String> {
        let mut lhs = self.parse_add()?;
        loop {
            match self.peek() {
                Some(Token::Shl) => {
                    self.bump();
                    let rhs = self.parse_add()?;
                    let shift = (rhs as u32).min(63);
                    lhs = ((lhs as i64).wrapping_shl(shift)) as f64;
                }
                Some(Token::Shr) => {
                    self.bump();
                    let rhs = self.parse_add()?;
                    let shift = (rhs as u32).min(63);
                    lhs = ((lhs as i64).wrapping_shr(shift)) as f64;
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_add(&mut self) -> Result<f64, String> {
        let mut lhs = self.parse_mul()?;
        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.bump();
                    lhs += self.parse_mul()?;
                }
                Some(Token::Minus) => {
                    self.bump();
                    lhs -= self.parse_mul()?;
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> Result<f64, String> {
        let mut lhs = self.parse_pow()?;
        loop {
            match self.peek() {
                Some(Token::Star) => {
                    self.bump();
                    lhs *= self.parse_pow()?;
                }
                Some(Token::Slash) => {
                    self.bump();
                    let rhs = self.parse_pow()?;
                    if rhs == 0.0 {
                        return Err("Division by zero".to_string());
                    }
                    lhs /= rhs;
                }
                Some(Token::Percent) => {
                    self.bump();
                    // Standalone trailing `%` (e.g. `50%`) scales by 0.01; binary `a % b` is modulo.
                    let is_binary_mod = matches!(
                        self.peek(),
                        Some(Token::Number(_) | Token::Ident(_) | Token::LParen | Token::Minus | Token::Plus)
                    );
                    if is_binary_mod {
                        let rhs = self.parse_pow()?;
                        if rhs == 0.0 {
                            return Err("Modulo by zero".to_string());
                        }
                        lhs %= rhs;
                    } else {
                        lhs /= 100.0;
                    }
                }
                // Implicit multiplication: `2(3+4)`, `2pi`, `3sqrt(9)`
                Some(Token::LParen) => {
                    lhs *= self.parse_pow()?;
                }
                Some(Token::Ident(id)) if id != "xor" => {
                    lhs *= self.parse_pow()?;
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_pow(&mut self) -> Result<f64, String> {
        let base = self.parse_unary()?;
        if matches!(self.peek(), Some(Token::Caret)) {
            self.bump();
            let exp = self.parse_pow()?; // right-associative
            Ok(base.powf(exp))
        } else {
            Ok(base)
        }
    }

    fn parse_unary(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some(Token::Plus) => {
                self.bump();
                self.parse_unary()
            }
            Some(Token::Minus) => {
                self.bump();
                Ok(-self.parse_unary()?)
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<f64, String> {
        let mut val = self.parse_primary()?;
        while matches!(self.peek(), Some(Token::Bang)) {
            self.bump();
            if val < 0.0 || val.fract().abs() > 1e-9 || val > 170.0 {
                return Err("Factorial requires integer 0..=170".to_string());
            }
            let n = val.round() as u64;
            let mut acc = 1.0_f64;
            for k in 2..=n {
                acc *= k as f64;
            }
            val = acc;
        }
        Ok(val)
    }

    fn parse_primary(&mut self) -> Result<f64, String> {
        match self.bump() {
            Some(Token::Number(n)) => Ok(n),
            Some(Token::LParen) => {
                let inner = self.parse_bitor()?;
                if !matches!(self.bump(), Some(Token::RParen)) {
                    return Err("Missing closing ')'".to_string());
                }
                Ok(inner)
            }
            Some(Token::Ident(name)) => {
                if matches!(self.peek(), Some(Token::LParen)) {
                    self.bump();
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some(Token::RParen)) {
                        loop {
                            args.push(self.parse_bitor()?);
                            if matches!(self.peek(), Some(Token::Comma)) {
                                self.bump();
                            } else {
                                break;
                            }
                        }
                    }
                    if !matches!(self.bump(), Some(Token::RParen)) {
                        return Err(format!("Missing ')' after function '{name}'"));
                    }
                    eval_function(&name, &args)
                } else {
                    match name.as_str() {
                        "pi" | "π" => Ok(std::f64::consts::PI),
                        "e" => Ok(std::f64::consts::E),
                        "tau" => Ok(std::f64::consts::TAU),
                        "phi" => Ok(f64::midpoint(1.0, 5.0_f64.sqrt())),
                        _ => Err(format!("Unknown identifier '{name}'")),
                    }
                }
            }
            Some(other) => Err(format!("Unexpected token {other:?}")),
            None => Err("Unexpected end of expression".to_string()),
        }
    }
}

fn eval_function(name: &str, args: &[f64]) -> Result<f64, String> {
    let one = |f: fn(f64) -> f64| -> Result<f64, String> {
        if args.len() != 1 {
            return Err(format!("{name}() expects 1 argument"));
        }
        Ok(f(args[0]))
    };
    match name {
        "sqrt" => one(f64::sqrt),
        "cbrt" => one(f64::cbrt),
        "abs" => one(f64::abs),
        "sin" => one(f64::sin),
        "cos" => one(f64::cos),
        "tan" => one(f64::tan),
        "asin" => one(f64::asin),
        "acos" => one(f64::acos),
        "atan" => one(f64::atan),
        "ln" => one(f64::ln),
        "log" | "log10" => one(f64::log10),
        "log2" => one(f64::log2),
        "exp" => one(f64::exp),
        "floor" => one(f64::floor),
        "ceil" => one(f64::ceil),
        "round" => one(f64::round),
        "pow" => {
            if args.len() != 2 {
                return Err("pow() expects 2 arguments".to_string());
            }
            Ok(args[0].powf(args[1]))
        }
        "min" => {
            if args.is_empty() {
                return Err("min() expects at least 1 argument".to_string());
            }
            Ok(args.iter().copied().fold(f64::INFINITY, f64::min))
        }
        "max" => {
            if args.is_empty() {
                return Err("max() expects at least 1 argument".to_string());
            }
            Ok(args.iter().copied().fold(f64::NEG_INFINITY, f64::max))
        }
        _ => Err(format!("Unknown function '{name}()'")),
    }
}

fn eval_math(input: &str) -> Result<f64, String> {
    let tokens = tokenize(input)?;
    if tokens.is_empty() {
        return Err("Empty expression".to_string());
    }
    let mut parser = Parser { tokens, pos: 0 };
    let val = parser.parse_bitor()?;
    if parser.pos != parser.tokens.len() {
        return Err("Unexpected trailing tokens".to_string());
    }
    Ok(val)
}
