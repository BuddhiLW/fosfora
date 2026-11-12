//! The OSC listener and sender settings (Setup › Control › OSC): Promote
//! (args -> [`OscReceiveChange`] / [`OscSendChange`], pure) and the Boundary
//! op bodies, one [`OscNetPort`] call per setting.

use serde_json::{Value, json};

use super::port::{OscNetPort, OscSettings};
use crate::control::{OpResult, args};

/// The send rates the Setup page offers, per second.
pub const TX_RATE_RANGE: std::ops::RangeInclusive<u32> = 1..=120;
/// The send namespaces: `/fosfora/...`, or the pre-rename `/phosphor/...`.
pub const TX_PREFIXES: [&str; 2] = ["fosfora", "phosphor"];

/// A change to the listener; `None` keeps a field.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OscReceiveChange {
    pub enabled: Option<bool>,
    pub port: Option<u16>,
    pub lan: Option<bool>,
}

impl OscReceiveChange {
    pub fn from_args(a: &Value) -> Result<Self, String> {
        Ok(Self {
            enabled: args::opt_bool(a, "enabled"),
            port: args::opt_port(a, "port")?,
            lan: args::opt_bool(a, "lan"),
        })
    }
}

/// A change to the sender; `None` keeps a field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OscSendChange {
    pub enabled: Option<bool>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub rate: Option<u32>,
    pub prefix: Option<&'static str>,
}

impl OscSendChange {
    pub fn from_args(a: &Value) -> Result<Self, String> {
        let host = match a.get("host") {
            None => None,
            Some(Value::String(h)) if !h.trim().is_empty() && !h.contains(char::is_whitespace) => {
                Some(h.clone())
            }
            Some(other) => {
                return Err(format!(":host must be a host name or address, got {other}"));
            }
        };
        let rate = match a.get("rate") {
            None => None,
            Some(v) => Some(
                v.as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| TX_RATE_RANGE.contains(n))
                    .ok_or_else(|| {
                        format!(
                            ":rate must be an integer in {}..={} (times a second), got {v}",
                            TX_RATE_RANGE.start(),
                            TX_RATE_RANGE.end()
                        )
                    })?,
            ),
        };
        let prefix = match args::opt_str(a, "prefix") {
            None if a.get("prefix").is_some() => {
                return Err(":prefix must be fosfora or phosphor".into());
            }
            None => None,
            Some(raw) => {
                let want = raw.trim().trim_matches('/').to_ascii_lowercase();
                Some(
                    TX_PREFIXES
                        .into_iter()
                        .find(|p| *p == want)
                        .ok_or_else(|| {
                            format!(":prefix must be fosfora or phosphor, got {raw:?}")
                        })?,
                )
            }
        };
        Ok(Self {
            enabled: args::opt_bool(a, "enabled"),
            host,
            port: args::opt_port(a, "port")?,
            rate,
            prefix,
        })
    }
}

fn receive_json(s: &OscSettings) -> Value {
    json!({
        "enabled": s.enabled,
        "port": s.rx_port,
        "lan": s.rx_lan,
        "active": s.active,
        "last-address": s.last_address,
    })
}

fn send_json(s: &OscSettings) -> Value {
    json!({
        "enabled": s.tx_enabled,
        "host": s.tx_host,
        "port": s.tx_port,
        "rate": s.tx_rate_hz,
        "prefix": s.tx_prefix,
    })
}

/// `bindings/osc-config`: read or change what the listener listens on.
pub fn osc_config<P: OscNetPort>(port: &mut P, a: &Value) -> OpResult {
    let change = OscReceiveChange::from_args(a)?;
    let now = port.osc_settings();
    if let Some(p) = change.port.filter(|&p| p != now.rx_port) {
        port.set_osc_rx_port(p);
    }
    if let Some(lan) = change.lan.filter(|&l| l != now.rx_lan) {
        port.set_osc_rx_lan(lan);
    }
    if let Some(on) = change.enabled.filter(|&e| e != now.enabled) {
        port.set_osc_enabled(on);
    }
    Ok(receive_json(&port.osc_settings()))
}

/// `bindings/osc-send`: read or change where the analysis is sent.
pub fn osc_send<P: OscNetPort>(port: &mut P, a: &Value) -> OpResult {
    let change = OscSendChange::from_args(a)?;
    let now = port.osc_settings();
    if change.host.is_some() || change.port.is_some() {
        let host = change.host.as_deref().unwrap_or(&now.tx_host);
        port.set_osc_tx_target(host, change.port.unwrap_or(now.tx_port));
    }
    if let Some(hz) = change.rate {
        port.set_osc_tx_rate(hz);
    }
    if let Some(p) = change.prefix {
        port.set_osc_tx_prefix(p);
    }
    if let Some(on) = change.enabled {
        port.set_osc_tx_enabled(on);
    }
    Ok(send_json(&port.osc_settings()))
}
