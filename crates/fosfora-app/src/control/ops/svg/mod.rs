//! SVG Offset ops: everything the SVG Offset panel does on an SVG Offset
//! layer (source file, quality, background, offset, rotation, keyer).
//!
//! Promote (`layer`, [`SettingsEdit`] and the path from args) -> Pipeline
//! (pure: [`SettingsEdit::apply`], [`state_json`]) -> Boundary (one
//! [`SvgPort::apply_svg`] call per op, the same action the panel runs).
//! Amount, motion, palette and order are ordinary effect params (`layer/*`).

mod actions;
pub mod port;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

use serde_json::{Value, json};

pub use self::port::SvgPort;
use super::particle::pipeline::expand_home;
use crate::control::{CljrsNs, ControlAddon, OpResult, OpSpec};
use crate::gpu::svg_offset::SvgOffsetSettings;
use crate::ui::panels::svg_offset_panel::{SvgOffsetCommand, SvgOffsetInfo};

pub struct Addon;

impl<C: SvgPort + 'static> ControlAddon<C> for Addon {
    fn id(&self) -> &'static str {
        "svg"
    }

    fn ops(&self) -> Vec<OpSpec<C>> {
        vec![
            OpSpec::new(
                "svg/state",
                "{:layer int?}",
                "The SVG Offset of a layer (default active): source, shape count, settings.",
                state::<C>,
            ),
            OpSpec::new(
                "svg/set",
                "{:layer int?, :quality full|half|quarter?, :background alpha|black|white?, \
                 :offset [x y]?, :offset-x num?, :offset-y num?, :rotation num?, \
                 :key-width num?, :key-phase num?, :key-soft num?, :key-repeat num?}",
                "Change SVG Offset settings; a new quality re-rasterizes the SVG.",
                set::<C>,
            ),
            OpSpec::new(
                "svg/load",
                "{:layer int?, :path str?}",
                "Load an SVG file (a leading ~ is home); no :path loads the built-in ornament.",
                load::<C>,
            ),
            OpSpec::new(
                "svg/reload",
                "{:layer int?}",
                "Re-read the current SVG source from disk.",
                reload::<C>,
            ),
        ]
    }

    fn ui_signals(&self) -> &'static [(&'static str, &'static str)] {
        &[("svg_offset_cmd", "svg/set")]
    }

    fn cljrs(&self) -> Option<CljrsNs> {
        Some(CljrsNs {
            ns: "fosfora.svg",
            alias: "svg",
            src: include_str!("svg.cljrs"),
        })
    }
}

// ── Promote ─────────────────────────────────────────────────────────────────

/// Quality divisors the panel offers, by name.
pub const QUALITIES: &[(u32, &str)] = &[(1, "full"), (2, "half"), (4, "quarter")];
/// Backgrounds the panel offers, by name, with their stored value.
pub const BACKGROUNDS: &[(u32, &str)] = &[(0, "alpha"), (1, "black"), (2, "white")];

/// The layer `{:layer n}` names, else the active one.
pub fn layer<P: SvgPort>(port: &P, a: &Value) -> Result<usize, String> {
    match a.get("layer") {
        None | Some(Value::Null) => Ok(port.active_layer()),
        Some(v) => {
            let n = v
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or(":layer must be a non-negative integer")?;
            if n < port.layer_count() {
                Ok(n)
            } else {
                Err(format!(
                    "layer {n} out of range (have {})",
                    port.layer_count()
                ))
            }
        }
    }
}

/// `v` as a number inside `lo..=hi` (the panel slider's range).
fn in_range(v: &Value, lo: f32, hi: f32) -> Option<f32> {
    #[allow(clippy::cast_possible_truncation)]
    v.as_f64()
        .filter(|x| x.is_finite() && (f64::from(lo)..=f64::from(hi)).contains(x))
        .map(|x| x as f32)
}

/// A number arg inside `lo..=hi`, if present.
fn num(a: &Value, key: &str, lo: f32, hi: f32) -> Result<Option<f32>, String> {
    match a.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => in_range(v, lo, hi)
            .map(Some)
            .ok_or_else(|| format!(":{key} must be a number {lo}..={hi}, got {v}")),
    }
}

/// `{:offset [x y]}`, if present, each in `-1..=1`.
fn offset_pair(a: &Value) -> Result<Option<(f32, f32)>, String> {
    let Some(v) = a.get("offset").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let bad = || format!(":offset must be [x y] with each -1..=1, got {v}");
    match v.as_array().map(Vec::as_slice) {
        Some([x, y]) => match (in_range(x, -1.0, 1.0), in_range(y, -1.0, 1.0)) {
            (Some(x), Some(y)) => Ok(Some((x, y))),
            _ => Err(bad()),
        },
        _ => Err(bad()),
    }
}

/// A choice given by name (any case) or by its stored number.
fn choice(a: &Value, key: &str, table: &[(u32, &str)]) -> Result<Option<u32>, String> {
    let Some(v) = a.get(key).filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let hit = match v {
        Value::String(s) => table.iter().find(|(_, n)| n.eq_ignore_ascii_case(s.trim())),
        _ => v
            .as_u64()
            .and_then(|n| table.iter().find(|(value, _)| u64::from(*value) == n)),
    };
    hit.map(|(value, _)| Some(*value)).ok_or_else(|| {
        let names: Vec<&str> = table.iter().map(|(_, n)| *n).collect();
        format!(":{key} must be one of {names:?}, got {v}")
    })
}

/// `{:path "..."}` as a path (a leading `~` is the home folder), if given.
fn path(a: &Value) -> Result<Option<String>, String> {
    match a.get("path") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if !s.trim().is_empty() => {
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let path = expand_home(s.trim(), home.as_deref());
            Ok(Some(path.to_string_lossy().into_owned()))
        }
        Some(v) => Err(format!(":path must be a file path, got {v}")),
    }
}

/// The settings `svg/set` changes; `None` keeps the current value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettingsEdit {
    pub quality: Option<u32>,
    pub background: Option<u32>,
    pub offset_x: Option<f32>,
    pub offset_y: Option<f32>,
    pub rotation: Option<f32>,
    pub key_width: Option<f32>,
    pub key_phase: Option<f32>,
    pub key_soft: Option<f32>,
    pub key_repeat: Option<f32>,
}

impl SettingsEdit {
    /// Read an edit from args. A key present with the wrong type or outside
    /// the panel's range is an error, never silently dropped. `:offset-x` /
    /// `:offset-y` win over the matching half of `:offset`.
    pub fn from_args(a: &Value) -> Result<Self, String> {
        let pair = offset_pair(a)?;
        Ok(Self {
            quality: choice(a, "quality", QUALITIES)?,
            background: choice(a, "background", BACKGROUNDS)?,
            offset_x: num(a, "offset-x", -1.0, 1.0)?.or(pair.map(|p| p.0)),
            offset_y: num(a, "offset-y", -1.0, 1.0)?.or(pair.map(|p| p.1)),
            rotation: num(a, "rotation", -180.0, 180.0)?,
            key_width: num(a, "key-width", 0.0, 1.0)?,
            key_phase: num(a, "key-phase", 0.0, 1.0)?,
            key_soft: num(a, "key-soft", 0.001, 0.5)?,
            key_repeat: num(a, "key-repeat", 1.0, 16.0)?,
        })
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    // ── Pipeline (pure) ─────────────────────────────────────────────────────

    /// `base` with this edit applied.
    pub fn apply(&self, base: &SvgOffsetSettings) -> SvgOffsetSettings {
        SvgOffsetSettings {
            quality: self.quality.unwrap_or(base.quality),
            background: self.background.unwrap_or(base.background),
            offset: [
                self.offset_x.unwrap_or(base.offset[0]),
                self.offset_y.unwrap_or(base.offset[1]),
            ],
            rotation: self.rotation.unwrap_or(base.rotation),
            key_width: self.key_width.unwrap_or(base.key_width),
            key_phase: self.key_phase.unwrap_or(base.key_phase),
            key_soft: self.key_soft.unwrap_or(base.key_soft),
            key_repeat: self.key_repeat.unwrap_or(base.key_repeat),
        }
    }
}

/// The name of stored value `v` in `table`, else the first (the sim reads an
/// unknown quality as full and an unknown background as alpha).
fn name_of(table: &[(u32, &'static str)], v: u32) -> &'static str {
    table
        .iter()
        .find(|(value, _)| *value == v)
        .map_or(table[0].1, |(_, n)| n)
}

/// `{layer path builtin shapes quality background offset rotation key-*}`.
pub fn state_json(layer: usize, info: &SvgOffsetInfo) -> Value {
    let s = &info.settings;
    json!({
        "layer": layer,
        "path": info.path,
        "builtin": info.path.is_none(),
        "shapes": info.shapes,
        "quality": name_of(QUALITIES, s.quality),
        "background": name_of(BACKGROUNDS, s.background),
        "offset": s.offset,
        "rotation": s.rotation,
        "key-width": s.key_width,
        "key-phase": s.key_phase,
        "key-soft": s.key_soft,
        "key-repeat": s.key_repeat,
    })
}

// ── Boundary ────────────────────────────────────────────────────────────────

/// The layer named by `a` and its SVG Offset, or why there is none.
fn target<P: SvgPort>(port: &P, a: &Value) -> Result<(usize, SvgOffsetInfo), String> {
    let layer = layer(port, a)?;
    let info = port
        .svg_state(layer)
        .ok_or_else(|| actions::no_svg(layer))?;
    Ok((layer, info))
}

fn read_back<P: SvgPort>(port: &P, layer: usize) -> OpResult {
    let (_, info) = target(port, &json!({ "layer": layer }))?;
    Ok(state_json(layer, &info))
}

fn state<P: SvgPort>(port: &mut P, a: &Value) -> OpResult {
    let (layer, info) = target(port, a)?;
    Ok(state_json(layer, &info))
}

fn set<P: SvgPort>(port: &mut P, a: &Value) -> OpResult {
    let edit = SettingsEdit::from_args(a)?;
    let (layer, info) = target(port, a)?;
    if !edit.is_empty() {
        let settings = edit.apply(&info.settings);
        // A new quality re-rasterizes, exactly as the panel's Quality buttons.
        let reload = settings.quality != info.settings.quality;
        port.apply_svg(
            layer,
            SvgOffsetCommand {
                path: info.path,
                settings,
                reload,
            },
        )?;
    }
    read_back(port, layer)
}

fn load<P: SvgPort>(port: &mut P, a: &Value) -> OpResult {
    let path = path(a)?;
    let (layer, info) = target(port, a)?;
    port.apply_svg(
        layer,
        SvgOffsetCommand {
            path,
            settings: info.settings,
            reload: true,
        },
    )?;
    read_back(port, layer)
}

fn reload<P: SvgPort>(port: &mut P, a: &Value) -> OpResult {
    let (layer, info) = target(port, a)?;
    port.apply_svg(
        layer,
        SvgOffsetCommand {
            path: info.path,
            settings: info.settings,
            reload: true,
        },
    )?;
    read_back(port, layer)
}
