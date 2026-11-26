use std::sync::{Arc, Mutex};

use aios_core::Database;
use aios_plugin_sdk::{PluginEvent, PluginResponse, ViewNode};
use aios_plugins::calculator::{
    CALCULATOR_COMMAND_ID, CALCULATOR_PLUGIN_ID, CalcCategory, CalcOptions, CachedCurrencyRates,
    CalculatorPlugin, evaluate_expression, might_be_calculator_query, needs_currency_refresh,
    parse_fawaz_currency_json, parse_open_er_api_json, parse_refresh_interval_secs,
    save_cached_currency_rates,
};
use aios_plugins::{FirstPartyPlugin, PluginEnv};

#[test]
fn fast_gate_rejects_normal_app_queries_and_accepts_calc_patterns() {
    for app_query in ["firefox", "code", "terminal", "settings", "disk usage", "notes", ""] {
        assert!(
            !might_be_calculator_query(app_query),
            "expected fast-gate to reject '{app_query}'"
        );
    }

    for calc_query in [
        "=pi",
        "=10*30",
        "10*30",
        "(45 + 5) / 2",
        "20% of 450",
        "0xff to dec",
        "100 cm to in",
        "24px to rem",
        "$49.99 to idr",
        "today + 14d",
        "now in tokyo",
        "sqrt(144)",
    ] {
        assert!(
            might_be_calculator_query(calc_query),
            "expected fast-gate to accept '{calc_query}'"
        );
    }
}

#[test]
fn arithmetic_scientific_percentage_and_float_cleanup() {
    let opts = CalcOptions::default();

    let r1 = evaluate_expression("10 * 30", &opts).expect("10*30");
    assert_eq!(r1.copy_value, "300");
    assert_eq!(r1.category, CalcCategory::Arithmetic);

    // IEEE-754 float noise cleaned cleanly.
    let r2 = evaluate_expression("0.1 + 0.2", &opts).expect("0.1+0.2");
    assert_eq!(r2.copy_value, "0.3");

    // Precedence, power right-associativity, implicit mult, factorial, functions.
    let r3 = evaluate_expression("2(3 + 4) + 2^3^2 - 5!", &opts).expect("complex expr");
    // 14 + 512 - 120 = 406
    assert_eq!(r3.copy_value, "406");

    let r4 = evaluate_expression("sqrt(144) + max(3, 9, 5)", &opts).expect("funcs");
    assert_eq!(r4.copy_value, "21");

    // Percentages
    let p1 = evaluate_expression("20% of 450", &opts).expect("20% of 450");
    assert_eq!(p1.copy_value, "90");

    let p2 = evaluate_expression("100 + 15%", &opts).expect("100 + 15%");
    assert_eq!(p2.copy_value, "115");

    let p3 = evaluate_expression("200 - 10%", &opts).expect("200 - 10%");
    assert_eq!(p3.copy_value, "180");

    // Division by zero produces clean error.
    assert!(evaluate_expression("10 / 0", &opts).is_err());
}

#[test]
fn programmer_bases_and_bitwise_operations() {
    let opts = CalcOptions::default();

    let hex = evaluate_expression("255 to hex", &opts).expect("255 to hex");
    assert_eq!(hex.copy_value, "0xFF");
    assert_eq!(hex.category, CalcCategory::Base);

    let bin = evaluate_expression("42 to bin", &opts).expect("42 to bin");
    assert_eq!(bin.copy_value, "0b101010");

    let oct = evaluate_expression("493 to oct", &opts).expect("493 to oct");
    assert_eq!(oct.copy_value, "0o755");

    let dec = evaluate_expression("0xff to dec", &opts).expect("0xff to dec");
    assert_eq!(dec.copy_value, "255");

    let bit = evaluate_expression("(0b1010 << 2) | 0x03", &opts).expect("bitwise");
    assert_eq!(bit.copy_value, "43");
}

#[test]
fn unit_temperature_data_and_css_conversions() {
    let mut opts = CalcOptions::default();

    let cm = evaluate_expression("100 cm to in", &opts).expect("cm to in");
    assert_eq!(cm.category, CalcCategory::Unit);
    assert!(cm.copy_value.starts_with("39.37"));

    let f_to_c = evaluate_expression("212 f to c", &opts).expect("212 f to c");
    assert_eq!(f_to_c.copy_value, "100");
    assert_eq!(f_to_c.formatted, "100 °C");

    let c_to_f = evaluate_expression("0 c to f", &opts).expect("0 c to f");
    assert_eq!(c_to_f.copy_value, "32");

    let data = evaluate_expression("1 gib to mib", &opts).expect("gib to mib");
    assert_eq!(data.copy_value, "1024");

    let css = evaluate_expression("24 px to rem", &opts).expect("px to rem");
    assert_eq!(css.copy_value, "1.5");
    assert_eq!(css.formatted, "1.5 rem");

    // Custom base font preference (20px) changes rem result cleanly.
    opts.base_font_px = 20.0;
    let css2 = evaluate_expression("20 px to rem", &opts).expect("20px at base 20");
    assert_eq!(css2.copy_value, "1");

    // Volume conversions (metric, US customary, multi-word `fl oz`, cubic).
    let vol_l = evaluate_expression("1500 ml to l", &opts).expect("ml to l");
    assert_eq!(vol_l.category, CalcCategory::Unit);
    assert_eq!(vol_l.copy_value, "1.5");
    assert_eq!(vol_l.formatted, "1.5 L");

    let vol_gal = evaluate_expression("1 gal to cup", &opts).expect("gal to cup");
    assert_eq!(vol_gal.copy_value, "16");

    let vol_floz = evaluate_expression("8 fl oz to cup", &opts).expect("fl oz to cup");
    assert_eq!(vol_floz.copy_value, "1");

    let vol_m3 = evaluate_expression("1 m3 to l", &opts).expect("m3 to l");
    assert_eq!(vol_m3.copy_value, "1000");
}

#[test]
fn date_timezone_and_offline_currency_conversions() {
    let opts = CalcOptions::default();

    let d1 = evaluate_expression("2026-01-01 + 14d", &opts).expect("date add");
    assert_eq!(d1.category, CalcCategory::Date);
    assert_eq!(d1.copy_value, "2026-01-15");

    let d2 = evaluate_expression("2026-12-31 - 2026-01-01", &opts).expect("date diff");
    assert_eq!(d2.copy_value, "364");
    assert_eq!(d2.formatted, "364 days");

    let tz = evaluate_expression("14:30 wib to utc", &opts).expect("tz convert");
    assert_eq!(tz.category, CalcCategory::Timezone);
    assert_eq!(tz.copy_value, "07:30");

    let now_tokyo = evaluate_expression("now in tokyo", &opts).expect("now in tokyo");
    assert_eq!(now_tokyo.category, CalcCategory::Timezone);
    assert!(now_tokyo.detail_subtitle.contains("JST"));

    let cur1 = evaluate_expression("100 usd to idr", &opts).expect("usd to idr");
    assert_eq!(cur1.category, CalcCategory::Currency);
    assert_eq!(cur1.copy_value, "1620000");
    assert_eq!(cur1.formatted, "1,620,000 IDR");

    let cur2 = evaluate_expression("$10", &opts).expect("$10 default idr");
    assert_eq!(cur2.copy_value, "162000");
}

#[test]
fn calculator_plugin_inline_prefix_history_and_preferences_round_trip() {
    let db = Arc::new(Mutex::new(
        Database::open_in_memory().expect("in-memory sqlite"),
    ));
    let plugin = CalculatorPlugin::new();
    let env = PluginEnv::new(CALCULATOR_PLUGIN_ID, "dark", Arc::clone(&db));

    // Normal app query returns None (fast-gate).
    assert!(plugin.evaluate_inline_query("firefox", &env).is_none());
    // Bare number without `=` does not spam Root Search.
    assert!(plugin.evaluate_inline_query("42", &env).is_none());
    // Explicit `=` prefix works even on bare constants and marks exclusive=true.
    let explicit = plugin
        .evaluate_inline_query("=pi", &env)
        .expect("=pi inline");
    assert!(explicit.exclusive);
    assert!(explicit.copy_value.starts_with("3.14159"));

    // Auto-detect expression without `=` sets exclusive=false.
    let auto = plugin
        .evaluate_inline_query("10 * 30", &env)
        .expect("10*30 inline");
    assert!(!auto.exclusive);
    assert_eq!(auto.copy_value, "300");

    // Inline copy action copies result AND persists to SQLite history.
    let inner_action = auto
        .copy_action_id
        .strip_prefix(&format!("plugin.inline:{CALCULATOR_PLUGIN_ID}:"))
        .expect("inline prefix");
    let copy_resp = plugin
        .handle_inline_action(inner_action, &env)
        .expect("inline copy response");
    assert!(matches!(
        copy_resp,
        PluginResponse::CopyToClipboard { ref text, .. } if text == "300"
    ));

    // Opening Calculator view now displays the saved history entry.
    let open_resp = plugin.init_command(CALCULATOR_COMMAND_ID, &[], &env);
    let PluginResponse::Render { view } = open_resp else {
        panic!("expected Render ListView");
    };
    let ViewNode::List(list) = *view else {
        panic!("expected ListView");
    };
    assert!(
        list.sections
            .iter()
            .any(|s| s.title.as_deref() == Some("Recent Calculations")
                && s.items
                    .iter()
                    .any(|i| i.title == "300"
                        && i.icon_path.as_deref()
                            == Some("asset://icons/plugins/calc-arithmetic.svg")))
    );

    // SearchTextChanged evaluates live inside Calculator view.
    let live_resp = plugin.handle_event(
        CALCULATOR_COMMAND_ID,
        &PluginEvent::SearchTextChanged {
            command_id: CALCULATOR_COMMAND_ID.to_string(),
            query: "24 px to rem".to_string(),
        },
        &env,
    );
    let PluginResponse::Render { view } = live_resp else {
        panic!("expected live Render");
    };
    let ViewNode::List(live_list) = *view else {
        panic!("expected ListView");
    };
    assert_eq!(live_list.sections[0].items[0].title, "1.5 rem");
    assert_eq!(
        live_list.sections[0].items[0].icon_path.as_deref(),
        Some("asset://icons/plugins/calc-unit.svg")
    );

    // Clear history empties saved records.
    let cleared = plugin.handle_event(
        CALCULATOR_COMMAND_ID,
        &PluginEvent::ActionTriggered {
            command_id: CALCULATOR_COMMAND_ID.to_string(),
            action_id: "calc.clear-history".to_string(),
            item_id: None,
            inline_args: Vec::new(),
        },
        &env,
    );
    let PluginResponse::Render { view } = cleared else {
        panic!("expected cleared Render");
    };
    let ViewNode::List(cleared_list) = *view else {
        panic!("expected ListView");
    };
    assert!(cleared_list.sections.is_empty());
}

#[test]
fn cached_currency_parsers_ttl_and_sqlite_round_trip() {
    // 1. Refresh interval parser
    assert_eq!(parse_refresh_interval_secs("off"), None);
    assert_eq!(parse_refresh_interval_secs("0"), None);
    assert_eq!(parse_refresh_interval_secs("disabled"), None);
    assert_eq!(parse_refresh_interval_secs("6h"), Some(6 * 3600));
    assert_eq!(parse_refresh_interval_secs("24h"), Some(24 * 3600));
    assert_eq!(parse_refresh_interval_secs("3d"), Some(3 * 86_400));
    assert_eq!(parse_refresh_interval_secs("1m"), Some(300)); // clamped to 5m min

    // 2. Stale-While-Revalidate TTL checks
    assert!(needs_currency_refresh(None, Some(86_400), 1_000_000));
    assert!(!needs_currency_refresh(None, None, 1_000_000)); // disabled
    assert!(!needs_currency_refresh(Some(990_000), Some(86_400), 1_000_000)); // fresh (10k < 86.4k)
    assert!(needs_currency_refresh(Some(900_000), Some(86_400), 1_000_000)); // stale (100k >= 86.4k)

    // 3. Primary fawazahmed0/currency-api parser (fiat + crypto)
    let fawaz_json = r#"{
        "date": "2026-11-26",
        "usd": {
            "usd": 1,
            "idr": 16543.25,
            "eur": 0.91,
            "gbp": 0.78,
            "jpy": 152.5,
            "sgd": 1.33,
            "btc": 0.0000102,
            "eth": 0.00029
        }
    }"#;
    let (f_date, f_rates) = parse_fawaz_currency_json(fawaz_json).expect("parse fawaz");
    assert_eq!(f_date, "2026-11-26");
    assert_eq!(f_rates.get("IDR"), Some(&16_543.25));
    assert_eq!(f_rates.get("BTC"), Some(&0.000_010_2));

    // 4. Fallback open.er-api.com parser
    let open_er_json = r#"{
        "result": "success",
        "time_last_update_unix": 1795651200,
        "rates": {
            "USD": 1,
            "IDR": 16600.0,
            "EUR": 0.90,
            "GBP": 0.77,
            "JPY": 151.0,
            "SGD": 1.32
        }
    }"#;
    let (e_date, e_rates) = parse_open_er_api_json(open_er_json).expect("parse open.er-api");
    assert_eq!(e_date, "2026-11-26");
    assert_eq!(e_rates.get("IDR"), Some(&16_600.0));

    // 5. SQLite CachedCurrencyRates persistence flows cleanly into live Calculator results
    let db = Arc::new(Mutex::new(
        Database::open_in_memory().expect("in-memory sqlite"),
    ));
    let snapshot = CachedCurrencyRates {
        updated_at: 1_795_651_200,
        date_label: f_date,
        source: "fawazahmed0/currency-api".to_string(),
        rates: f_rates,
    };
    save_cached_currency_rates(&db, &snapshot).expect("save cached rates");

    let plugin = CalculatorPlugin::new();
    let env = PluginEnv::new(CALCULATOR_PLUGIN_ID, "dark", Arc::clone(&db));
    let inline = plugin
        .evaluate_inline_query("100 usd to idr", &env)
        .expect("cached live rate evaluation");
    assert_eq!(inline.copy_value, "1654325");
    assert!(inline.subtitle.contains("cached 2026-11-26"));
}
