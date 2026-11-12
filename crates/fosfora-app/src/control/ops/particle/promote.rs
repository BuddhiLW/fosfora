//! Promote: op args (a JSON map, REPL keywords as string keys) into the
//! particle domain values. A key that is present with the wrong type or out
//! of range is an error, never silently dropped.

use std::path::PathBuf;

use serde_json::Value;

use super::model::{MediaKind, SimEdit, VideoEdit};
use super::pipeline::{GEOMETRY_SHAPES, MORPH_CYCLES, MORPH_STYLES, expand_home, index_or_name};
use super::port::ParticleLayers;
use crate::control::args;
use crate::gpu::particle::types::{ModelSampleDef, ObstacleFit, ObstacleMode};
use crate::settings::ParticleQuality;
use crate::ui::panels::obstacle_panel::ObstacleCommand;

/// The layer `{:layer n}` names, else the active one.
pub fn layer<P: ParticleLayers>(port: &P, a: &Value) -> Result<usize, String> {
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

/// A number arg, if present; `check` is its valid range, `what` names it.
fn num(
    a: &Value,
    key: &str,
    check: impl Fn(f64) -> bool,
    what: &str,
) -> Result<Option<f32>, String> {
    match a.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_f64() {
            #[allow(clippy::cast_possible_truncation)]
            Some(x) if x.is_finite() && check(x) => Ok(Some(x as f32)),
            _ => Err(format!(":{key} must be {what}, got {v}")),
        },
    }
}

/// A whole-number arg in `0..=max`, if present.
fn whole(a: &Value, key: &str, max: u32) -> Result<Option<u32>, String> {
    match a.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|&n| n <= max)
            .map(Some)
            .ok_or_else(|| format!(":{key} must be a whole number 0..={max}, got {v}")),
    }
}

/// A boolean arg, if present.
fn flag(a: &Value, key: &str) -> Result<Option<bool>, String> {
    match a.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_bool()
            .map(Some)
            .ok_or_else(|| format!(":{key} must be true or false")),
    }
}

/// `{:emit-rate :burst :lifetime :speed :size :drag :trail-length}`, any subset.
pub fn sim_edit(a: &Value) -> Result<SimEdit, String> {
    Ok(SimEdit {
        emit_rate: num(a, "emit-rate", |x| x >= 0.0, "a number >= 0")?,
        burst: whole(a, "burst", 1_000_000)?,
        lifetime: num(a, "lifetime", |x| x > 0.0, "a number > 0")?,
        speed: num(a, "speed", |_| true, "a number")?,
        size: num(a, "size", |x| x > 0.0, "a number > 0")?,
        drag: num(a, "drag", |x| (0.0..=1.0).contains(&x), "a number 0..1")?,
        trail_length: whole(a, "trail-length", 32)?,
    })
}

pub fn quality_name(q: ParticleQuality) -> &'static str {
    match q {
        ParticleQuality::Low => "low",
        ParticleQuality::Medium => "medium",
        ParticleQuality::High => "high",
        ParticleQuality::Ultra => "ultra",
        ParticleQuality::Max => "max",
    }
}

/// A quality by [`quality_name`], any case.
pub fn quality(raw: &str) -> Result<ParticleQuality, String> {
    ParticleQuality::ALL
        .iter()
        .copied()
        .find(|q| quality_name(*q).eq_ignore_ascii_case(raw.trim()))
        .ok_or_else(|| format!("quality must be low, medium, high, ultra or max, got {raw:?}"))
}

/// `{:playing :looping :speed :seek}`, any subset.
pub fn video_edit(a: &Value) -> Result<VideoEdit, String> {
    Ok(VideoEdit {
        playing: flag(a, "playing")?,
        looping: flag(a, "looping")?,
        speed: num(a, "speed", |x| x > 0.0 && x <= 16.0, "a number in (0, 16]")?,
        seek: num(a, "seek", |x| x >= 0.0, "seconds >= 0")?.map(f64::from),
    })
}

/// `base` with any of `{:yaw :pitch :scale :ambient :light-mix :light-x
/// :light-y :light-z :rays}` replaced.
pub fn pose(base: &ModelSampleDef, a: &Value) -> Result<ModelSampleDef, String> {
    let any = |_: f64| true;
    let get = |key: &str, cur: f32| num(a, key, any, "a number").map(|v| v.unwrap_or(cur));
    Ok(ModelSampleDef {
        yaw_degrees: get("yaw", base.yaw_degrees)?,
        pitch_degrees: get("pitch", base.pitch_degrees)?,
        scale: num(a, "scale", |x| x > 0.0, "a number > 0")?.unwrap_or(base.scale),
        ambient: get("ambient", base.ambient)?,
        light_mix: num(
            a,
            "light-mix",
            |x| (0.0..=1.0).contains(&x),
            "a number 0..1",
        )?
        .unwrap_or(base.light_mix),
        light_x: get("light-x", base.light_x)?,
        light_y: get("light-y", base.light_y)?,
        light_z: get("light-z", base.light_z)?,
        ray_strength: get("rays", base.ray_strength)?,
    })
}

/// `{:path "..."}` as a path (a leading `~` is the home folder), if given.
pub fn path(a: &Value) -> Result<Option<PathBuf>, String> {
    match a.get("path") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if !s.trim().is_empty() => {
            let home = std::env::var_os("HOME").map(PathBuf::from);
            Ok(Some(expand_home(s.trim(), home.as_deref())))
        }
        Some(v) => Err(format!(":path must be a file path, got {v}")),
    }
}

/// `{:kind "image" | "video" | "model"}`.
pub fn media_kind(a: &Value) -> Result<MediaKind, String> {
    let raw = args::str(a, "kind")?;
    MediaKind::parse(raw).ok_or_else(|| format!(":kind must be image, video or model, got {raw:?}"))
}

const MODES: &[(&str, ObstacleMode)] = &[
    ("bounce", ObstacleMode::Bounce),
    ("stick", ObstacleMode::Stick),
    ("flow", ObstacleMode::Flow),
    ("contain", ObstacleMode::Contain),
    ("drape", ObstacleMode::Drape),
];

const FITS: &[(&str, ObstacleFit)] = &[
    ("stretch", ObstacleFit::Stretch),
    ("fit", ObstacleFit::Contain),
    ("fill", ObstacleFit::Cover),
];

fn named<T: Copy>(table: &[(&str, T)], v: &Value, what: &str) -> Result<T, String> {
    let s = v.as_str().unwrap_or_default();
    table
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(s.trim()))
        .map(|(_, t)| *t)
        .ok_or_else(|| {
            let names: Vec<&str> = table.iter().map(|(n, _)| *n).collect();
            format!("{what} must be one of {names:?}, got {v}")
        })
}

/// What `particle/obstacle {:cmd ...}` asks for.
pub enum ObstacleOp {
    Command(ObstacleCommand),
    Load(MediaKind, Option<PathBuf>),
}

/// An obstacle command by name (`mode`, `set-mode`, `clear`...) with its
/// `:value`; `load-image`/`load-video`/`load-model` take `:path`.
pub fn obstacle_op(a: &Value) -> Result<ObstacleOp, String> {
    use ObstacleCommand as C;
    let raw = args::str(a, "cmd")?;
    let cmd = raw.trim().to_ascii_lowercase().replace('_', "-");
    let cmd = cmd.strip_prefix("set-").unwrap_or(&cmd);
    let v = a.get("value").unwrap_or(&Value::Null);
    let f = |what: &str| -> Result<f32, String> {
        args::opt_f32(a, "value")
            .filter(|x| x.is_finite())
            .ok_or_else(|| format!("{what} takes a number :value, got {v}"))
    };
    let b = |what: &str| {
        v.as_bool()
            .ok_or_else(|| format!("{what} takes a boolean :value, got {v}"))
    };
    Ok(ObstacleOp::Command(match cmd {
        "enabled" | "enable" => C::SetEnabled(b(cmd)?),
        "mode" => C::SetMode(named(MODES, v, "mode")?),
        "fit" => C::SetFit(named(FITS, v, "fit")?),
        "threshold" => C::SetThreshold(f(cmd)?),
        "elasticity" => C::SetElasticity(f(cmd)?),
        "use-webcam" | "webcam" => C::UseWebcam,
        "use-depth" | "depth" => C::UseDepth,
        "download-depth-model" => C::DownloadDepthModel,
        "clear" => C::Clear,
        "water" | "water-enabled" => C::SetWaterEnabled(b(cmd)?),
        "water-level" => C::SetWaterLevel(f(cmd)?),
        "water-source" => C::SetWaterSource(f(cmd)?),
        "water-drain" => C::SetWaterDrain(f(cmd)?),
        "water-flux" => C::SetWaterFlux(f(cmd)?),
        "model-spin" => C::SetModelSpin(f(cmd)?),
        "model-display" => C::SetModelDisplay(f(cmd)?),
        "fluid" | "fluid-enabled" => C::SetFluidEnabled(b(cmd)?),
        "fluid-speed" => C::SetFluidSpeed(f(cmd)?),
        "fluid-coupling" => C::SetFluidCoupling(f(cmd)?),
        "fluid-vorticity" => C::SetFluidVorticity(f(cmd)?),
        "fluid-viscosity" => C::SetFluidViscosity(f(cmd)?),
        "fluid-grid" => C::SetFluidGrid(
            v.as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|&n| n > 0)
                .ok_or_else(|| format!("fluid-grid takes a positive whole :value, got {v}"))?,
        ),
        "load-image" => return Ok(ObstacleOp::Load(MediaKind::Image, path(a)?)),
        "load-video" => return Ok(ObstacleOp::Load(MediaKind::Video, path(a)?)),
        "load-model" => return Ok(ObstacleOp::Load(MediaKind::Model, path(a)?)),
        _ => {
            return Err(format!(
                "unknown obstacle :cmd {raw:?}; one of {:?}",
                OBSTACLE_CMDS
            ));
        }
    }))
}

/// Every name [`obstacle_op`] takes (without the optional `set-` prefix).
pub const OBSTACLE_CMDS: &[&str] = &[
    "enabled",
    "mode",
    "fit",
    "threshold",
    "elasticity",
    "use-webcam",
    "use-depth",
    "download-depth-model",
    "clear",
    "water",
    "water-level",
    "water-source",
    "water-drain",
    "water-flux",
    "model-spin",
    "model-display",
    "fluid",
    "fluid-speed",
    "fluid-coupling",
    "fluid-vorticity",
    "fluid-viscosity",
    "fluid-grid",
    "load-image",
    "load-video",
    "load-model",
];

/// A morph style by name or panel index.
pub fn morph_style(a: &Value) -> Result<u32, String> {
    let v = a.get("style").ok_or("missing :style")?;
    index_or_name(v, MORPH_STYLES)
        .ok_or_else(|| format!(":style must be one of {MORPH_STYLES:?} (or 0..4), got {v}"))
}

/// A morph auto-cycle mode by name or panel index.
pub fn morph_cycle(a: &Value) -> Result<u32, String> {
    let v = a.get("mode").ok_or("missing :mode")?;
    index_or_name(v, MORPH_CYCLES)
        .ok_or_else(|| format!(":mode must be one of {MORPH_CYCLES:?} (or 0..2), got {v}"))
}

/// A morph slot `0..=3` under `key`, if given.
pub fn morph_slot(a: &Value, key: &str) -> Result<Option<u32>, String> {
    whole(a, key, 3)
}

/// A geometry shape name, any case.
pub fn geometry(a: &Value) -> Result<String, String> {
    let raw = args::str(a, "shape")?;
    GEOMETRY_SHAPES
        .iter()
        .find(|s| s.eq_ignore_ascii_case(raw.trim()))
        .map(|s| (*s).to_owned())
        .ok_or_else(|| format!(":shape must be one of {GEOMETRY_SHAPES:?}, got {raw:?}"))
}

/// A number arg that must be present.
pub fn required(
    a: &Value,
    key: &str,
    check: impl Fn(f64) -> bool,
    what: &str,
) -> Result<f32, String> {
    num(a, key, check, what)?.ok_or_else(|| format!("missing :{key} ({what})"))
}

/// A boolean arg, if present (exposed for the lattice `:reseed`).
pub fn opt_flag(a: &Value, key: &str) -> Result<Option<bool>, String> {
    flag(a, key)
}
