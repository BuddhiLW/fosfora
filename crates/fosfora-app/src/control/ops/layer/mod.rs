//! Layer ops: everything the stack, layer, param, catalog, media, webcam,
//! post-processing and shader-editor panels do.
//!
//! Promote (layer/effect/device picks, [`LayerEdit`], [`TransportEdit`],
//! param values in real or normalized units) -> Pipeline (pure: names,
//! refusals, unit conversion, JSON views) -> Boundary (one port call per op,
//! over the narrowest port it needs).

#![allow(
    clippy::unnecessary_wraps,
    reason = "every op body has the OpFn signature, failing or not"
)]

pub mod actions;
pub mod port;
#[cfg(test)]
mod tests;

use serde_json::{Map, Value, json};

pub use self::port::{
    EffectPort, EffectView, LayerEdit, LayerKind, LayerPorts, LayerView, MediaPort, ParamPort,
    PostFxPort, StackPort, TransportEdit, TransportView, WebcamList,
};
use crate::control::{CljrsNs, ControlAddon, OpResult, OpSpec, args};
use crate::effect::format::PostProcessDef;
use crate::gpu::layer::BlendMode;
use crate::media::types::PlayDirection;
use crate::params::{ParamDef, ParamValue};

pub struct Addon;

const LAYER: &str = "{:index int | :name str}?";

impl<C: LayerPorts + 'static> ControlAddon<C> for Addon {
    fn id(&self) -> &'static str {
        "layer"
    }

    #[allow(clippy::too_many_lines)]
    fn ops(&self) -> Vec<OpSpec<C>> {
        vec![
            // Stack
            OpSpec::new("layer/list", "{}", "Every layer, top first: index, name, kind, effect, blend, opacity...", list::<C>),
            OpSpec::new("layer/get", LAYER, "One layer (default: the active one).", get::<C>),
            OpSpec::new("layer/add", "{}", "Add an empty layer on top and select it.", add::<C>),
            OpSpec::new("layer/remove", LAYER, "Remove a layer (never the last); returns what was removed.", remove::<C>),
            OpSpec::new("layer/clear", "{}", "Clear the stack back to the launch layers.", clear::<C>),
            OpSpec::new("layer/move", "{:index int | :name str, :to int}", "Move a layer to position :to.", move_::<C>),
            OpSpec::new("layer/select", LAYER, "Make a layer the active one.", select::<C>),
            OpSpec::new(
                "layer/set",
                "{:index|:name?, :opacity 0..1?, :blend name?, :displace 0..1?, :enabled?, :locked?, :pinned?, :rename str|nil?}",
                "Change a layer's own fields; a locked layer refuses blend/opacity/displace/enabled.",
                set::<C>,
            ),
            OpSpec::new("layer/blend-modes", "{}", "Every blend mode by name, in menu order, with its family.", blend_modes::<C>),
            // Effects
            OpSpec::new(
                "layer/effects",
                "{:category str?, :favorites bool?, :query str?, :hidden bool?}",
                "The effects catalog: index, name, category, builtin, favorite.",
                effects::<C>,
            ),
            OpSpec::new(
                "layer/load-effect",
                "{:effect int|str, :index|:name layer?}",
                "Run an effect on a layer (default: the active one).",
                load_effect::<C>,
            ),
            OpSpec::new("layer/favorite", "{:effect int|str, :on bool?}", "Star or unstar an effect (toggles without :on).", favorite::<C>),
            OpSpec::new("layer/create-effect", "{:name str}", "Write a new effect from the template and load it.", create_effect::<C>),
            OpSpec::new(
                "layer/copy-effect",
                "{:as str, :effect int|str?}",
                "Copy an effect (default: the active layer's) to a new user effect and load it.",
                copy_effect::<C>,
            ),
            OpSpec::new("layer/delete-effect", "{:effect int|str}", "Delete a user effect's files.", delete_effect::<C>),
            OpSpec::new("layer/edit-shader", LAYER, "Open a layer's shader in the editor.", edit_shader::<C>),
            OpSpec::new("layer/save-shader", "{}", "Save the shader editor's dirty tabs.", save_shader::<C>),
            // Params
            OpSpec::new("layer/params", LAYER, "A layer's params: type, value, range, default, normalized.", params::<C>),
            OpSpec::new("layer/param", "{:param str, :index|:name layer?}", "One param of a layer.", param::<C>),
            OpSpec::new(
                "layer/set-param",
                "{:param str, :value num|bool|[..], :normalized bool?, :index|:name layer?}",
                "Set a param in real units (or 0..1 with :normalized true); out-of-range is refused.",
                set_param::<C>,
            ),
            OpSpec::new("layer/reset-param", "{:param str?, :index|:name layer?}", "Reset one param (or all) to the effect's defaults.", reset_param::<C>),
            // Media
            OpSpec::new("layer/add-media", "{:path str}", "Decode an image or video into a new layer (appears when ready).", add_media::<C>),
            OpSpec::new("layer/media", LAYER, "A media layer's transport: playing, loop, speed, direction, position.", media::<C>),
            OpSpec::new(
                "layer/transport",
                "{:index|:name?, :toggle?, :playing?, :loop?, :speed num?, :direction forward|reverse|ping-pong?, :seek secs?}",
                "Drive a media layer's transport.",
                transport::<C>,
            ),
            // Webcams
            OpSpec::new("layer/webcams", "{}", "The cameras (and streams) to pick from, and the default.", webcams::<C>),
            OpSpec::new("layer/add-webcam", "{:device int|str?}", "Add a camera layer (default: the next unused camera).", add_webcam::<C>),
            OpSpec::new("layer/switch-webcam", "{:device int|str, :index|:name layer?}", "Point a camera layer at another camera.", switch_webcam::<C>),
            OpSpec::new("layer/mirror", "{:on bool, :index|:name layer?}", "Mirror a camera layer.", mirror::<C>),
            OpSpec::new("layer/webcam-backend", "{:ffmpeg bool}", "Capture cameras through FFmpeg or the native backend.", webcam_backend::<C>),
            OpSpec::new("layer/streams", "{:streams [{:name :url :enabled :listen}]?}", "Read, or replace, the network streams offered as cameras.", streams::<C>),
            // Post-processing
            OpSpec::new("layer/postfx", "{:set {key value}?}", "Master post-processing; :set changes any of its fields.", postfx::<C>),
            OpSpec::new("layer/adopt-postfx", LAYER, "Make a layer's effect's own post-processing Master's.", adopt_postfx::<C>),
            OpSpec::new("layer/postfx-previous", "{}", "Go back to the post-processing the last replacement replaced.", postfx_previous::<C>),
            OpSpec::new("layer/postfx-defaults", "{}", "Reset Master post-processing to the defaults.", postfx_defaults::<C>),
        ]
    }

    fn ui_signals(&self) -> &'static [(&'static str, &'static str)] {
        &[
            ("add_layer", "layer/add"),
            ("remove_layer", "layer/remove"),
            ("clear_all_layers", "layer/clear"),
            ("layer_move", "layer/move"),
            ("select_layer", "layer/select"),
            ("layer_rename", "layer/set"),
            ("layer_blend", "layer/set"),
            ("layer_opacity", "layer/set"),
            ("layer_displace", "layer/set"),
            ("layer_toggle_enable", "layer/set"),
            ("layer_toggle_lock", "layer/set"),
            ("layer_toggle_pin", "layer/set"),
            ("pending_effect", "layer/load-effect"),
            ("toggle_favorite_effect", "layer/favorite"),
            ("create_new_effect", "layer/create-effect"),
            ("new_effect_prompt", "layer/create-effect"),
            ("create_copy_effect", "layer/copy-effect"),
            ("copy_builtin_prompt", "layer/copy-effect"),
            ("delete_effect", "layer/delete-effect"),
            ("open_shader_editor", "layer/edit-shader"),
            ("shader_editor_save", "layer/save-shader"),
            ("add_media_layer", "layer/add-media"),
            ("media_play_pause", "layer/transport"),
            ("media_loop", "layer/transport"),
            ("media_speed", "layer/transport"),
            ("media_direction", "layer/transport"),
            ("media_seek", "layer/transport"),
            ("add_webcam_layer", "layer/add-webcam"),
            ("switch_webcam_device", "layer/switch-webcam"),
            ("webcam_mirror", "layer/mirror"),
            ("webcam_disconnect", "layer/remove"),
            ("set_ffmpeg_webcam", "layer/webcam-backend"),
            ("set_rtmp_streams", "layer/streams"),
            ("adopt_layer_postprocess", "layer/adopt-postfx"),
        ]
    }

    fn cljrs(&self) -> Option<CljrsNs> {
        Some(CljrsNs {
            ns: "fosfora.layer",
            alias: "layer",
            src: include_str!("layer.cljrs"),
        })
    }
}

// ── Promote ──────────────────────────────────────────────────────────────

/// A number arg, if present; present but not a finite number is an error.
fn opt_num(a: &Value, key: &str) -> Result<Option<f64>, String> {
    match a.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|n| n.is_finite())
            .map(Some)
            .ok_or_else(|| format!(":{key} must be a number, got {v}")),
    }
}

/// A bool arg, if present; present but not a bool is an error.
fn opt_flag(a: &Value, key: &str) -> Result<Option<bool>, String> {
    match a.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(v) => Err(format!(":{key} must be true or false, got {v}")),
    }
}

/// A 0..1 arg, if present.
#[allow(clippy::cast_possible_truncation)]
fn opt_unit(a: &Value, key: &str) -> Result<Option<f32>, String> {
    match opt_num(a, key)? {
        Some(v) if (0.0..=1.0).contains(&v) => Ok(Some(v as f32)),
        Some(v) => Err(format!(":{key} must be within 0..1, got {v}")),
        None => Ok(None),
    }
}

/// `v` (an index or a name) against `names`, like [`args::pick`].
fn pick_in<S: AsRef<str>>(v: &Value, names: &[S], what: &str) -> Result<usize, String> {
    match v {
        Value::Number(_) => args::pick(&json!({ "index": v }), names, what),
        Value::String(_) => args::pick(&json!({ "name": v }), names, what),
        _ => Err(format!("give the {what} as an index or a name, got {v}")),
    }
}

/// The layer `{:index n}` or `{:name "x"}` names: by label, then by what it
/// shows (effect or file). Neither key: the active layer.
fn layer_target<P: StackPort>(port: &P, a: &Value) -> Result<usize, String> {
    if a.get("index").is_none() && a.get("name").is_none() {
        return Ok(port.active_layer());
    }
    pick_layer(&port.layers(), a)
}

/// [`layer_target`]'s pick, over a listing.
pub fn pick_layer(views: &[LayerView], a: &Value) -> Result<usize, String> {
    let labels: Vec<&str> = views.iter().map(LayerView::label).collect();
    match args::pick(a, &labels, "layer") {
        Ok(i) => Ok(i),
        Err(e) if a.get("index").is_none() => {
            let shows: Vec<&str> = views
                .iter()
                .map(|v| v.effect.as_deref().or(v.media.as_deref()).unwrap_or(""))
                .collect();
            args::pick(a, &shows, "layer").map_err(|_| e)
        }
        Err(e) => Err(e),
    }
}

/// A blend mode by name (`"color-dodge"`, `"Color Dodge"`, any case) or by
/// its wire number.
pub fn blend_from_value(v: &Value) -> Result<BlendMode, String> {
    match v {
        Value::String(s) => blend_from_name(s),
        Value::Number(n) => n
            .as_u64()
            .and_then(|i| usize::try_from(i).ok())
            .and_then(|i| BlendMode::ALL.get(i).copied())
            .ok_or_else(|| format!("no blend mode number {n}")),
        _ => Err(format!("a blend mode is a name, got {v}")),
    }
}

/// [`LayerEdit`] from `layer/set` args; every field is optional.
pub fn layer_edit_from(a: &Value) -> Result<LayerEdit, String> {
    let name = match a.get("rename") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(s)) => Some((!s.trim().is_empty()).then(|| s.trim().to_owned())),
        Some(v) => Err(format!(":rename must be a string or nil, got {v}"))?,
    };
    Ok(LayerEdit {
        opacity: opt_unit(a, "opacity")?,
        blend: a.get("blend").map(blend_from_value).transpose()?,
        displace: opt_unit(a, "displace")?,
        enabled: opt_flag(a, "enabled")?,
        locked: opt_flag(a, "locked")?,
        pinned: opt_flag(a, "pinned")?,
        name,
    })
}

/// [`TransportEdit`] from `layer/transport` args.
#[allow(clippy::cast_possible_truncation)]
pub fn transport_edit_from(a: &Value) -> Result<TransportEdit, String> {
    let speed = match opt_num(a, "speed")? {
        Some(s) if s > 0.0 && s <= 16.0 => Some(s as f32),
        Some(s) => return Err(format!(":speed must be within (0, 16], got {s}")),
        None => None,
    };
    let seek_secs = match opt_num(a, "seek")? {
        Some(s) if s >= 0.0 => Some(s),
        Some(s) => return Err(format!(":seek must be a non-negative number of seconds, got {s}")),
        None => None,
    };
    let direction = match a.get("direction") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(direction_from_name(s)?),
        Some(v) => return Err(format!(":direction must be forward, reverse or ping-pong, got {v}")),
    };
    Ok(TransportEdit {
        toggle: opt_flag(a, "toggle")?.unwrap_or(false),
        playing: opt_flag(a, "playing")?,
        looping: opt_flag(a, "loop")?,
        speed,
        direction,
        seek_secs,
    })
}

// ── Pipeline (pure) ──────────────────────────────────────────────────────

fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// A blend mode's op name: its display name in kebab case (`color-dodge`).
pub fn blend_name(mode: BlendMode) -> String {
    mode.display_name().to_lowercase().replace(' ', "-")
}

/// The blend mode named `name`, ignoring case, spaces, dashes, underscores.
pub fn blend_from_name(name: &str) -> Result<BlendMode, String> {
    let want = squash(name);
    BlendMode::ALL
        .iter()
        .copied()
        .find(|m| squash(m.display_name()) == want)
        .ok_or_else(|| {
            let all: Vec<String> = BlendMode::MENU.iter().map(|m| blend_name(*m)).collect();
            format!("no blend mode {name:?}; one of {}", all.join(", "))
        })
}

pub fn direction_name(d: PlayDirection) -> &'static str {
    match d {
        PlayDirection::Forward => "forward",
        PlayDirection::Reverse => "reverse",
        PlayDirection::PingPong => "ping-pong",
    }
}

pub fn direction_from_name(name: &str) -> Result<PlayDirection, String> {
    match squash(name).as_str() {
        "forward" => Ok(PlayDirection::Forward),
        "reverse" => Ok(PlayDirection::Reverse),
        "pingpong" => Ok(PlayDirection::PingPong),
        _ => Err(format!("no direction {name:?}; forward, reverse or ping-pong")),
    }
}

pub fn kind_name(kind: LayerKind) -> &'static str {
    match kind {
        LayerKind::Effect => "effect",
        LayerKind::Media => "media",
        LayerKind::Webcam => "webcam",
    }
}

/// Why `edit` may not touch layer `index`, if it may not: a locked layer
/// keeps its blend, opacity, displace and switch unless the edit unlocks it.
pub fn edit_refusal(index: usize, locked: bool, edit: &LayerEdit) -> Option<String> {
    let guarded = edit.opacity.is_some()
        || edit.blend.is_some()
        || edit.displace.is_some()
        || edit.enabled.is_some();
    (locked && guarded && edit.locked != Some(false))
        .then(|| format!("layer {index} is locked; unlock it (:locked false) to change it"))
}

pub fn layer_json(index: usize, v: &LayerView, active: bool) -> Value {
    json!({
        "index": index,
        "name": v.label(),
        "custom-name": v.custom_name,
        "kind": kind_name(v.kind),
        "effect": v.effect,
        "media": v.media,
        "blend": blend_name(v.blend),
        "opacity": v.opacity,
        "displace": v.displace,
        "enabled": v.enabled,
        "locked": v.locked,
        "pinned": v.pinned,
        "active": active,
        "shader-error": v.shader_error,
    })
}

pub fn effect_json(index: usize, e: &EffectView) -> Value {
    json!({
        "index": index,
        "name": e.name,
        "category": e.category,
        "builtin": e.builtin,
        "favorite": e.favorite,
    })
}

pub fn transport_json(layer: usize, t: &TransportView) -> Value {
    json!({
        "layer": layer,
        "playing": t.playing,
        "loop": t.looping,
        "speed": t.speed,
        "direction": direction_name(t.direction),
        "position": t.position_secs,
        "duration": t.duration_secs,
        "video": t.video,
        "live": t.live,
    })
}

/// The indices of `effects` a catalog query keeps, in catalog order.
pub fn filter_effects(
    effects: &[EffectView],
    category: Option<&str>,
    favorites: bool,
    query: Option<&str>,
    hidden: bool,
) -> Vec<usize> {
    let query = query.map(str::to_lowercase);
    effects
        .iter()
        .enumerate()
        .filter(|(_, e)| hidden || !e.hidden)
        .filter(|(_, e)| !favorites || e.favorite)
        .filter(|(_, e)| category.is_none_or(|c| e.category.eq_ignore_ascii_case(c)))
        .filter(|(_, e)| {
            query
                .as_deref()
                .is_none_or(|q| e.name.to_lowercase().contains(q))
        })
        .map(|(i, _)| i)
        .collect()
}

fn type_name(def: &ParamDef) -> &'static str {
    match def {
        ParamDef::Int { .. } => "int",
        ParamDef::Float { .. } => "float",
        ParamDef::Color { .. } => "color",
        ParamDef::Bool { .. } => "bool",
        ParamDef::Point2D { .. } => "point",
    }
}

pub fn value_json(v: &ParamValue) -> Value {
    match v {
        ParamValue::Float(f) => json!(f),
        ParamValue::Bool(b) => json!(b),
        ParamValue::Color(c) => json!(c),
        ParamValue::Point2D(p) => json!(p),
    }
}

fn unit_of(x: f32, lo: f32, hi: f32) -> f32 {
    if hi > lo { (x - lo) / (hi - lo) } else { 0.0 }
}

fn real_of(t: f32, lo: f32, hi: f32) -> f32 {
    lo + t * (hi - lo)
}

/// `v` as 0..1 of `def`'s range (Float and Point2D only).
pub fn normalize(def: &ParamDef, v: &ParamValue) -> Option<Value> {
    match (def, v) {
        (ParamDef::Float { min, max, .. }, ParamValue::Float(x)) => Some(json!(unit_of(*x, *min, *max))),
        #[allow(clippy::cast_precision_loss)]
        (ParamDef::Int { min, max, .. }, ParamValue::Float(x)) => {
            Some(json!(unit_of(*x, *min as f32, *max as f32)))
        }
        (ParamDef::Point2D { min, max, .. }, ParamValue::Point2D(p)) => Some(json!([
            unit_of(p[0], min[0], max[0]),
            unit_of(p[1], min[1], max[1])
        ])),
        _ => None,
    }
}

/// `{name type value default min max normalized}` for one param.
pub fn param_json(def: &ParamDef, value: Option<&ParamValue>) -> Value {
    let default = def.default_value();
    let value = value.unwrap_or(&default);
    let mut out = json!({
        "name": def.name(),
        "type": type_name(def),
        "value": value_json(value),
        "default": value_json(&default),
        "normalized": normalize(def, value),
    });
    match def {
        ParamDef::Int { min, max, .. } => {
            out["min"] = json!(min);
            out["max"] = json!(max);
        }
        ParamDef::Float { min, max, .. } => {
            out["min"] = json!(min);
            out["max"] = json!(max);
        }
        ParamDef::Point2D { min, max, .. } => {
            out["min"] = json!(min);
            out["max"] = json!(max);
        }
        _ => {}
    }
    out
}

#[allow(clippy::cast_possible_truncation)]
fn num_in(v: &Value, what: &str) -> Result<f32, String> {
    v.as_f64()
        .filter(|n| n.is_finite())
        .map(|n| n as f32)
        .ok_or_else(|| format!("{what} must be a finite number, got {v}"))
}

fn ranged(x: f32, lo: f32, hi: f32, normalized: bool, what: &str) -> Result<f32, String> {
    if normalized {
        if !(0.0..=1.0).contains(&x) {
            return Err(format!("{what}: normalized value {x} is outside 0..1"));
        }
        return Ok(real_of(x, lo, hi));
    }
    let (a, b) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    if (a..=b).contains(&x) {
        Ok(x)
    } else {
        Err(format!("{what}: {x} is outside {lo}..{hi}"))
    }
}

fn components<const N: usize>(v: &Value, what: &str) -> Result<[f32; N], String> {
    let arr = v
        .as_array()
        .filter(|a| a.len() == N)
        .ok_or_else(|| format!("{what} must be a vector of {N} numbers, got {v}"))?;
    let mut out = [0.0; N];
    for (slot, x) in out.iter_mut().zip(arr) {
        *slot = num_in(x, what)?;
    }
    Ok(out)
}

/// `raw` as a value of `def`: real units, or 0..1 of the range with
/// `normalized` (Float, Point2D). Colors are `[r g b]` or `[r g b a]` in 0..1;
/// bools are booleans (or, normalized, a number over 0.5).
pub fn promote_param(def: &ParamDef, raw: &Value, normalized: bool) -> Result<ParamValue, String> {
    let what = def.name();
    match def {
        // Stored as f32 like every scalar (see `ParamDef::default_value`), rounded.
        #[allow(clippy::cast_precision_loss)]
        ParamDef::Int { min, max, .. } => {
            let (lo, hi) = (*min as f32, *max as f32);
            let x = ranged(num_in(raw, what)?, lo, hi, normalized, what)?;
            Ok(ParamValue::Float(x.round().clamp(lo.min(hi), hi.max(lo))))
        }
        ParamDef::Float { min, max, .. } => {
            Ok(ParamValue::Float(ranged(num_in(raw, what)?, *min, *max, normalized, what)?))
        }
        ParamDef::Bool { .. } => match raw {
            Value::Bool(b) => Ok(ParamValue::Bool(*b)),
            Value::Number(_) if normalized => Ok(ParamValue::Bool(num_in(raw, what)? > 0.5)),
            _ => Err(format!("{what} is a bool; give true or false, got {raw}")),
        },
        ParamDef::Point2D { min, max, .. } => {
            let [x, y] = components::<2>(raw, what)?;
            Ok(ParamValue::Point2D([
                ranged(x, min[0], max[0], normalized, what)?,
                ranged(y, min[1], max[1], normalized, what)?,
            ]))
        }
        ParamDef::Color { .. } => {
            let c = match raw.as_array().map(Vec::len) {
                Some(3) => {
                    let [r, g, b] = components::<3>(raw, what)?;
                    [r, g, b, 1.0]
                }
                _ => components::<4>(raw, what)?,
            };
            if c.iter().all(|x| (0.0..=1.0).contains(x)) {
                Ok(ParamValue::Color(c))
            } else {
                Err(format!("{what}: color components must be within 0..1"))
            }
        }
    }
}

/// `base` with the fields `patch` names changed. Keys are the saved names
/// (`bloom_intensity`; `bloom-intensity` also works); an unknown key or a
/// value of the wrong type is refused.
pub fn merge_postfx(base: &PostProcessDef, patch: &Value) -> Result<PostProcessDef, String> {
    let patch = patch
        .as_object()
        .ok_or_else(|| format!(":set must be a map, got {patch}"))?;
    let mut merged = match serde_json::to_value(base) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    };
    for (k, v) in patch {
        let key = k.replace('-', "_");
        if !merged.contains_key(&key) {
            let known: Vec<&String> = merged.keys().collect();
            return Err(format!("no post-processing field {k:?}; one of {known:?}"));
        }
        merged.insert(key, v.clone());
    }
    serde_json::from_value(Value::Object(merged)).map_err(|e| format!("bad post-processing value: {e}"))
}

fn postfx_json<P: PostFxPort>(port: &P) -> Value {
    let mut v = serde_json::to_value(port.postprocess()).unwrap_or(Value::Null);
    if let Value::Object(m) = &mut v {
        m.insert("has-previous".into(), json!(port.has_previous_postprocess()));
    }
    v
}

// ── Boundary ─────────────────────────────────────────────────────────────

fn view_at<P: StackPort>(port: &P, index: usize) -> Value {
    let active = port.active_layer();
    port.layers()
        .get(index)
        .map_or(Value::Null, |v| layer_json(index, v, index == active))
}

fn listing<P: StackPort>(port: &P) -> Value {
    let active = port.active_layer();
    Value::Array(
        port.layers()
            .iter()
            .enumerate()
            .map(|(i, v)| layer_json(i, v, i == active))
            .collect(),
    )
}

fn list<P: StackPort>(port: &mut P, _: &Value) -> OpResult {
    Ok(listing(port))
}

fn get<P: StackPort>(port: &mut P, a: &Value) -> OpResult {
    let i = layer_target(port, a)?;
    Ok(view_at(port, i))
}

fn add<P: StackPort>(port: &mut P, _: &Value) -> OpResult {
    let i = port.add_layer()?;
    Ok(view_at(port, i))
}

fn remove<P: StackPort>(port: &mut P, a: &Value) -> OpResult {
    let i = layer_target(port, a)?;
    if port.layers().len() <= 1 {
        return Err("the last layer cannot be removed".into());
    }
    let gone = view_at(port, i);
    port.remove_layer(i)?;
    Ok(gone)
}

fn clear<P: StackPort>(port: &mut P, _: &Value) -> OpResult {
    port.clear_layers();
    Ok(listing(port))
}

fn move_<P: StackPort>(port: &mut P, a: &Value) -> OpResult {
    let from = layer_target(port, a)?;
    let to = args::usize(a, "to")?;
    let len = port.layers().len();
    if to >= len {
        return Err(format!(":to {to} out of range (have {len} layers)"));
    }
    port.move_layer(from, to);
    Ok(view_at(port, to))
}

fn select<P: StackPort>(port: &mut P, a: &Value) -> OpResult {
    let i = layer_target(port, a)?;
    port.select_layer(i);
    Ok(view_at(port, i))
}

fn set<P: StackPort>(port: &mut P, a: &Value) -> OpResult {
    let i = layer_target(port, a)?;
    let edit = layer_edit_from(a)?;
    if edit != LayerEdit::default() {
        port.edit_layer(i, &edit)?;
    }
    Ok(view_at(port, i))
}

fn blend_modes<P>(_: &mut P, _: &Value) -> OpResult {
    Ok(Value::Array(
        BlendMode::MENU
            .iter()
            .map(|m| {
                json!({
                    "name": blend_name(*m),
                    "display": m.display_name(),
                    "family": if m.is_displacement() { "displacement" } else { "color" },
                    "description": m.description(),
                })
            })
            .collect(),
    ))
}

fn effect_names<P: EffectPort>(port: &P) -> (Vec<EffectView>, Vec<String>) {
    let effects = port.effects();
    let names = effects.iter().map(|e| e.name.clone()).collect();
    (effects, names)
}

fn effect_arg<P: EffectPort>(port: &P, a: &Value) -> Result<(usize, Vec<EffectView>), String> {
    let v = a.get("effect").ok_or("missing arg :effect (an index or a name)")?;
    let (effects, names) = effect_names(port);
    Ok((pick_in(v, &names, "effect")?, effects))
}

fn effects<P: EffectPort>(port: &mut P, a: &Value) -> OpResult {
    let all = port.effects();
    let keep = filter_effects(
        &all,
        args::opt_str(a, "category"),
        opt_flag(a, "favorites")?.unwrap_or(false),
        args::opt_str(a, "query"),
        opt_flag(a, "hidden")?.unwrap_or(false),
    );
    Ok(Value::Array(keep.into_iter().map(|i| effect_json(i, &all[i])).collect()))
}

fn load_effect<P: StackPort + EffectPort>(port: &mut P, a: &Value) -> OpResult {
    let (effect, _) = effect_arg(port, a)?;
    let layer = layer_target(port, a)?;
    port.load_effect(layer, effect)?;
    Ok(view_at(port, layer))
}

fn favorite<P: EffectPort>(port: &mut P, a: &Value) -> OpResult {
    let (i, effects) = effect_arg(port, a)?;
    let on = opt_flag(a, "on")?.unwrap_or(!effects[i].favorite);
    port.set_favorite(&effects[i].name, on);
    Ok(json!({"index": i, "name": effects[i].name, "favorite": on}))
}

fn found<P: EffectPort>(port: &P, name: &str) -> Value {
    let effects = port.effects();
    effects
        .iter()
        .position(|e| e.name == name)
        .map_or_else(|| json!({"name": name}), |i| effect_json(i, &effects[i]))
}

fn fresh_name(effects: &[EffectView], raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err("an effect name cannot be empty".into());
    }
    if effects.iter().any(|e| e.name.eq_ignore_ascii_case(name)) {
        return Err(format!("an effect named {name:?} already exists"));
    }
    Ok(name.to_owned())
}

fn create_effect<P: EffectPort>(port: &mut P, a: &Value) -> OpResult {
    let name = fresh_name(&port.effects(), args::str(a, "name")?)?;
    port.create_effect(&name)?;
    Ok(found(port, &name))
}

fn copy_effect<P: StackPort + EffectPort>(port: &mut P, a: &Value) -> OpResult {
    let (effects, names) = effect_names(port);
    let name = fresh_name(&effects, args::str(a, "as")?)?;
    let source = match a.get("effect") {
        Some(v) => pick_in(v, &names, "effect")?,
        None => {
            let active = port.active_layer();
            let shown = port.layers().get(active).and_then(|l| l.effect.clone());
            shown
                .and_then(|n| names.iter().position(|e| *e == n))
                .ok_or("the active layer runs no effect; give :effect")?
        }
    };
    port.copy_effect(source, &name)?;
    let mut out = found(port, &name);
    out["from"] = json!(names[source]);
    Ok(out)
}

fn delete_effect<P: EffectPort>(port: &mut P, a: &Value) -> OpResult {
    let (i, effects) = effect_arg(port, a)?;
    if effects[i].builtin {
        return Err(format!("{} is built in and cannot be deleted", effects[i].name));
    }
    let name = port.delete_effect(i)?;
    Ok(json!({"index": i, "name": name}))
}

fn edit_shader<P: StackPort + EffectPort>(port: &mut P, a: &Value) -> OpResult {
    let layer = layer_target(port, a)?;
    let path = port.open_shader_editor(layer)?;
    let effect = port.layers().get(layer).and_then(|l| l.effect.clone());
    Ok(json!({"layer": layer, "effect": effect, "path": path}))
}

fn save_shader<P: EffectPort>(port: &mut P, _: &Value) -> OpResult {
    Ok(json!({"saved": port.save_shader_editor()?}))
}

fn param_def<P: ParamPort>(port: &P, layer: usize, a: &Value) -> Result<ParamDef, String> {
    let defs = port.param_defs(layer);
    let names: Vec<&str> = defs.iter().map(ParamDef::name).collect();
    let v = a.get("param").ok_or("missing arg :param (a param name)")?;
    let i = pick_in(v, &names, "param")?;
    Ok(defs[i].clone())
}

fn one_param<P: ParamPort>(port: &P, layer: usize, def: &ParamDef) -> Value {
    let mut v = param_json(def, port.param_value(layer, def.name()).as_ref());
    v["layer"] = json!(layer);
    v
}

fn params<P: StackPort + ParamPort>(port: &mut P, a: &Value) -> OpResult {
    let layer = layer_target(port, a)?;
    let list: Vec<Value> = port
        .param_defs(layer)
        .iter()
        .map(|d| param_json(d, port.param_value(layer, d.name()).as_ref()))
        .collect();
    let effect = port.layers().get(layer).and_then(|l| l.effect.clone());
    Ok(json!({"layer": layer, "effect": effect, "params": list}))
}

fn param<P: StackPort + ParamPort>(port: &mut P, a: &Value) -> OpResult {
    let layer = layer_target(port, a)?;
    let def = param_def(port, layer, a)?;
    Ok(one_param(port, layer, &def))
}

fn set_param<P: StackPort + ParamPort>(port: &mut P, a: &Value) -> OpResult {
    let layer = layer_target(port, a)?;
    let def = param_def(port, layer, a)?;
    let raw = a.get("value").ok_or("missing arg :value")?;
    let value = promote_param(&def, raw, opt_flag(a, "normalized")?.unwrap_or(false))?;
    port.set_param(layer, def.name(), value);
    Ok(one_param(port, layer, &def))
}

fn reset_param<P: StackPort + ParamPort>(port: &mut P, a: &Value) -> OpResult {
    let layer = layer_target(port, a)?;
    if a.get("param").is_none() {
        port.reset_params(layer, None);
        return params(port, a);
    }
    let def = param_def(port, layer, a)?;
    port.reset_params(layer, Some(def.name()));
    Ok(one_param(port, layer, &def))
}

/// `~/x` against `$HOME`.
fn expand_home(path: &str) -> String {
    match (path.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}/{rest}"),
        _ => path.to_owned(),
    }
}

fn add_media<P: MediaPort>(port: &mut P, a: &Value) -> OpResult {
    let path = expand_home(args::str(a, "path")?);
    port.add_media(&path)?;
    Ok(json!({"path": path, "pending": true}))
}

fn media_layer<P: StackPort + MediaPort>(port: &P, a: &Value) -> Result<(usize, TransportView), String> {
    let layer = layer_target(port, a)?;
    let t = port
        .transport(layer)
        .ok_or_else(|| format!("layer {layer} is not a media layer"))?;
    Ok((layer, t))
}

fn media<P: StackPort + MediaPort>(port: &mut P, a: &Value) -> OpResult {
    let (layer, t) = media_layer(port, a)?;
    Ok(transport_json(layer, &t))
}

fn transport<P: StackPort + MediaPort>(port: &mut P, a: &Value) -> OpResult {
    let edit = transport_edit_from(a)?;
    let (layer, t) = media_layer(port, a)?;
    if edit == TransportEdit::default() {
        return Ok(transport_json(layer, &t));
    }
    port.edit_transport(layer, &edit)?;
    let t = port.transport(layer).ok_or("the media layer went away")?;
    Ok(transport_json(layer, &t))
}

fn webcam_json(list: &WebcamList) -> Value {
    json!({
        "devices": list.devices.iter().map(|(i, n)| json!({"index": i, "name": n})).collect::<Vec<_>>(),
        "default": list.default,
        "ffmpeg": list.ffmpeg,
    })
}

fn webcams<P: MediaPort>(port: &mut P, _: &Value) -> OpResult {
    Ok(webcam_json(&port.webcams()?))
}

/// The device `{:device n | "name"}` names: a device index as listed, or a
/// name.
fn device_arg(list: &WebcamList, v: &Value) -> Result<(u32, String), String> {
    if let Some(n) = v.as_u64() {
        return list
            .devices
            .iter()
            .find(|(i, _)| u64::from(*i) == n)
            .cloned()
            .ok_or_else(|| format!("no camera with device index {n}"));
    }
    let names: Vec<&str> = list.devices.iter().map(|(_, n)| n.as_str()).collect();
    let i = pick_in(v, &names, "camera")?;
    Ok(list.devices[i].clone())
}

fn add_webcam<P: StackPort + MediaPort>(port: &mut P, a: &Value) -> OpResult {
    let device = match a.get("device") {
        Some(v) => Some(device_arg(&port.webcams()?, v)?.1),
        None => None,
    };
    let shown = port.add_webcam(device.as_deref())?;
    let mut out = view_at(port, port.active_layer());
    out["device"] = json!(shown);
    Ok(out)
}

fn switch_webcam<P: StackPort + MediaPort>(port: &mut P, a: &Value) -> OpResult {
    let layer = layer_target(port, a)?;
    if port.layers().get(layer).map(|l| l.kind) != Some(LayerKind::Webcam) {
        return Err(format!("layer {layer} is not a camera layer"));
    }
    let v = a.get("device").ok_or("missing arg :device (an index or a name)")?;
    let (device, _) = device_arg(&port.webcams()?, v)?;
    let name = port.switch_webcam(layer, device)?;
    Ok(json!({"layer": layer, "device": name}))
}

fn mirror<P: StackPort + MediaPort>(port: &mut P, a: &Value) -> OpResult {
    let layer = layer_target(port, a)?;
    let on = args::bool(a, "on")?;
    port.set_mirror(layer, on)?;
    Ok(json!({"layer": layer, "mirror": on}))
}

fn webcam_backend<P: MediaPort>(port: &mut P, a: &Value) -> OpResult {
    let on = args::bool(a, "ffmpeg")?;
    let n = port.set_ffmpeg_webcam(on)?;
    Ok(json!({"ffmpeg": on, "devices": n}))
}

fn streams<P: MediaPort>(port: &mut P, a: &Value) -> OpResult {
    if let Some(v) = a.get("streams") {
        let streams: Vec<crate::settings::RtmpStream> = serde_json::from_value(v.clone())
            .map_err(|e| format!(":streams must be [{{:name :url :enabled? :listen?}}]: {e}"))?;
        port.set_rtmp_streams(streams)?;
    }
    serde_json::to_value(port.rtmp_streams()).map_err(|e| e.to_string())
}

fn postfx<P: PostFxPort>(port: &mut P, a: &Value) -> OpResult {
    if let Some(patch) = a.get("set") {
        let next = merge_postfx(&port.postprocess(), patch)?;
        port.replace_postprocess(next);
    }
    Ok(postfx_json(port))
}

fn adopt_postfx<P: StackPort + PostFxPort>(port: &mut P, a: &Value) -> OpResult {
    let layer = layer_target(port, a)?;
    port.adopt_postprocess(layer);
    Ok(postfx_json(port))
}

fn postfx_previous<P: PostFxPort>(port: &mut P, _: &Value) -> OpResult {
    if !port.has_previous_postprocess() {
        return Err("there is no earlier post-processing to go back to".into());
    }
    port.restore_previous_postprocess();
    Ok(postfx_json(port))
}

fn postfx_defaults<P: PostFxPort>(port: &mut P, _: &Value) -> OpResult {
    port.replace_postprocess(PostProcessDef::default());
    Ok(postfx_json(port))
}
