//! The App actions behind the particle, lattice and helix panels (the morph
//! and obstacle ones live beside this file). `main.rs`'s signal handlers and
//! the `particle/*` ops both call these, so a click and an op run the same
//! code.
//!
//! Every action names its layer; the panels pass the active one.

use std::path::{Path, PathBuf};

use super::model::{MediaKind, SimEdit, SourceLoad, VideoEdit};
use crate::app::App;
use crate::effect::loader::EffectLoader;
use crate::gpu::helix::HelixParams;
use crate::gpu::lattice::LatticeParams;
use crate::gpu::layer::LayerStack;
use crate::gpu::particle::types::{ModelSampleDef, ParticleDef};
use crate::gpu::particle::{ParticleSource, ParticleSystem, SourceRequest};
use crate::settings::ParticleQuality;

/// The particle system of `layer`, or why there is none.
pub(crate) fn particles(stack: &LayerStack, layer: usize) -> Result<&ParticleSystem, String> {
    stack
        .layers
        .get(layer)
        .ok_or_else(|| format!("there is no layer {layer}"))?
        .as_effect()
        .and_then(|e| e.pass_executor.particle_system.as_ref())
        .ok_or_else(|| format!("layer {layer} has no particle system"))
}

/// [`particles`], mutably.
pub(crate) fn particles_mut(
    stack: &mut LayerStack,
    layer: usize,
) -> Result<&mut ParticleSystem, String> {
    stack
        .layers
        .get_mut(layer)
        .ok_or_else(|| format!("there is no layer {layer}"))?
        .as_effect_mut()
        .and_then(|e| e.pass_executor.particle_system.as_mut())
        .ok_or_else(|| format!("layer {layer} has no particle system"))
}

/// The message for an action this build was compiled without.
pub(crate) fn not_built(feature: &str) -> String {
    format!("this build has no {feature} support")
}

impl App {
    /// Apply an emission edit to `layer`'s particle system and, for a user
    /// effect, write the new definition back to its file. Marks the preset
    /// dirty. Fails when the layer has no particle system or the save fails.
    pub fn apply_particle_sim(&mut self, layer: usize, edit: &SimEdit) -> Result<(), String> {
        if edit.is_empty() {
            return Ok(());
        }
        self.preset_store.mark_dirty();
        let device = self.gpu.device.clone();
        let hdr = crate::gpu::GpuContext::hdr_format();
        let effect = self
            .layer_stack
            .layers
            .get_mut(layer)
            .and_then(|l| l.as_effect_mut())
            .ok_or_else(|| format!("layer {layer} is not an effect"))?;
        let effect_index = effect.effect_index;
        let ps = effect
            .pass_executor
            .particle_system
            .as_mut()
            .ok_or_else(|| format!("layer {layer} has no particle system"))?;
        if let Some(v) = edit.emit_rate {
            ps.emit_rate = v;
            ps.def.emit_rate = v;
        }
        if let Some(v) = edit.burst {
            ps.burst_on_beat = v;
            ps.def.burst_on_beat = v;
        }
        if let Some(v) = edit.lifetime {
            ps.def.lifetime = v;
        }
        if let Some(v) = edit.speed {
            ps.def.initial_speed = v;
        }
        if let Some(v) = edit.size {
            ps.def.initial_size = v;
        }
        if let Some(v) = edit.drag {
            ps.def.drag = v;
        }
        if let Some(v) = edit.trail_length {
            ps.def.trail_length = v;
            ps.set_trail_length(&device, hdr, v);
        }
        let def = ps.def.clone();
        match effect_index {
            Some(idx) => self.save_particle_def(idx, def),
            None => Ok(()),
        }
    }

    /// Persist `def` into user effect `idx`'s file. Built-in effects are
    /// runtime-only and are left alone.
    fn save_particle_def(&mut self, idx: usize, def: ParticleDef) -> Result<(), String> {
        let Some(effect) = self.effect_loader.effects.get_mut(idx) else {
            return Ok(());
        };
        if EffectLoader::is_builtin(effect) {
            return Ok(());
        }
        effect.particles = Some(def);
        if let Some(path) = effect.source_path.clone() {
            if let Ok(json) = serde_json::to_string_pretty(&*effect) {
                std::fs::write(path, json).map_err(|e| format!("Effect save failed: {e}"))?;
            }
        }
        Ok(())
    }

    /// Change the particle quality (saved to Settings) and reload the active
    /// layer's effect so its buffers are rebuilt at the new size.
    pub fn set_particle_quality(&mut self, quality: ParticleQuality) {
        self.settings.particle_quality = quality;
        self.settings.save();
        let active = self.layer_stack.active_layer;
        if let Some(effect_idx) = self
            .layer_stack
            .layers
            .get(active)
            .and_then(|l| l.effect_index())
        {
            self.load_effect_on_layer(active, effect_idx);
        }
    }

    /// Replace `layer`'s lattice params, rebuilding the sim, and reseed it
    /// when asked. Marks the preset dirty.
    pub fn apply_lattice(
        &mut self,
        layer: usize,
        params: LatticeParams,
        reseed: bool,
    ) -> Result<(), String> {
        self.preset_store.mark_dirty();
        let device = self.gpu.device.clone();
        let ps = particles_mut(&mut self.layer_stack, layer)?;
        ps.lattice_params = params;
        ps.init_lattice(&device, crate::gpu::GpuContext::hdr_format());
        if reseed {
            ps.request_lattice_seed();
        }
        Ok(())
    }

    /// Replace `layer`'s helix params, rebuilding the sim. Marks the preset dirty.
    pub fn apply_helix(&mut self, layer: usize, params: HelixParams) -> Result<(), String> {
        self.preset_store.mark_dirty();
        let device = self.gpu.device.clone();
        let ps = particles_mut(&mut self.layer_stack, layer)?;
        ps.helix_params = params;
        ps.init_helix(&device, crate::gpu::GpuContext::hdr_format());
        Ok(())
    }

    /// Start a source load for `request`: the file at `path`, or the native
    /// dialog for `kind` when `path` is `None`.
    pub(super) fn start_source_load(
        &mut self,
        kind: MediaKind,
        path: Option<PathBuf>,
        request: SourceRequest,
    ) -> Result<(), String> {
        if let Some(p) = &path {
            if !p.is_file() {
                return Err(format!("no such file: {}", p.display()));
            }
        }
        let loader = &mut self.particle_source_loader;
        let started = match (kind, path) {
            (MediaKind::Image, Some(p)) => loader.load_image(p, request),
            (MediaKind::Image, None) => loader.open_image_dialog(request),
            (MediaKind::Video, Some(p)) => loader.load_video(p, request),
            #[cfg(feature = "video")]
            (MediaKind::Video, None) => loader.open_video_dialog(request),
            #[cfg(not(feature = "video"))]
            (MediaKind::Video, None) => return Err(not_built("video")),
            (MediaKind::Model, Some(p)) => loader.load_model(p, request),
            (MediaKind::Model, None) => loader.open_model_dialog(request),
        };
        started.map_err(|e| format!("could not start the source loader: {e}"))
    }

    /// Load a new particle source onto `layer`. Refused while another source
    /// load is in flight. Marks the preset dirty.
    pub fn load_particle_source(&mut self, layer: usize, load: SourceLoad) -> Result<(), String> {
        particles(&self.layer_stack, layer)?;
        if self.particle_source_loader.is_loading() {
            return Err("a particle source is still loading".into());
        }
        let request = SourceRequest {
            layer_idx: layer,
            ..Default::default()
        };
        match load {
            SourceLoad::Builtin(name) => {
                let path = crate::gpu::particle::builtin_raster_path(&name);
                self.start_source_load(MediaKind::Image, Some(path), request)?;
            }
            SourceLoad::File { kind, path } => self.start_source_load(kind, path, request)?,
        }
        self.preset_store.mark_dirty();
        Ok(())
    }

    /// Re-sample `layer`'s model source at `pose`; the particle count it
    /// sampled. Fails when the source is not a model. Marks the preset dirty.
    pub fn set_particle_model_pose(
        &mut self,
        layer: usize,
        pose: &ModelSampleDef,
    ) -> Result<usize, String> {
        self.preset_store.mark_dirty();
        let (device, queue) = (&self.gpu.device, &self.gpu.queue);
        let ps = particles_mut(&mut self.layer_stack, layer)?;
        let ParticleSource::Model { path } = &ps.source else {
            return Err(format!("layer {layer}'s particle source is not a model"));
        };
        let path = path.clone();
        ps.apply_model_source(device, queue, Path::new(&path), pose)
    }

    /// Make camera `device` (the default one for `None`) `layer`'s particle
    /// source. Marks the preset dirty.
    pub fn use_particle_webcam(&mut self, layer: usize, device: Option<String>) -> Result<(), String> {
        particles(&self.layer_stack, layer)?;
        #[cfg(feature = "webcam")]
        {
            let started = self.set_particle_webcam(layer, device);
            self.cleanup_webcam_if_unused();
            self.preset_store.mark_dirty();
            started
        }
        #[cfg(not(feature = "webcam"))]
        {
            let _ = device;
            Err(not_built("webcam"))
        }
    }

    /// Change `layer`'s video-source transport. Fails when the source is not
    /// a video. A loop or speed change marks the preset dirty.
    pub fn edit_particle_video(&mut self, layer: usize, edit: &VideoEdit) -> Result<(), String> {
        let ps = particles_mut(&mut self.layer_stack, layer)?;
        let Some(playback) = ps.source.playback_mut() else {
            return Err(format!("layer {layer}'s particle source is not a video"));
        };
        if let Some(v) = edit.playing {
            playback.playing = v;
        }
        if let Some(v) = edit.looping {
            playback.looping = v;
        }
        if let Some(v) = edit.speed {
            playback.speed = v;
        }
        if let Some(v) = edit.seek {
            ps.source.seek_to_secs(v);
        }
        if edit.looping.is_some() || edit.speed.is_some() {
            self.preset_store.mark_dirty();
        }
        Ok(())
    }

    /// Load a splat scene onto `layer` (a splat effect): the file at `path`,
    /// or the native dialog for `None`. Marks the preset dirty.
    pub fn splat_load_scene(&mut self, layer: usize, path: Option<PathBuf>) -> Result<(), String> {
        if self.splat_loader.loading {
            return Err("a splat scene is still loading".into());
        }
        let ps = particles(&self.layer_stack, layer)?;
        let splat = ps
            .def
            .splat
            .as_ref()
            .ok_or_else(|| format!("layer {layer}'s effect is not a splat effect"))?;
        let target = ps.max_particles;
        let opts = crate::gpu::particle::splat_source::SceneOptions::from(splat);
        match path {
            Some(p) => self.splat_loader.load(p, target, opts, layer),
            None => self.splat_loader.open_dialog(target, opts, layer),
        }
        self.preset_store.mark_dirty();
        Ok(())
    }

    /// Start downloading the demo splat scene; it loads onto the active splat
    /// layer when done. Refused while a download runs.
    pub fn splat_download_demo(&mut self) -> Result<(), String> {
        if self.splat_demo_download.is_some() {
            return Err("the demo scene is already downloading".into());
        }
        self.splat_demo_download =
            Some(crate::gpu::particle::splat_source::download_demo_scene("default"));
        Ok(())
    }
}
