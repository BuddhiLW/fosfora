//! Pure functions of the particle ops: JSON patching of the look params,
//! file-kind guessing, and the JSON views every op answers with.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use super::model::{MediaKind, MorphView, ObstacleState, SimState, SourceState, SplatState};

/// The shapes `particle/morph-geometry` knows.
pub const GEOMETRY_SHAPES: &[&str] = &["circle", "ring", "grid", "spiral", "heart", "star"];

/// Morph transition styles, by their panel index.
pub const MORPH_STYLES: &[&str] = &["spring", "explode", "flow", "cascade", "direct"];

/// Morph auto-cycle modes, by their panel index.
pub const MORPH_CYCLES: &[&str] = &["off", "beat", "timed"];

/// `patch` merged into `base`: objects merge key by key (recursively), any
/// other patch value replaces. A patch key `base` does not have is an error
/// naming its path, so a typo is refused rather than ignored.
pub fn merge_patch(base: &Value, patch: &Value) -> Result<Value, String> {
    merge_at(base, patch, "")
}

fn merge_at(base: &Value, patch: &Value, at: &str) -> Result<Value, String> {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            let mut out: Map<String, Value> = b.clone();
            for (k, pv) in p {
                let path = if at.is_empty() {
                    k.clone()
                } else {
                    format!("{at}.{k}")
                };
                let bv = b.get(k).ok_or_else(|| {
                    format!(
                        "unknown param {path:?}; known: {:?}",
                        b.keys().collect::<Vec<_>>()
                    )
                })?;
                out.insert(k.clone(), merge_at(bv, pv, &path)?);
            }
            Ok(Value::Object(out))
        }
        (Value::Object(_), _) if !at.is_empty() => Err(format!("param {at:?} takes a map")),
        _ => Ok(patch.clone()),
    }
}

/// `base` with the JSON `patch` (a map) applied, read back as `T`.
pub fn patched<T: Serialize + DeserializeOwned>(base: &T, patch: &Value) -> Result<T, String> {
    if !patch.is_object() {
        return Err(":params must be a map".into());
    }
    let base = serde_json::to_value(base).map_err(|e| e.to_string())?;
    let merged = merge_patch(&base, patch)?;
    serde_json::from_value(merged).map_err(|e| format!("bad params: {e}"))
}

/// `value` as JSON (`null` if it cannot be).
pub fn to_json<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// What an obstacle file at `path` is, by extension: a 3D model, a video,
/// else an image.
pub fn obstacle_file_kind(path: &Path) -> MediaKind {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "glb" | "gltf" | "ply" | "splat" => MediaKind::Model,
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" | "gif" => MediaKind::Video,
        _ => MediaKind::Image,
    }
}

/// `raw` as a path, with a leading `~` meaning `home`.
pub fn expand_home(raw: &str, home: Option<&Path>) -> PathBuf {
    match (raw.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
            home.join(rest.trim_start_matches('/'))
        }
        _ => PathBuf::from(raw),
    }
}

/// The index of `name` (any case) in `names`, or of a number below `len`.
pub fn index_or_name(v: &Value, names: &[&str]) -> Option<u32> {
    if let Some(n) = v.as_u64() {
        return (usize::try_from(n).ok()? < names.len()).then(|| u32::try_from(n).ok())?;
    }
    let s = v.as_str()?.trim();
    names
        .iter()
        .position(|n| n.eq_ignore_ascii_case(s))
        .and_then(|i| u32::try_from(i).ok())
}

pub fn sim_json(layer: usize, s: &SimState) -> Value {
    json!({
        "layer": layer, "emit-rate": s.emit_rate, "burst": s.burst, "lifetime": s.lifetime,
        "speed": s.speed, "size": s.size, "drag": s.drag, "trail-length": s.trail_length,
        "max-particles": s.max_particles,
    })
}

pub fn source_json(layer: usize, s: &SourceState) -> Value {
    json!({
        "layer": layer, "kind": s.kind, "path": s.path,
        "video": s.video.map(|v| json!({
            "playing": v.playing, "looping": v.looping, "speed": v.speed,
            "frame": v.frame, "frames": v.frames,
        })),
        "pose": to_json(&s.pose), "loading": s.loading,
    })
}

pub fn splat_json(layer: usize, s: &SplatState) -> Value {
    json!({
        "layer": layer, "splat": s.is_splat, "loading": s.loading,
        "downloading": s.downloading, "error": s.last_error,
    })
}

pub fn obstacle_json(layer: usize, s: &ObstacleState) -> Value {
    json!({
        "layer": layer, "enabled": s.enabled, "mode": to_json(&s.mode), "fit": to_json(&s.fit),
        "threshold": s.threshold, "elasticity": s.elasticity, "source": s.source,
        "image-path": s.image_path, "water": s.water, "fluid": s.fluid,
        "model-spin": s.model_spin, "model-display": s.model_display,
    })
}

pub fn morph_json(layer: usize, m: &MorphView) -> Value {
    fn name(names: &[&'static str], i: u32) -> &'static str {
        names.get(i as usize).copied().unwrap_or("?")
    }
    json!({
        "layer": layer, "slots": m.slots, "targets": m.target_count, "source": m.source,
        "dest": m.dest, "progress": m.progress, "transitioning": m.transitioning,
        "style": name(MORPH_STYLES, m.style), "cycle": name(MORPH_CYCLES, m.auto_cycle),
        "hold": m.hold,
    })
}
