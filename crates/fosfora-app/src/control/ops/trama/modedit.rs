//! Modulation edits from `trama/mod` args: pure, in the `.fio.json`
//! spelling of a [`Modulation`].

use serde_json::{Value, json};

use crate::control::args;
use crate::trama::audio::AudioFeature;
use crate::trama::modulation::{Modulation, OscShape};

/// A fresh modulation's depth and slew: the inspector's defaults.
const DEFAULT_AMOUNT: f32 = 0.5;
const DEFAULT_SMOOTHING: f32 = 0.2;

fn norm(s: &str) -> String {
    s.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}

/// The source a name picks, as the inspector's source menu does: an
/// oscillator shape keeps the current oscillator's rate and phase.
fn source_from_name(name: &str, current: Option<&Value>) -> Result<Value, String> {
    let n = norm(name);
    if serde_json::from_value::<OscShape>(json!(n)).is_ok() {
        let osc = current.and_then(|s| s.get("oscillator"));
        let rate = osc
            .and_then(|o| o.get("rate"))
            .cloned()
            .unwrap_or_else(|| json!({"hz": 1.0}));
        let phase = osc
            .and_then(|o| o.get("phase"))
            .cloned()
            .unwrap_or_else(|| json!(0.0));
        return Ok(json!({"oscillator": {"shape": n, "rate": rate, "phase": phase}}));
    }
    if let Some(band) = n.strip_prefix("band").map(|b| b.trim_start_matches('_')) {
        if let Ok(b) = band.parse::<u8>() {
            return Ok(json!({"audio": {"band": b.min(31)}}));
        }
    }
    if serde_json::from_value::<AudioFeature>(json!(n)).is_ok() {
        return Ok(json!({"audio": n}));
    }
    Err(format!(
        "unknown :source {name:?}: an oscillator (sine saw square triangle sample_hold drift) \
         or audio (rms onset bass mid high band<n> beat_phase bpm)"
    ))
}

/// The modulation a `trama/mod` call leaves on a param. `:off true`
/// removes it; otherwise `:source`, `:amount`, `:mode`, `:smoothing`,
/// `:rate` (Hz number or beat division name) and `:phase` change only what
/// they name. A param with no modulation needs a `:source`.
pub fn modulation_edit(
    current: Option<Modulation>,
    a: &Value,
) -> Result<Option<Modulation>, String> {
    if args::opt_bool(a, "off") == Some(true) {
        return Ok(None);
    }
    let mut m = match current {
        Some(c) => serde_json::to_value(c).map_err(|e| e.to_string())?,
        None if a.get("source").is_some() => {
            json!({"source": null, "amount": DEFAULT_AMOUNT, "mode": "add", "smoothing": DEFAULT_SMOOTHING})
        }
        None => return Err("that param has no modulation; give :source to add one".into()),
    };
    match a.get("source") {
        Some(Value::String(s)) => m["source"] = source_from_name(s, m.get("source"))?,
        Some(obj @ Value::Object(_)) => m["source"] = obj.clone(),
        Some(other) => {
            return Err(format!(
                ":source must be a name or a source map, got {other}"
            ));
        }
        None => {}
    }
    for key in ["rate", "phase"] {
        let Some(v) = a.get(key) else { continue };
        let Some(osc) = m["source"].get_mut("oscillator") else {
            return Err(format!(":{key} applies to an oscillator source only"));
        };
        osc[key] = match (key, v) {
            ("rate", Value::Number(_)) => json!({"hz": v}),
            ("rate", Value::String(s)) => json!({"beat_sync": norm(s)}),
            _ => v.clone(),
        };
    }
    for key in ["amount", "smoothing"] {
        if let Some(v) = a.get(key) {
            m[key] = v.clone();
        }
    }
    if let Some(mode) = args::opt_str(a, "mode") {
        m["mode"] = json!(norm(mode));
    }
    let parsed: Modulation =
        serde_json::from_value(m).map_err(|e| format!("bad modulation: {e}"))?;
    check_modulation(&parsed)?;
    Ok(Some(parsed))
}

fn check_modulation(m: &Modulation) -> Result<(), String> {
    use crate::trama::modulation::{ModSource, OscRate};
    if !(-1.0..=1.0).contains(&m.amount) {
        return Err(":amount must be within -1..1".into());
    }
    if !(0.0..=1.0).contains(&m.smoothing) {
        return Err(":smoothing must be within 0..1".into());
    }
    if let ModSource::Oscillator(osc) = m.source {
        if !(0.0..=1.0).contains(&osc.phase) {
            return Err(":phase must be within 0..1".into());
        }
        if let OscRate::Hz(hz) = osc.rate {
            if !(hz.is_finite() && hz > 0.0) {
                return Err(":rate in Hz must be a positive number".into());
            }
        }
    }
    Ok(())
}
