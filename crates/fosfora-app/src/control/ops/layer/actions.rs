//! The layer-domain UI actions as `App` methods, shared by the panels'
//! signal handlers in `main.rs` and by the layer ops.

use std::path::PathBuf;

use super::edit_refusal;
use super::port::{LayerEdit, TransportEdit};
use crate::app::App;
use crate::bindings::catalog::MAX_LAYERS;

fn out_of_range(what: &str, index: usize, len: usize) -> String {
    format!("{what} index {index} out of range (have {len})")
}

impl App {
    /// Add an empty layer on top and select it (the "+" button); its index.
    pub fn layer_add(&mut self) -> Result<usize, String> {
        let before = self.layer_stack.layers.len();
        if before >= MAX_LAYERS {
            return Err(format!("the stack is full ({MAX_LAYERS} layers)"));
        }
        self.add_layer();
        if self.layer_stack.layers.len() == before {
            return Err("could not create the layer (default shader failed)".into());
        }
        self.preset_store.mark_dirty();
        Ok(self.layer_stack.layers.len() - 1)
    }

    /// Add an empty layer at position `at` and select it, as a catalog
    /// effect dropped between layers does ([`App::place_effect`]); `at`.
    pub fn layer_insert(&mut self, at: usize) -> Result<usize, String> {
        let n = self.layer_stack.layers.len();
        if at > n {
            return Err(format!(":at {at} out of range (0..={n})"));
        }
        self.layer_add()?;
        // add_layer appends at the bottom and selects it; the move carries
        // the selection and the bindings along.
        self.move_layer(n, at);
        self.layer_stack.active_layer = at;
        self.sync_active_layer();
        Ok(at)
    }

    /// Remove layer `index`, carrying its bindings; the last layer stays.
    pub fn layer_remove(&mut self, index: usize) -> Result<(), String> {
        let len = self.layer_stack.layers.len();
        if index >= len {
            return Err(out_of_range("layer", index, len));
        }
        if len <= 1 {
            return Err("the last layer cannot be removed".into());
        }
        self.remove_layer(index);
        self.preset_store.mark_dirty();
        #[cfg(feature = "webcam")]
        self.cleanup_webcam_if_unused();
        Ok(())
    }

    /// Back to the launch stack (Clear stack).
    pub fn layer_clear(&mut self) {
        self.clear_all_layers();
        #[cfg(feature = "webcam")]
        self.cleanup_webcam_if_unused();
        self.preset_store.mark_dirty();
    }

    /// Move a layer, carrying its bindings.
    pub fn layer_move(&mut self, from: usize, to: usize) {
        self.move_layer(from, to);
        self.preset_store.mark_dirty();
    }

    /// Make layer `index` the active one; false when out of range.
    pub fn layer_select(&mut self, index: usize) -> bool {
        if index >= self.layer_stack.layers.len() {
            return false;
        }
        self.layer_stack.active_layer = index;
        self.sync_active_layer();
        true
    }

    /// Apply `edit` to layer `index`. A locked layer refuses blend,
    /// opacity, displace and its switch unless the edit unlocks it.
    pub fn layer_edit(&mut self, index: usize, edit: &LayerEdit) -> Result<(), String> {
        let len = self.layer_stack.layers.len();
        let layer = self
            .layer_stack
            .layers
            .get_mut(index)
            .ok_or_else(|| out_of_range("layer", index, len))?;
        if let Some(why) = edit_refusal(index, layer.locked, edit) {
            return Err(why);
        }
        if let Some(v) = edit.locked {
            layer.locked = v;
        }
        if let Some(v) = edit.pinned {
            layer.pinned = v;
        }
        if let Some(v) = &edit.name {
            layer.custom_name = v.clone();
        }
        if let Some(v) = edit.blend {
            layer.blend_mode = v;
        }
        if let Some(v) = edit.opacity {
            layer.opacity = v;
        }
        if let Some(v) = edit.displace {
            layer.displace_amount = v;
        }
        if let Some(v) = edit.enabled {
            layer.enabled = v;
        }
        self.preset_store.mark_dirty();
        Ok(())
    }

    /// Run catalog effect `effect` on layer `layer`; a locked layer refuses.
    pub fn layer_load_effect(&mut self, layer: usize, effect: usize) -> Result<(), String> {
        let effects = self.effect_loader.effects.len();
        if effect >= effects {
            return Err(out_of_range("effect", effect, effects));
        }
        let len = self.layer_stack.layers.len();
        let target = self
            .layer_stack
            .layers
            .get(layer)
            .ok_or_else(|| out_of_range("layer", layer, len))?;
        if target.locked {
            return Err(format!("layer {layer} is locked"));
        }
        if layer == self.layer_stack.active_layer {
            self.load_effect(effect);
        } else {
            self.load_effect_on_layer(layer, effect);
        }
        self.preset_store.mark_dirty();
        Ok(())
    }

    /// Star or unstar an effect (toggle with `None`); the new state.
    pub fn effect_set_favorite(&mut self, name: &str, on: Option<bool>) -> bool {
        let favs = &mut self.settings.favorite_effects;
        let pos = favs.iter().position(|f| f == name);
        let want = on.unwrap_or(pos.is_none());
        match (pos, want) {
            (Some(p), false) => {
                favs.remove(p);
            }
            (None, true) => favs.push(name.to_owned()),
            _ => return want,
        }
        self.settings.save();
        want
    }

    /// Copy catalog effect `source` to a new user effect and load it.
    pub fn effect_copy(&mut self, source: usize, name: &str) -> Result<(), String> {
        let effects = self.effect_loader.effects.len();
        if source >= effects {
            return Err(out_of_range("effect", source, effects));
        }
        self.effect_loader.current_effect = Some(source);
        self.copy_builtin_effect(name).map_err(|e| e.to_string())
    }

    /// Delete a user effect's files; the deleted name. It leaves the
    /// favorites, and the editor closes.
    pub fn effect_delete(&mut self, index: usize) -> Result<String, String> {
        let name = self.effect_loader.delete_effect(index).map_err(|e| {
            log::error!("Failed to delete effect: {e}");
            e.to_string()
        })?;
        log::info!("Deleted effect: {name}");
        let favs = &mut self.settings.favorite_effects;
        if let Some(pos) = favs.iter().position(|f| *f == name) {
            favs.remove(pos);
            self.settings.save();
        }
        self.shader_editor.open = false;
        // The active layer's effect_index refers to the old list.
        self.effect_loader.current_effect = None;
        Ok(name)
    }

    /// Open layer `layer`'s first shader pass, with its `.pfx` paired, in
    /// the editor; the shader's path.
    pub fn shader_editor_open(&mut self, layer: usize) -> Result<PathBuf, String> {
        let idx = self
            .layer_stack
            .layers
            .get(layer)
            .and_then(|l| l.effect_index())
            .ok_or_else(|| format!("layer {layer} runs no effect"))?;
        let effect = self
            .effect_loader
            .effects
            .get(idx)
            .cloned()
            .ok_or("the layer's effect is gone")?;
        let pass = effect
            .normalized_passes()
            .into_iter()
            .next()
            .ok_or_else(|| format!("{} has no shader pass", effect.name))?;
        let path = self.effect_loader.resolve_shader_path(&pass.shader);
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Could not read shader {}: {e}", path.display()))?;
        self.shader_editor
            .open_file(&effect.name, path.clone(), content);
        if let Some(ref pfx_path) = effect.source_path {
            if let Ok(pfx) = std::fs::read_to_string(pfx_path) {
                self.shader_editor.load_paired_pfx(pfx_path.clone(), pfx);
            }
        }
        Ok(path)
    }

    /// Write the editor's active tab, and its paired tab when dirty; the
    /// paths written. Both are tried; the first failure is returned.
    pub fn shader_editor_save(&mut self) -> Result<Vec<PathBuf>, String> {
        let mut saved = Vec::new();
        let mut first_err = None;
        let ed = &mut self.shader_editor;
        if let Some(path) = ed.file_path.clone() {
            match std::fs::write(&path, &ed.code) {
                Ok(()) => {
                    ed.disk_content = ed.code.clone();
                    log::info!("Saved shader: {}", path.display());
                    saved.push(path);
                }
                Err(e) => {
                    log::error!("Failed to save shader: {e}");
                    first_err.get_or_insert(e.to_string());
                }
            }
        }
        if ed.paired_is_dirty() {
            if let Some(path) = ed.paired_path.clone() {
                match std::fs::write(&path, &ed.paired_content) {
                    Ok(()) => {
                        ed.paired_disk_content = ed.paired_content.clone();
                        log::info!("Saved paired file: {}", path.display());
                        saved.push(path);
                    }
                    Err(e) => {
                        log::error!("Failed to save paired file: {e}");
                        first_err.get_or_insert(e.to_string());
                    }
                }
            }
        }
        match first_err {
            Some(e) => Err(e),
            None => Ok(saved),
        }
    }

    /// Start decoding `path` into a new media layer.
    pub fn media_add(&mut self, path: PathBuf) -> Result<(), String> {
        if !path.is_file() {
            return Err(format!("no such file: {}", path.display()));
        }
        let pending = self.layer_stack.layers.len() + self.media_loads.len();
        if pending >= MAX_LAYERS {
            return Err(format!("the stack is full ({MAX_LAYERS} layers)"));
        }
        self.start_media_layer(path);
        Ok(())
    }

    /// Apply `edit` to media layer `layer`'s transport.
    pub fn media_transport_edit(
        &mut self,
        layer: usize,
        edit: &TransportEdit,
    ) -> Result<(), String> {
        let len = self.layer_stack.layers.len();
        let m = self
            .layer_stack
            .layers
            .get_mut(layer)
            .ok_or_else(|| out_of_range("layer", layer, len))?
            .as_media_mut()
            .ok_or_else(|| format!("layer {layer} is not a media layer"))?;
        let t = &mut m.transport;
        if edit.toggle {
            t.playing = !t.playing;
        }
        if let Some(v) = edit.playing {
            t.playing = v;
        }
        if let Some(v) = edit.looping {
            t.looping = v;
        }
        if let Some(v) = edit.speed {
            t.speed = v;
        }
        if let Some(v) = edit.direction {
            t.direction = v;
        }
        if let Some(secs) = edit.seek_secs {
            m.seek_to_secs(secs);
        }
        Ok(())
    }

    /// Make layer `layer`'s effect's own post-processing Master's.
    pub fn postfx_adopt(&mut self, layer: usize) {
        self.adopt_layer_postprocess(layer);
        self.preset_store.mark_dirty();
    }
}

#[cfg(feature = "webcam")]
impl App {
    /// Add a camera layer showing `device` (the next unused camera for
    /// `None`), listing cameras afresh first; the device shown.
    pub fn webcam_add_layer(&mut self, device: Option<&str>) -> Result<String, String> {
        self.refresh_webcam_devices();
        let name = match device {
            Some(d) => d.to_owned(),
            None => self.webcam_for_new_layer().ok_or("No camera found")?,
        };
        let before = self.layer_stack.layers.len();
        self.add_webcam_layer(&name);
        if self.layer_stack.layers.len() == before {
            return Err(self.status_error.as_ref().map_or_else(
                || format!("could not add camera {name:?}"),
                |(m, _)| m.clone(),
            ));
        }
        self.preset_store.mark_dirty();
        Ok(name)
    }

    /// Point live layer `layer` at camera `device`. The previous camera
    /// keeps running until the new one is; the pick becomes the default
    /// unless a particle source or obstacle uses the default.
    pub fn webcam_switch(&mut self, layer: usize, device: u32) -> Result<String, String> {
        let is_live = self
            .layer_stack
            .layers
            .get(layer)
            .and_then(|l| l.as_media())
            .is_some_and(|m| m.is_live());
        if !is_live {
            return Err(format!("layer {layer} is not a camera layer"));
        }
        let started = self
            .webcam_device_name(device)
            .ok_or_else(|| "Camera is no longer connected".to_string())
            .and_then(|name| self.ensure_webcam(&name).map(|_| name));
        let name = match started {
            Ok(name) => name,
            Err(e) => {
                log::error!("Failed to switch webcam device: {e}");
                self.refresh_webcam_devices();
                return Err(e);
            }
        };
        if let Some(m) = self
            .layer_stack
            .layers
            .get_mut(layer)
            .and_then(|l| l.as_media_mut())
        {
            m.set_live_device(&name);
        }
        if !self.default_webcam_in_use() {
            self.set_default_webcam(device);
        }
        self.cleanup_webcam_if_unused();
        self.preset_store.mark_dirty();
        Ok(name)
    }

    /// Mirror (or unmirror) camera layer `layer`.
    pub fn webcam_mirror(&mut self, layer: usize, mirror: bool) -> Result<(), String> {
        let m = self
            .layer_stack
            .layers
            .get_mut(layer)
            .and_then(|l| l.as_media_mut())
            .ok_or_else(|| format!("layer {layer} is not a media layer"))?;
        m.set_mirror(&self.gpu.queue, mirror);
        Ok(())
    }

    /// Switch the capture backend (FFmpeg or native), restarting every
    /// capture; how many devices the new backend lists.
    pub fn webcam_set_ffmpeg(&mut self, on: bool) -> Result<usize, String> {
        if on && !crate::media::webcam_ffmpeg::ffmpeg_available() {
            return Err("FFmpeg not found in PATH. Install FFmpeg to use this feature.".into());
        }
        self.webcam_captures.clear();
        self.use_ffmpeg_webcam = on;
        self.settings.use_ffmpeg_webcam = on;
        self.settings.save();
        self.refresh_webcam_devices();
        self.start_rtmp_streams();
        let n = self.webcam_devices.len();
        log::info!(
            "Webcam backend switched to {}, {n} device(s) found",
            if on { "FFmpeg" } else { "native" }
        );
        Ok(n)
    }
}
