//! Boundary: the obstacle and morph ops.

use serde_json::{Value, json};

use super::model::{MorphEdit, MorphView, ObstacleFollowUp};
use super::pipeline::{morph_json, obstacle_file_kind, obstacle_json};
use super::port::{MorphPort, ObstaclePort};
use super::promote::{self, ObstacleOp};
use crate::control::{OpResult, args};

fn obstacle_view<P: ObstaclePort>(port: &P, layer: usize) -> OpResult {
    port.obstacle_state(layer)
        .map(|s| obstacle_json(layer, &s))
        .ok_or_else(|| format!("layer {layer} has no particle system"))
}

/// `particle/obstacle`: the obstacle, or run `:cmd` on it first.
pub fn obstacle<P: ObstaclePort>(port: &mut P, a: &Value) -> OpResult {
    let layer = promote::layer(port, a)?;
    obstacle_view(port, layer)?;
    if a.get("cmd").is_none() {
        return obstacle_view(port, layer);
    }
    match promote::obstacle_op(a)? {
        ObstacleOp::Load(kind, path) => port.load_obstacle(layer, kind, path)?,
        ObstacleOp::Command(cmd) => match port.obstacle_command(layer, cmd)? {
            ObstacleFollowUp::Done => {}
            _ => return Err("give the obstacle file as :path".into()),
        },
    }
    obstacle_view(port, layer)
}

/// `particle/obstacle-load`: an obstacle file; its kind from `:kind` or the
/// file's extension.
pub fn obstacle_load<P: ObstaclePort>(port: &mut P, a: &Value) -> OpResult {
    let layer = promote::layer(port, a)?;
    let path = promote::path(a)?.ok_or("missing :path")?;
    let kind = match a.get("kind") {
        Some(_) => promote::media_kind(a)?,
        None => obstacle_file_kind(&path),
    };
    obstacle_view(port, layer)?;
    port.load_obstacle(layer, kind, Some(path))?;
    obstacle_view(port, layer)
}

/// `particle/obstacle-webcam`: feed camera obstacles from a camera picked
/// by `:index` (its device index) or `:name`.
pub fn obstacle_webcam<P: ObstaclePort + super::port::ParticleSourcePort>(port: &mut P, a: &Value) -> OpResult {
    let devices = port.webcam_devices();
    let (index, name) = match args::opt_usize(a, "index") {
        Some(i) => devices
            .iter()
            .find(|(d, _)| usize::try_from(*d).is_ok_and(|d| d == i))
            .cloned()
            .ok_or_else(|| format!("no camera with index {i}"))?,
        None => {
            let names: Vec<&str> = devices.iter().map(|(_, n)| n.as_str()).collect();
            devices[args::pick(a, &names, "camera")?].clone()
        }
    };
    port.switch_obstacle_webcam(index)?;
    Ok(json!({"index": index, "name": name}))
}

fn morph_view<P: MorphPort>(port: &P, layer: usize) -> Result<MorphView, String> {
    port.morph_state(layer)
        .ok_or_else(|| format!("layer {layer}'s effect has no morph targets"))
}

/// Apply `edit` (new targets into `:slot`, if given), answer the morph state.
fn morph_apply<P: MorphPort>(port: &mut P, a: &Value, edit: MorphEdit) -> OpResult {
    let layer = promote::layer(port, a)?;
    let slot = promote::morph_slot(a, "slot")?;
    morph_view(port, layer)?;
    port.apply_morph(layer, &edit, slot)?;
    Ok(morph_json(layer, &morph_view(port, layer)?))
}

/// `particle/morph`: the morph state.
pub fn morph<P: MorphPort>(port: &mut P, a: &Value) -> OpResult {
    let layer = promote::layer(port, a)?;
    Ok(morph_json(layer, &morph_view(port, layer)?))
}

/// An index arg that must name a loaded target.
fn loaded_target<P: MorphPort>(port: &P, a: &Value, key: &str) -> Result<u32, String> {
    let layer = promote::layer(port, a)?;
    let count = morph_view(port, layer)?.target_count;
    let i = promote::morph_slot(a, key)?.ok_or_else(|| format!("missing :{key}"))?;
    if i < count {
        Ok(i)
    } else {
        Err(format!(":{key} {i} is not a loaded target (have {count})"))
    }
}

/// `particle/morph-trigger`: morph to loaded target `:target`.
pub fn morph_trigger<P: MorphPort>(port: &mut P, a: &Value) -> OpResult {
    let trigger = Some(loaded_target(port, a, "target")?);
    morph_apply(port, a, MorphEdit { trigger, ..MorphEdit::default() })
}

/// `particle/morph-source`: the target morphs start from.
pub fn morph_source<P: MorphPort>(port: &mut P, a: &Value) -> OpResult {
    let set_source = Some(loaded_target(port, a, "target")?);
    morph_apply(port, a, MorphEdit { set_source, ..MorphEdit::default() })
}

/// `particle/morph-cycle`: auto-cycle off, on beat or timed.
pub fn morph_cycle<P: MorphPort>(port: &mut P, a: &Value) -> OpResult {
    let auto_cycle = Some(promote::morph_cycle(a)?);
    morph_apply(port, a, MorphEdit { auto_cycle, ..MorphEdit::default() })
}

/// `particle/morph-style`: the transition style.
pub fn morph_style<P: MorphPort>(port: &mut P, a: &Value) -> OpResult {
    let style = Some(promote::morph_style(a)?);
    morph_apply(port, a, MorphEdit { style, ..MorphEdit::default() })
}

/// `particle/morph-hold`: seconds a shape holds before the next auto morph.
pub fn morph_hold<P: MorphPort>(port: &mut P, a: &Value) -> OpResult {
    let hold = Some(promote::required(a, "secs", |x| (0.0..=60.0).contains(&x), "seconds 0..60")?);
    morph_apply(port, a, MorphEdit { hold, ..MorphEdit::default() })
}

/// `particle/morph-blend`: scrub the current morph to `:progress` (0..1).
pub fn morph_blend<P: MorphPort>(port: &mut P, a: &Value) -> OpResult {
    let blend = Some(promote::required(a, "progress", |x| (0.0..=1.0).contains(&x), "a number 0..1")?);
    morph_apply(port, a, MorphEdit { blend, ..MorphEdit::default() })
}

/// `particle/morph-clear`: empty slot `:slot`; higher slots shift down.
pub fn morph_clear<P: MorphPort>(port: &mut P, a: &Value) -> OpResult {
    let layer = promote::layer(port, a)?;
    let clear = promote::morph_slot(a, "slot")?.ok_or("missing :slot (0..3)")?;
    morph_view(port, layer)?;
    port.apply_morph(layer, &MorphEdit { clear_slot: Some(clear), ..MorphEdit::default() }, None)?;
    Ok(morph_json(layer, &morph_view(port, layer)?))
}

/// `particle/morph-geometry`: a generated shape as a target.
pub fn morph_geometry<P: MorphPort>(port: &mut P, a: &Value) -> OpResult {
    let add_geometry = Some(promote::geometry(a)?);
    morph_apply(port, a, MorphEdit { add_geometry, ..MorphEdit::default() })
}

/// `particle/morph-text`: rendered text as a target.
pub fn morph_text<P: MorphPort>(port: &mut P, a: &Value) -> OpResult {
    let text = args::str(a, "text")?;
    if text.trim().is_empty() {
        return Err(":text is empty".into());
    }
    let add_text = Some(text.to_owned());
    morph_apply(port, a, MorphEdit { add_text, ..MorphEdit::default() })
}

/// `particle/morph-snapshot`: the particles as they are now, as a target.
pub fn morph_snapshot<P: MorphPort>(port: &mut P, a: &Value) -> OpResult {
    morph_apply(port, a, MorphEdit { snapshot: true, ..MorphEdit::default() })
}

/// `particle/morph-load`: an image, video or model file as target(s).
pub fn morph_load<P: MorphPort>(port: &mut P, a: &Value) -> OpResult {
    let load = Some((promote::media_kind(a)?, promote::path(a)?));
    morph_apply(port, a, MorphEdit { load, ..MorphEdit::default() })
}
