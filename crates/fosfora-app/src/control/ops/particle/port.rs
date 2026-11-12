//! What the particle ops need from the app, split by panel (ISP): the
//! emission/look port, the source port, and the obstacle and morph ports.
//! The `App` adapters below are the only code here that reads `App`; what
//! they do lives in the `*_actions.rs` files beside this one.

use std::path::PathBuf;

use super::actions::particles;
use super::model::{
    MediaKind, MorphEdit, MorphView, ObstacleFollowUp, ObstacleState, SimEdit, SimState,
    SourceLoad, SourceState, SplatState, VideoEdit, VideoState,
};
use crate::app::App;
use crate::gpu::helix::HelixParams;
use crate::gpu::lattice::LatticeParams;
pub use crate::gpu::particle::source_loader::SourceLoading;
use crate::gpu::particle::types::ModelSampleDef;
use crate::gpu::volumetric::VolumetricParams;
use crate::settings::ParticleQuality;
use crate::ui::panels::obstacle_panel::ObstacleCommand;

/// Which layers exist; every particle op addresses one.
pub trait ParticleLayers {
    fn active_layer(&self) -> usize;
    fn layer_count(&self) -> usize;
}

/// Emission params, particle quality, the volumetric look, lattice and helix.
/// Getters answer `None` when the layer has no particle system (or no
/// lattice/helix sim).
pub trait ParticleSimPort: ParticleLayers {
    fn sim_state(&self, layer: usize) -> Option<SimState>;
    fn apply_sim(&mut self, layer: usize, edit: &SimEdit) -> Result<(), String>;
    fn particle_quality(&self) -> ParticleQuality;
    fn set_particle_quality(&mut self, quality: ParticleQuality);
    fn volumetric(&self) -> (bool, VolumetricParams);
    fn set_volumetric(&mut self, enabled: bool, params: VolumetricParams);
    fn lattice(&self, layer: usize) -> Option<LatticeParams>;
    fn set_lattice(&mut self, layer: usize, params: LatticeParams, reseed: bool) -> Result<(), String>;
    fn helix(&self, layer: usize) -> Option<HelixParams>;
    fn set_helix(&mut self, layer: usize, params: HelixParams) -> Result<(), String>;
}

/// The particle source: built-ins, files, webcam, video transport, model
/// pose and splat scenes.
pub trait ParticleSourcePort: ParticleLayers {
    fn source_state(&self, layer: usize) -> Option<SourceState>;
    /// Display names of the built-in pictures.
    fn builtin_images(&self) -> Vec<String>;
    fn load_source(&mut self, layer: usize, load: SourceLoad) -> Result<(), String>;
    /// Connected cameras as `(index, name)`.
    fn webcam_devices(&self) -> Vec<(u32, String)>;
    fn use_webcam(&mut self, layer: usize, device: Option<String>) -> Result<(), String>;
    fn edit_video(&mut self, layer: usize, edit: &VideoEdit) -> Result<(), String>;
    fn set_model_pose(&mut self, layer: usize, pose: &ModelSampleDef) -> Result<(), String>;
    fn splat_state(&self, layer: usize) -> Option<SplatState>;
    fn splat_load(&mut self, layer: usize, path: Option<PathBuf>) -> Result<(), String>;
    fn splat_download_demo(&mut self) -> Result<(), String>;
    /// Every source load in flight (any layer), in list order.
    fn source_loads(&self) -> Vec<SourceLoading>;
    /// Stop load `index` of [`Self::source_loads`] (the panel's Cancel).
    fn cancel_source_load(&mut self, index: usize) -> Result<(), String>;
}

/// The obstacle a particle system collides with.
pub trait ObstaclePort: ParticleLayers {
    fn obstacle_state(&self, layer: usize) -> Option<ObstacleState>;
    fn obstacle_command(&mut self, layer: usize, cmd: ObstacleCommand) -> Result<ObstacleFollowUp, String>;
    /// Load an obstacle file; only a video may come from the dialog (`None`).
    fn load_obstacle(&mut self, layer: usize, kind: MediaKind, path: Option<PathBuf>) -> Result<(), String>;
    /// Feed camera obstacles from listed camera `index`.
    fn switch_obstacle_webcam(&mut self, index: u32) -> Result<(), String>;
}

/// Morph targets.
pub trait MorphPort: ParticleLayers {
    fn morph_state(&self, layer: usize) -> Option<MorphView>;
    /// Apply `edit`, putting new targets in `slot` (else the next empty one).
    fn apply_morph(&mut self, layer: usize, edit: &MorphEdit, slot: Option<u32>) -> Result<(), String>;
}

// ── App adapters ────────────────────────────────────────────────────────────

impl ParticleLayers for App {
    fn active_layer(&self) -> usize {
        self.layer_stack.active_layer
    }

    fn layer_count(&self) -> usize {
        self.layer_stack.layers.len()
    }
}

impl ParticleSimPort for App {
    fn sim_state(&self, layer: usize) -> Option<SimState> {
        let ps = particles(&self.layer_stack, layer).ok()?;
        Some(SimState {
            emit_rate: ps.emit_rate,
            burst: ps.burst_on_beat,
            lifetime: ps.def.lifetime,
            speed: ps.def.initial_speed,
            size: ps.def.initial_size,
            drag: ps.def.drag,
            trail_length: ps.def.trail_length,
            max_particles: ps.max_particles,
        })
    }

    fn apply_sim(&mut self, layer: usize, edit: &SimEdit) -> Result<(), String> {
        self.apply_particle_sim(layer, edit)
    }

    fn particle_quality(&self) -> ParticleQuality {
        self.settings.particle_quality
    }

    fn set_particle_quality(&mut self, quality: ParticleQuality) {
        App::set_particle_quality(self, quality);
    }

    fn volumetric(&self) -> (bool, VolumetricParams) {
        (self.volumetric_enabled, self.volumetric_params)
    }

    fn set_volumetric(&mut self, enabled: bool, params: VolumetricParams) {
        if (enabled, params) != (self.volumetric_enabled, self.volumetric_params) {
            self.volumetric_enabled = enabled;
            self.volumetric_params = params;
            self.preset_store.mark_dirty();
        }
    }

    fn lattice(&self, layer: usize) -> Option<LatticeParams> {
        let ps = particles(&self.layer_stack, layer).ok()?;
        ps.lattice_enabled.then_some(ps.lattice_params)
    }

    fn set_lattice(&mut self, layer: usize, params: LatticeParams, reseed: bool) -> Result<(), String> {
        self.apply_lattice(layer, params, reseed)
    }

    fn helix(&self, layer: usize) -> Option<HelixParams> {
        let ps = particles(&self.layer_stack, layer).ok()?;
        ps.helix_enabled.then_some(ps.helix_params)
    }

    fn set_helix(&mut self, layer: usize, params: HelixParams) -> Result<(), String> {
        self.apply_helix(layer, params)
    }
}

impl ParticleSourcePort for App {
    fn source_state(&self, layer: usize) -> Option<SourceState> {
        use crate::gpu::particle::source::ParticleSourceKind as K;
        let ps = particles(&self.layer_stack, layer).ok()?;
        let kind = match ps.source.kind() {
            K::None => "none",
            K::Image => "image",
            K::Video => "video",
            K::Webcam => "webcam",
            K::Model => "model",
        };
        let video = ps.source.playback().map(|p| VideoState {
            playing: p.playing,
            looping: p.looping,
            speed: p.speed,
            frame: p.current_frame,
            frames: ps.source.frame_count(),
        });
        Some(SourceState {
            kind,
            path: ps.source.path().map(str::to_owned),
            video,
            pose: ps.model_sample.clone(),
            loading: self
                .particle_source_loader
                .loads()
                .into_iter()
                .map(|l| l.name)
                .collect(),
        })
    }

    fn builtin_images(&self) -> Vec<String> {
        crate::gpu::particle::source_loader::builtin_raster_images().to_vec()
    }

    fn load_source(&mut self, layer: usize, load: SourceLoad) -> Result<(), String> {
        self.load_particle_source(layer, load)
    }

    fn webcam_devices(&self) -> Vec<(u32, String)> {
        #[cfg(feature = "webcam")]
        {
            self.webcam_devices.clone()
        }
        #[cfg(not(feature = "webcam"))]
        {
            Vec::new()
        }
    }

    fn use_webcam(&mut self, layer: usize, device: Option<String>) -> Result<(), String> {
        self.use_particle_webcam(layer, device)
    }

    fn edit_video(&mut self, layer: usize, edit: &VideoEdit) -> Result<(), String> {
        self.edit_particle_video(layer, edit)
    }

    fn set_model_pose(&mut self, layer: usize, pose: &ModelSampleDef) -> Result<(), String> {
        self.set_particle_model_pose(layer, pose).map(|_| ())
    }

    fn splat_state(&self, layer: usize) -> Option<SplatState> {
        let ps = particles(&self.layer_stack, layer).ok()?;
        Some(SplatState {
            is_splat: ps.def.splat.is_some(),
            loading: self.splat_loader.loading,
            downloading: self.splat_demo_download.is_some(),
            last_error: self.splat_loader.last_error.clone(),
        })
    }

    fn splat_load(&mut self, layer: usize, path: Option<PathBuf>) -> Result<(), String> {
        if let Some(p) = &path {
            if !p.is_file() {
                return Err(format!("no such file: {}", p.display()));
            }
        }
        self.splat_load_scene(layer, path)
    }

    fn splat_download_demo(&mut self) -> Result<(), String> {
        App::splat_download_demo(self)
    }

    fn source_loads(&self) -> Vec<SourceLoading> {
        self.particle_source_loader.loads()
    }

    fn cancel_source_load(&mut self, index: usize) -> Result<(), String> {
        let len = self.particle_source_loader.loads().len();
        if index >= len {
            return Err(format!("source load {index} out of range (have {len})"));
        }
        self.particle_source_loader.cancel(index);
        Ok(())
    }
}

impl ObstaclePort for App {
    fn obstacle_state(&self, layer: usize) -> Option<ObstacleState> {
        use crate::gpu::particle::ObstacleSource as S;
        let ps = particles(&self.layer_stack, layer).ok()?;
        Some(ObstacleState {
            enabled: ps.obstacle_enabled,
            mode: ps.obstacle_mode,
            fit: ps.obstacle_fit,
            threshold: ps.obstacle_threshold,
            elasticity: ps.obstacle_elasticity,
            source: ps.obstacle_source.map(|s| match s {
                S::Image => "image",
                S::Video => "video",
                S::Webcam => "webcam",
                S::Depth => "depth",
                S::Model => "model",
            }),
            image_path: ps.obstacle_image_path.clone(),
            water: ps.obstacle_water_enabled,
            fluid: ps.obstacle_fluid_enabled,
            model_spin: ps.obstacle_model_spin,
            model_display: ps.obstacle_model_display,
        })
    }

    fn obstacle_command(&mut self, layer: usize, cmd: ObstacleCommand) -> Result<ObstacleFollowUp, String> {
        self.apply_obstacle_command(layer, cmd)
    }

    fn load_obstacle(&mut self, layer: usize, kind: MediaKind, path: Option<PathBuf>) -> Result<(), String> {
        match (kind, path) {
            (MediaKind::Video, path) => self.load_obstacle_video(layer, path),
            (_, Some(p)) if p.is_file() => self.load_obstacle_file(layer, &p),
            (_, Some(p)) => Err(format!("no such file: {}", p.display())),
            (_, None) => Err("give the obstacle file as :path".into()),
        }
    }

    fn switch_obstacle_webcam(&mut self, index: u32) -> Result<(), String> {
        App::switch_obstacle_webcam(self, index)
    }
}

impl MorphPort for App {
    fn morph_state(&self, layer: usize) -> Option<MorphView> {
        use crate::gpu::particle::morph::AutoCycle;
        let ps = particles(&self.layer_stack, layer).ok()?;
        let m = ps.morph_state.as_ref()?;
        Some(MorphView {
            slots: ps
                .def
                .morph_targets
                .iter()
                .flatten()
                .map(|t| t.source.clone())
                .collect(),
            target_count: m.target_count,
            source: m.source_index,
            dest: m.dest_index,
            progress: m.progress,
            transitioning: m.transitioning,
            style: m.transition_style,
            auto_cycle: match m.auto_cycle {
                AutoCycle::Off => 0,
                AutoCycle::OnBeat => 1,
                AutoCycle::Timed(_) => 2,
            },
            hold: m.hold_duration,
        })
    }

    fn apply_morph(&mut self, layer: usize, edit: &MorphEdit, slot: Option<u32>) -> Result<(), String> {
        App::apply_morph(self, layer, edit, slot).map(|_| ())
    }
}
