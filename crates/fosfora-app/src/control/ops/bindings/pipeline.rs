//! Promote (args -> domain values) and Pipeline (pure functions) for the
//! binding ops. Nothing here touches a port.

use serde_json::{Value, json};

use super::port::{BindingEdit, MapTarget, MappingRow, MappingSource};
use crate::audio::StructureConfig;
use crate::bindings::lfo;
use crate::bindings::transforms::CURVE_TYPES;
use crate::bindings::types::{Binding, BindingScope, BindingTarget, TransformDef};
use crate::control::args;
use crate::midi::types::TriggerAction;
use crate::settings::BandScale;

// ── Promote ────────────────────────────────────────────────────────────────

/// `"preset"` or `"global"` (any case) at `key`; absent is `None`.
pub fn parse_scope(a: &Value, key: &str) -> Result<Option<BindingScope>, String> {
    let Some(raw) = args::opt_str(a, key) else {
        return Ok(None);
    };
    match raw.to_ascii_lowercase().as_str() {
        "preset" => Ok(Some(BindingScope::Preset)),
        "global" => Ok(Some(BindingScope::Global)),
        _ => Err(format!(":{key} must be preset or global, got {raw:?}")),
    }
}

pub fn scope_name(scope: &BindingScope) -> &'static str {
    match scope {
        BindingScope::Preset => "preset",
        BindingScope::Global => "global",
    }
}

/// A target in its dotted wire form. Empty is "no target yet"; a string the
/// app would not act on is refused.
pub fn parse_target(raw: &str) -> Result<BindingTarget, String> {
    match BindingTarget::from(raw) {
        BindingTarget::Unknown(s) => {
            Err(format!("unknown target {s:?}; (bind/targets) lists them"))
        }
        t => Ok(t),
    }
}

/// Sources are open-ended (MIDI, OSC and ws.* names come from outside), but
/// an `lfo.` key must be one the bus can generate.
pub fn validate_source(source: &str) -> Result<(), String> {
    if source.starts_with(lfo::PREFIX) && lfo::parse(source).is_none() {
        return Err(format!(
            "{source:?} is not an LFO source; (bind/lfos) lists them"
        ));
    }
    Ok(())
}

/// The name a binding shows: its own, else `source -> target`.
pub fn label(b: &Binding) -> String {
    if b.name.is_empty() {
        format!("{} -> {}", b.source, b.target)
    } else {
        b.name.clone()
    }
}

/// The binding `{:id "b_001"}`, `{:index n}` or `{:name "x"}` (a label, or a
/// unique substring of one) names.
pub fn pick_binding(bindings: &[Binding], a: &Value) -> Result<usize, String> {
    if let Some(id) = args::opt_str(a, "id") {
        return bindings
            .iter()
            .position(|b| b.id == id)
            .ok_or_else(|| format!("no binding {id:?}"));
    }
    let labels: Vec<String> = bindings.iter().map(label).collect();
    args::pick(a, &labels, "binding")
}

/// A trigger by name: `next_preset`, `next-preset` or `NextPreset`.
pub fn parse_trigger(raw: &str) -> Option<TriggerAction> {
    let squash = |s: &str| s.to_ascii_lowercase().replace(['_', '-', ' '], "");
    let want = squash(raw);
    TriggerAction::ALL
        .iter()
        .copied()
        .find(|a| squash(&format!("{a:?}")) == want)
}

/// The snake_case name [`parse_trigger`] reads back.
pub fn trigger_key(action: TriggerAction) -> String {
    let mut out = String::new();
    for (i, c) in format!("{action:?}").chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// `{:param "speed"}` or `{:trigger "next_preset"}`.
pub fn map_target(a: &Value) -> Result<MapTarget, String> {
    if let Some(p) = args::opt_str(a, "param") {
        return Ok(MapTarget::Param(p.to_owned()));
    }
    let raw = args::str(a, "trigger").map_err(|_| "give :param or :trigger".to_string())?;
    parse_trigger(raw).map(MapTarget::Trigger).ok_or_else(|| {
        let all: Vec<String> = TriggerAction::ALL.iter().map(|a| trigger_key(*a)).collect();
        format!("unknown trigger {raw:?}; one of {}", all.join(" "))
    })
}

/// `param:<name>` or `trigger:<name>`: a mapping target as one sortable key.
pub fn target_key(t: &MapTarget) -> String {
    match t {
        MapTarget::Param(p) => format!("param:{p}"),
        MapTarget::Trigger(a) => format!("trigger:{}", trigger_key(*a)),
    }
}

pub fn band_scale_key(scale: BandScale) -> &'static str {
    match scale {
        BandScale::Db => "db",
        BandScale::Legacy => "legacy",
    }
}

/// `db` (or `unified`) and `legacy`, any case.
pub fn parse_band_scale(raw: &str) -> Option<BandScale> {
    match raw.to_ascii_lowercase().as_str() {
        "db" | "unified" | "unified_db" | "unified-db" => Some(BandScale::Db),
        "legacy" => Some(BandScale::Legacy),
        _ => None,
    }
}

// ── Pipeline (pure) ────────────────────────────────────────────────────────

pub fn transform_kind(t: &TransformDef) -> &'static str {
    match t {
        TransformDef::Remap { .. } => "remap",
        TransformDef::Smooth { .. } => "smooth",
        TransformDef::Invert => "invert",
        TransformDef::Quantize { .. } => "quantize",
        TransformDef::Deadzone { .. } => "deadzone",
        TransformDef::Curve { .. } => "curve",
        TransformDef::Gate { .. } => "gate",
        TransformDef::Scale { .. } => "scale",
        TransformDef::Offset { .. } => "offset",
        TransformDef::Clamp { .. } => "clamp",
    }
}

/// `list` with the transform of `kind` set to `to`: the first one of that
/// kind is replaced in place (later ones dropped), or `to` is appended; `None`
/// removes every one of that kind. Idempotent.
pub fn set_transform(
    list: &[TransformDef],
    kind: &str,
    to: Option<TransformDef>,
) -> Vec<TransformDef> {
    let mut out = Vec::with_capacity(list.len() + 1);
    let mut placed = false;
    for t in list {
        if transform_kind(t) != kind {
            out.push(t.clone());
        } else if let (false, Some(new)) = (placed, &to) {
            out.push(new.clone());
            placed = true;
        }
    }
    if let (false, Some(new)) = (placed, to) {
        out.push(new);
    }
    out
}

/// Whether `t` is a transform the matrix could have produced: finite numbers,
/// a known curve, a smoothing factor in 0..=1, at least two quantize steps.
pub fn validate_transform(t: &TransformDef) -> Result<(), String> {
    let finite = |vals: &[f32]| vals.iter().all(|v| v.is_finite());
    let ok = match t {
        TransformDef::Remap {
            in_lo,
            in_hi,
            out_lo,
            out_hi,
        } => finite(&[*in_lo, *in_hi, *out_lo, *out_hi]),
        TransformDef::Smooth { factor } => factor.is_finite() && (0.0..=1.0).contains(factor),
        TransformDef::Invert => true,
        TransformDef::Quantize { steps } => *steps >= 2,
        TransformDef::Deadzone { lo, hi } | TransformDef::Clamp { lo, hi } => finite(&[*lo, *hi]),
        TransformDef::Curve { curve_type } => CURVE_TYPES.contains(&curve_type.as_str()),
        TransformDef::Gate { threshold: v }
        | TransformDef::Scale { factor: v }
        | TransformDef::Offset { value: v } => v.is_finite(),
    };
    if ok {
        Ok(())
    } else {
        Err(format!(
            "bad {} transform {}; curves: {}",
            transform_kind(t),
            serde_json::to_string(t).unwrap_or_default(),
            CURVE_TYPES.join(" ")
        ))
    }
}

fn number(v: &Value, what: &str) -> Result<f32, String> {
    #[allow(clippy::cast_possible_truncation)]
    v.as_f64()
        .filter(|n| n.is_finite() && n.abs() <= 1e6)
        .map(|n| n as f32)
        .ok_or_else(|| format!(":{what} must be a number"))
}

/// The transform chain after the args' edits: `:transforms` (a full list in
/// the saved JSON form) replaces it, then the shorthands set one transform
/// each: `:curve` (a name; nil or "none" removes), `:smoothing` (0..1),
/// `:range [lo hi]` (output range of a remap), `:invert` (bool).
pub fn patch_transforms(current: &[TransformDef], a: &Value) -> Result<Vec<TransformDef>, String> {
    let mut list = match a.get("transforms") {
        None => current.to_vec(),
        Some(v) => serde_json::from_value::<Vec<TransformDef>>(v.clone())
            .map_err(|e| format!(":transforms: {e}"))?,
    };
    if let Some(v) = a.get("curve") {
        let to = match v {
            Value::Null => None,
            Value::String(s) if s == "none" => None,
            Value::String(s) => Some(TransformDef::Curve {
                curve_type: s.clone(),
            }),
            _ => return Err(":curve must be a curve name".into()),
        };
        list = set_transform(&list, "curve", to);
    }
    if let Some(v) = a.get("smoothing") {
        let to = match v {
            Value::Null => None,
            v => Some(TransformDef::Smooth {
                factor: number(v, "smoothing")?,
            }),
        };
        list = set_transform(&list, "smooth", to);
    }
    if let Some(v) = a.get("range") {
        let to = match v {
            Value::Null => None,
            Value::Array(pair) if pair.len() == 2 => {
                let (in_lo, in_hi) = list
                    .iter()
                    .find_map(|t| match t {
                        TransformDef::Remap { in_lo, in_hi, .. } => Some((*in_lo, *in_hi)),
                        _ => None,
                    })
                    .unwrap_or((0.0, 1.0));
                Some(TransformDef::Remap {
                    in_lo,
                    in_hi,
                    out_lo: number(&pair[0], "range")?,
                    out_hi: number(&pair[1], "range")?,
                })
            }
            _ => return Err(":range must be [lo hi]".into()),
        };
        list = set_transform(&list, "remap", to);
    }
    if let Some(v) = a.get("invert") {
        let on = match v {
            Value::Null => false,
            Value::Bool(b) => *b,
            _ => return Err(":invert must be true or false".into()),
        };
        list = set_transform(&list, "invert", on.then_some(TransformDef::Invert));
    }
    list.iter().try_for_each(validate_transform)?;
    Ok(list)
}

/// `base` with the args' field edits: `:source`, `:target`, `:name`,
/// `:enabled` and the transform edits of [`patch_transforms`]. Refuses the
/// whole edit if any part is bad.
pub fn edited(base: BindingEdit, a: &Value) -> Result<BindingEdit, String> {
    let text = |key: &str| -> Result<Option<String>, String> {
        match a.get(key) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(_) => Err(format!(":{key} must be a string")),
        }
    };
    let mut e = base;
    if let Some(s) = text("source")? {
        validate_source(&s)?;
        e.source = s;
    }
    if let Some(t) = text("target")? {
        e.target = parse_target(&t)?;
    }
    if let Some(n) = text("name")? {
        e.name = n;
    }
    if let Some(v) = a.get("enabled") {
        e.enabled = v.as_bool().ok_or(":enabled must be true or false")?;
    }
    e.transforms = patch_transforms(&e.transforms, a)?;
    Ok(e)
}

/// `{index id name label enabled scope source target transforms input output}`.
pub fn binding_json(
    index: usize,
    b: &Binding,
    (input, output): (Option<f32>, Option<f32>),
) -> Value {
    json!({
        "index": index,
        "id": b.id,
        "name": b.name,
        "label": label(b),
        "enabled": b.enabled,
        "scope": scope_name(&b.scope),
        "source": b.source,
        "target": b.target.to_string(),
        "transforms": serde_json::to_value(&b.transforms).unwrap_or(Value::Null),
        "input": input,
        "output": output,
    })
}

/// Case-insensitive substring match; an absent needle matches everything.
pub fn matches(needle: Option<&str>, hay: &[&str]) -> bool {
    needle.is_none_or(|n| {
        let n = n.to_lowercase();
        hay.iter().any(|h| h.to_lowercase().contains(&n))
    })
}

/// The LFO source key for `shape`, `division` and a phase offset in degrees,
/// in the form the bus publishes; refused unless the bus can generate it.
pub fn lfo_key(shape: &str, division: &str, degrees: u16) -> Result<String, String> {
    let raw = if degrees == 0 {
        format!("{}{shape}.{division}", lfo::PREFIX)
    } else {
        format!("{}{shape}.{division}.{degrees}", lfo::PREFIX)
    };
    lfo::parse(&raw).map(|s| s.key()).ok_or_else(|| {
        format!(
            "no LFO {raw:?}: shapes sine triangle saw square sh; divisions 4bar 2bar bar half beat 8th 16th; degrees 0..359"
        )
    })
}

/// `{key label}` for an LFO source key, or `None` if it is not one.
pub fn lfo_json(key: &str) -> Option<Value> {
    lfo::parse(key).map(|s| json!({"key": s.key(), "label": s.label()}))
}

/// `base` with the args' numeric field edits (snake or kebab case), other
/// keys than the tuning's own refused. `"reset"` is ignored here.
pub fn patch_tuning(base: StructureConfig, a: &Value) -> Result<StructureConfig, String> {
    let Ok(Value::Object(mut fields)) = serde_json::to_value(base) else {
        return Err("structure tuning did not serialize".into());
    };
    if let Value::Object(given) = a {
        for (k, v) in given {
            if k == "reset" {
                continue;
            }
            let key = k.replace('-', "_");
            if !fields.contains_key(&key) {
                let known: Vec<&str> = fields.keys().map(String::as_str).collect();
                return Err(format!(
                    "unknown tuning field :{k}; known: {}",
                    known.join(" ")
                ));
            }
            fields.insert(key, json!(number(v, k)?));
        }
    }
    serde_json::from_value(Value::Object(fields)).map_err(|e| e.to_string())
}

/// `{target, param|trigger, ...where it listens}` for one mapping.
pub fn mapping_json(row: &MappingRow) -> Value {
    let mut v = match &row.target {
        MapTarget::Param(p) => json!({"param": p}),
        MapTarget::Trigger(a) => json!({"trigger": trigger_key(*a)}),
    };
    v["target"] = json!(target_key(&row.target));
    match &row.source {
        MappingSource::Midi(m) => v["midi"] = serde_json::to_value(m).unwrap_or(Value::Null),
        MappingSource::Osc(address) => v["address"] = json!(address),
    }
    v
}
