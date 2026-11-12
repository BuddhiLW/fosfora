//! Boundary: the particle-source ops (built-ins, files, webcam, video
//! transport, model pose, splat scenes).

use serde_json::{Value, json};

use super::model::{MediaKind, SourceLoad, SourceState};
use super::pipeline::{source_json, splat_json, to_json};
use super::port::ParticleSourcePort;
use super::promote;
use crate::control::{OpResult, args};

fn source<P: ParticleSourcePort>(port: &P, layer: usize) -> Result<SourceState, String> {
    port.source_state(layer)
        .ok_or_else(|| format!("layer {layer} has no particle system"))
}

/// `particle/source`: the layer's source.
pub fn state<P: ParticleSourcePort>(port: &mut P, a: &Value) -> OpResult {
    let layer = promote::layer(port, a)?;
    Ok(source_json(layer, &source(port, layer)?))
}

/// `particle/builtins`: the built-in pictures.
#[allow(clippy::unnecessary_wraps)] // an OpFn
pub fn builtins<P: ParticleSourcePort>(port: &mut P, _: &Value) -> OpResult {
    Ok(Value::Array(
        port.builtin_images()
            .iter()
            .enumerate()
            .map(|(i, n)| json!({"index": i, "name": n}))
            .collect(),
    ))
}

/// `particle/load-builtin`: load a built-in picture by index or name.
pub fn load_builtin<P: ParticleSourcePort>(port: &mut P, a: &Value) -> OpResult {
    let layer = promote::layer(port, a)?;
    let names = port.builtin_images();
    let name = names[args::pick(a, &names, "built-in image")?].clone();
    port.load_source(layer, SourceLoad::Builtin(name.clone()))?;
    Ok(json!({"layer": layer, "loading": name}))
}

fn load_file<P: ParticleSourcePort>(port: &mut P, a: &Value, kind: MediaKind) -> OpResult {
    let layer = promote::layer(port, a)?;
    let path = promote::path(a)?;
    let shown = path
        .as_ref()
        .map_or_else(|| "file dialog".to_owned(), |p| p.display().to_string());
    port.load_source(layer, SourceLoad::File { kind, path })?;
    Ok(json!({"layer": layer, "kind": kind.name(), "loading": shown}))
}

/// `particle/load-image`: an image file (`:path`), or the file dialog.
pub fn load_image<P: ParticleSourcePort>(port: &mut P, a: &Value) -> OpResult {
    load_file(port, a, MediaKind::Image)
}

/// `particle/load-video`: a video file (`:path`), or the file dialog.
pub fn load_video<P: ParticleSourcePort>(port: &mut P, a: &Value) -> OpResult {
    load_file(port, a, MediaKind::Video)
}

/// `particle/load-model`: a 3D model file (`:path`), or the file dialog.
pub fn load_model<P: ParticleSourcePort>(port: &mut P, a: &Value) -> OpResult {
    load_file(port, a, MediaKind::Model)
}

/// `particle/model-pose`: read, or change any part of, a model source's pose.
pub fn model_pose<P: ParticleSourcePort>(port: &mut P, a: &Value) -> OpResult {
    let layer = promote::layer(port, a)?;
    let current = source(port, layer)?;
    let pose = promote::pose(&current.pose, a)?;
    if pose != current.pose {
        if current.kind != "model" {
            return Err(format!("layer {layer}'s particle source is not a model"));
        }
        port.set_model_pose(layer, &pose)?;
    }
    Ok(json!({"layer": layer, "pose": to_json(&source(port, layer)?.pose)}))
}

/// `particle/webcams`: the connected cameras.
#[allow(clippy::unnecessary_wraps)] // an OpFn
pub fn webcams<P: ParticleSourcePort>(port: &mut P, _: &Value) -> OpResult {
    Ok(Value::Array(
        port.webcam_devices()
            .iter()
            .map(|(i, n)| json!({"index": i, "name": n}))
            .collect(),
    ))
}

/// `particle/webcam`: a camera (`:device` name, default camera without) as
/// the source.
pub fn webcam<P: ParticleSourcePort>(port: &mut P, a: &Value) -> OpResult {
    let layer = promote::layer(port, a)?;
    let device = match args::opt_str(a, "device") {
        Some(name) => {
            let names: Vec<String> = port.webcam_devices().into_iter().map(|(_, n)| n).collect();
            let i = args::pick(&json!({"name": name}), &names, "camera")?;
            Some(names[i].clone())
        }
        None => None,
    };
    port.use_webcam(layer, device)?;
    Ok(source_json(layer, &source(port, layer)?))
}

/// `particle/video`: read, or change, a video source's transport.
pub fn video<P: ParticleSourcePort>(port: &mut P, a: &Value) -> OpResult {
    let layer = promote::layer(port, a)?;
    let edit = promote::video_edit(a)?;
    if source(port, layer)?.video.is_none() {
        return Err(format!("layer {layer}'s particle source is not a video"));
    }
    if !edit.is_empty() {
        port.edit_video(layer, &edit)?;
    }
    Ok(source_json(layer, &source(port, layer)?)["video"].clone())
}

/// `particle/splat`: the layer's splat scene state.
pub fn splat<P: ParticleSourcePort>(port: &mut P, a: &Value) -> OpResult {
    let layer = promote::layer(port, a)?;
    let s = port
        .splat_state(layer)
        .ok_or_else(|| format!("layer {layer} has no particle system"))?;
    Ok(splat_json(layer, &s))
}

/// `particle/splat-load`: a splat scene file (`:path`), or the file dialog.
pub fn splat_load<P: ParticleSourcePort>(port: &mut P, a: &Value) -> OpResult {
    let layer = promote::layer(port, a)?;
    let path = promote::path(a)?;
    let shown = path
        .as_ref()
        .map_or_else(|| "file dialog".to_owned(), |p| p.display().to_string());
    port.splat_load(layer, path)?;
    Ok(json!({"layer": layer, "loading": shown}))
}

fn load_json(l: &super::port::SourceLoading) -> Value {
    json!({"index": l.index, "name": l.name, "done": l.done, "total": l.total, "secs": l.secs})
}

/// `particle/loading`: every particle source, obstacle or morph file still
/// loading.
#[allow(clippy::unnecessary_wraps)] // an OpFn
pub fn loading<P: ParticleSourcePort>(port: &mut P, _: &Value) -> OpResult {
    Ok(Value::Array(
        port.source_loads().iter().map(load_json).collect(),
    ))
}

/// `particle/cancel-load`: stop one load, picked by its index or by (a unique
/// part of) its file name; the only one without `:load`. The load stopped.
pub fn cancel_load<P: ParticleSourcePort>(port: &mut P, a: &Value) -> OpResult {
    let loads = port.source_loads();
    let names: Vec<&str> = loads.iter().map(|l| l.name.as_str()).collect();
    let i = match a.get("load") {
        _ if loads.is_empty() => return Err("no particle source is loading".into()),
        None | Some(Value::Null) if loads.len() == 1 => 0,
        None | Some(Value::Null) => {
            return Err(format!(
                "{} loads are running; give :load (an index or a file name)",
                loads.len()
            ));
        }
        Some(v @ Value::Number(_)) => args::pick(&json!({"index": v}), &names, "source load")?,
        Some(v @ Value::String(_)) => args::pick(&json!({"name": v}), &names, "source load")?,
        Some(v) => return Err(format!(":load must be an index or a file name, got {v}")),
    };
    let gone = load_json(&loads[i]);
    port.cancel_source_load(i)?;
    Ok(gone)
}

/// `particle/splat-demo`: download the demo scene onto the active splat layer.
pub fn splat_demo<P: ParticleSourcePort>(port: &mut P, _: &Value) -> OpResult {
    port.splat_download_demo()?;
    Ok(json!({"downloading": "demo:default"}))
}
