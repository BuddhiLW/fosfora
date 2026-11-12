//! Boundary: the audio-input op bodies (device, trim, band scale, sync
//! offset, structure tuning). Each setter is one [`AudioPort`] call; every op
//! answers with the value now in force.

// Every op body has the fixed `OpFn` signature, infallible ones included.
#![allow(clippy::unnecessary_wraps)]

use serde_json::{Value, json};

use super::pipeline::{band_scale_key, parse_band_scale, patch_tuning};
use super::port::AudioPort;
use crate::audio::input_level::{TRIM_MAX_DB, TRIM_MIN_DB};
use crate::audio::{SYNC_OFFSET_MAX_MS, SYNC_OFFSET_MIN_MS, StructureConfig};
use crate::control::{OpResult, args};

fn state_json<P: AudioPort>(port: &mut P) -> Value {
    let s = port.audio_state();
    json!({
        "device": s.device,
        "active": s.active,
        "error": s.error,
        "saved": s.saved,
        "devices": port.audio_devices(),
    })
}

/// A number arg at `key` inside `lo..=hi`, or `None` when absent.
fn opt_in_range(a: &Value, key: &str, lo: f32, hi: f32) -> Result<Option<f32>, String> {
    match a.get(key) {
        None => Ok(None),
        Some(_) => match args::opt_f32(a, key) {
            Some(v) if v.is_finite() && (lo..=hi).contains(&v) => Ok(Some(v)),
            _ => Err(format!(":{key} must be a number in {lo}..={hi}")),
        },
    }
}

pub fn audio<P: AudioPort>(port: &mut P, _: &Value) -> OpResult {
    let mut v = state_json(port);
    v["input-trim-db"] = json!(port.input_trim_db());
    v["band-scale"] = json!(band_scale_key(port.band_scale()));
    v["sync-offset-ms"] = json!(port.sync_offset_ms());
    Ok(v)
}

pub fn device<P: AudioPort>(port: &mut P, a: &Value) -> OpResult {
    if args::opt_bool(a, "default").unwrap_or(false) {
        port.switch_audio_device(None);
        return Ok(state_json(port));
    }
    let devices = port.audio_devices();
    let i = args::pick(a, &devices, "audio device").map_err(|e| {
        format!("{e} (or :default true); devices: {devices:?}, rescanned every few seconds")
    })?;
    port.switch_audio_device(Some(&devices[i]));
    Ok(state_json(port))
}

pub fn input_trim<P: AudioPort>(port: &mut P, a: &Value) -> OpResult {
    if let Some(db) = opt_in_range(a, "db", TRIM_MIN_DB, TRIM_MAX_DB)? {
        port.set_input_trim(db);
    }
    Ok(json!({"db": port.input_trim_db()}))
}

pub fn band_scale<P: AudioPort>(port: &mut P, a: &Value) -> OpResult {
    if let Some(raw) = args::opt_str(a, "scale") {
        let scale = parse_band_scale(raw)
            .ok_or_else(|| format!(":scale must be db or legacy, got {raw:?}"))?;
        port.set_band_scale(scale);
    } else if a.get("scale").is_some() {
        return Err(":scale must be db or legacy".into());
    }
    let scale = port.band_scale();
    Ok(json!({"scale": band_scale_key(scale), "label": scale.display_name()}))
}

pub fn sync_offset<P: AudioPort>(port: &mut P, a: &Value) -> OpResult {
    if let Some(ms) = opt_in_range(a, "ms", SYNC_OFFSET_MIN_MS, SYNC_OFFSET_MAX_MS)? {
        port.set_sync_offset(ms);
    }
    Ok(json!({"ms": port.sync_offset_ms()}))
}

pub fn structure_tuning<P: AudioPort>(port: &mut P, a: &Value) -> OpResult {
    let reset = args::opt_bool(a, "reset").unwrap_or(false);
    let base = if reset {
        StructureConfig::default()
    } else {
        port.structure_tuning()
    };
    let next = patch_tuning(base, a)?;
    if reset || next != port.structure_tuning() {
        port.set_structure_tuning(next);
    }
    serde_json::to_value(port.structure_tuning()).map_err(|e| e.to_string())
}
