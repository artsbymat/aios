//! XDG Desktop Portal `org.freedesktop.portal.Settings` color-scheme reader & live watcher.
//!
//! Reads `org.freedesktop.appearance` / `color-scheme` on session D-Bus:
//! - `1` -> [`SystemColorScheme::PreferDark`]
//! - `2` -> [`SystemColorScheme::PreferLight`]
//! - `0` (or unavailable) -> [`SystemColorScheme::NoPreference`]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use zbus::blocking::{Connection, MessageIterator, Proxy};
use zbus::zvariant::OwnedValue;

use crate::SystemColorScheme;

const PORTAL_BUS: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS_IFACE: &str = "org.freedesktop.portal.Settings";
const APPEARANCE_NS: &str = "org.freedesktop.appearance";
const COLOR_SCHEME_KEY: &str = "color-scheme";

/// Maps the freedesktop `color-scheme` uint32 value to [`SystemColorScheme`].
#[must_use]
pub fn decode_portal_color_scheme(raw: u32) -> SystemColorScheme {
    match raw {
        1 => SystemColorScheme::PreferDark,
        2 => SystemColorScheme::PreferLight,
        _ => SystemColorScheme::NoPreference,
    }
}

fn extract_u32_from_value(value: &OwnedValue) -> Option<u32> {
    fn unwrap_val(v: &zbus::zvariant::Value<'_>) -> Option<u32> {
        match v {
            zbus::zvariant::Value::U32(n) => Some(*n),
            zbus::zvariant::Value::Value(inner) => unwrap_val(inner),
            _ => None,
        }
    }
    unwrap_val(value)
}

/// Queries the current desktop color scheme once from `org.freedesktop.portal.Settings`.
///
/// Degrades gracefully to [`SystemColorScheme::NoPreference`] if D-Bus or the portal
/// service is unavailable.
#[must_use]
pub fn read_system_color_scheme() -> SystemColorScheme {
    match try_read_portal_color_scheme() {
        Ok(scheme) => scheme,
        Err(err) => {
            tracing::debug!(error = %err, "XDG portal color-scheme unavailable, using fallback");
            SystemColorScheme::NoPreference
        }
    }
}

fn try_read_portal_color_scheme() -> zbus::Result<SystemColorScheme> {
    let conn = Connection::session()?;
    let proxy = Proxy::new(&conn, PORTAL_BUS, PORTAL_PATH, SETTINGS_IFACE)?;

    // Portal Settings v2 provides `ReadOne(ns, key) -> v`; v1 provides `Read(ns, key) -> v` (nested).
    if let Ok(val) = proxy.call::<_, _, OwnedValue>("ReadOne", &(APPEARANCE_NS, COLOR_SCHEME_KEY))
        && let Some(raw) = extract_u32_from_value(&val)
    {
        return Ok(decode_portal_color_scheme(raw));
    }

    let val: OwnedValue = proxy.call("Read", &(APPEARANCE_NS, COLOR_SCHEME_KEY))?;
    Ok(extract_u32_from_value(&val)
        .map(decode_portal_color_scheme)
        .unwrap_or(SystemColorScheme::NoPreference))
}

/// Background D-Bus watcher streaming live `SettingChanged` theme updates.
#[derive(Debug)]
pub struct PortalThemeWatcher {
    stop_flag: Arc<AtomicBool>,
    _thread: Option<JoinHandle<()>>,
}

impl PortalThemeWatcher {
    /// Starts listening for `org.freedesktop.portal.Settings.SettingChanged` on a background thread.
    ///
    /// Immediately reads and returns the initial [`SystemColorScheme`] along with the watcher
    /// handle and an async channel delivering live changes.
    #[must_use]
    pub fn spawn() -> (
        Self,
        SystemColorScheme,
        async_channel::Receiver<SystemColorScheme>,
    ) {
        let initial = read_system_color_scheme();
        let (tx, rx) = async_channel::unbounded();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop_flag);

        let handle = thread::Builder::new()
            .name("aios-portal-theme".into())
            .spawn(move || watch_loop(thread_stop, tx))
            .ok();

        (
            Self {
                stop_flag,
                _thread: handle,
            },
            initial,
            rx,
        )
    }
}

impl Drop for PortalThemeWatcher {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Release);
    }
}

fn watch_loop(stop_flag: Arc<AtomicBool>, tx: async_channel::Sender<SystemColorScheme>) {
    let Ok(conn) = Connection::session() else {
        tracing::debug!("session D-Bus unavailable; live portal theme watcher disabled");
        return;
    };

    let Ok(rule) = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(SETTINGS_IFACE)
        .and_then(|b| b.member("SettingChanged"))
        .map(|b| b.build())
    else {
        return;
    };

    let Ok(stream) = MessageIterator::for_match_rule(rule, &conn, Some(16)) else {
        tracing::debug!("failed to subscribe to portal SettingChanged signal");
        return;
    };

    for msg in stream {
        if stop_flag.load(Ordering::Acquire) {
            break;
        }
        let Ok(msg) = msg else {
            continue;
        };
        let Ok((namespace, key, value)) = msg.body().deserialize::<(String, String, OwnedValue)>()
        else {
            continue;
        };
        if namespace == APPEARANCE_NS
            && key == COLOR_SCHEME_KEY
            && let Some(raw) = extract_u32_from_value(&value)
        {
            let scheme = decode_portal_color_scheme(raw);
            tracing::info!(?scheme, "desktop portal color-scheme changed");
            if tx.send_blocking(scheme).is_err() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_freedesktop_color_scheme_values() {
        assert_eq!(
            decode_portal_color_scheme(0),
            SystemColorScheme::NoPreference
        );
        assert_eq!(decode_portal_color_scheme(1), SystemColorScheme::PreferDark);
        assert_eq!(
            decode_portal_color_scheme(2),
            SystemColorScheme::PreferLight
        );
        assert_eq!(
            decode_portal_color_scheme(99),
            SystemColorScheme::NoPreference
        );
    }
}
