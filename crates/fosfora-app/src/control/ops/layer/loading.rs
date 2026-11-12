//! Boundary: media files still decoding into new layers, and the shader a
//! layer runs.

use serde_json::{Value, json};

use super::port::{EffectPort, MediaLoadView, MediaPort, StackPort};
use super::{layer_target, pick_in};
use crate::control::OpResult;

/// One pending media load as the ops show it.
pub fn load_json(index: usize, l: &MediaLoadView) -> Value {
    json!({
        "index": index,
        "file": l.file,
        "done": l.done,
        "total": l.total,
        "secs": l.secs,
    })
}

fn loads_json<P: MediaPort>(port: &P) -> Value {
    Value::Array(
        port.media_loads()
            .iter()
            .enumerate()
            .map(|(i, l)| load_json(i, l))
            .collect(),
    )
}

/// `layer/loading`: the media files still decoding.
#[allow(clippy::unnecessary_wraps)] // an OpFn
pub fn loading<P: MediaPort>(port: &mut P, _: &Value) -> OpResult {
    Ok(loads_json(port))
}

/// `layer/cancel-media`: stop one pending load, picked by its index or by
/// (a unique part of) its file name; the load that was stopped.
pub fn cancel_media<P: MediaPort>(port: &mut P, a: &Value) -> OpResult {
    let loads = port.media_loads();
    if loads.is_empty() {
        return Err("no media is loading".into());
    }
    let files: Vec<&str> = loads.iter().map(|l| l.file.as_str()).collect();
    let i = match a.get("load") {
        Some(v) => pick_in(v, &files, "media load")?,
        None if loads.len() == 1 => 0,
        None => {
            return Err(format!(
                "{} files are loading; give :load (an index or a file name)",
                loads.len()
            ));
        }
    };
    let gone = load_json(i, &loads[i]);
    port.cancel_media(i)?;
    Ok(gone)
}

/// `layer/shader-source`: the WGSL a layer's effect runs, one entry per pass.
pub fn shader_source<P: StackPort + EffectPort>(port: &mut P, a: &Value) -> OpResult {
    let layer = layer_target(port, a)?;
    let sources = port
        .shader_sources(layer)
        .ok_or_else(|| format!("layer {layer} is not an effect layer"))?;
    let effect = port.layers().get(layer).and_then(|l| l.effect.clone());
    Ok(json!({
        "layer": layer,
        "effect": effect,
        "source": sources.first(),
        "passes": sources,
    }))
}
