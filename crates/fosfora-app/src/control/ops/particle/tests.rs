//! Unit + property tests for the particle ops, against an in-memory port.
//! The integration leg (a live app over nREPL) is `tests/repl_ops.rs`.

mod property;
mod unit;

use std::path::PathBuf;

use serde_json::Value;

use super::model::{
    MediaKind, MorphEdit, MorphView, ObstacleFollowUp, ObstacleState, SimEdit, SimState,
    SourceLoad, SourceState, SplatState, VideoEdit, VideoState,
};
use super::port::{
    MorphPort, ObstaclePort, ParticleLayers, ParticleSimPort, ParticleSourcePort, SourceLoading,
};
use super::Addon;
use crate::control::{ControlAddon, OpHost, Registry, run};
use crate::gpu::helix::HelixParams;
use crate::gpu::lattice::LatticeParams;
use crate::gpu::particle::types::{ModelSampleDef, ObstacleFit, ObstacleMode};
use crate::gpu::volumetric::VolumetricParams;
use crate::settings::ParticleQuality;
use crate::ui::panels::obstacle_panel::ObstacleCommand;

/// One stub layer; `sim: None` is a layer without a particle system.
#[derive(Default)]
struct Layer {
    sim: Option<SimState>,
    source: SourceState,
    obstacle: Option<ObstacleState>,
    morph: Option<MorphView>,
    lattice: Option<LatticeParams>,
    helix: Option<HelixParams>,
    splat: bool,
}

fn obstacle() -> ObstacleState {
    ObstacleState {
        enabled: false,
        mode: ObstacleMode::Bounce,
        fit: ObstacleFit::Cover,
        threshold: 0.5,
        elasticity: 0.5,
        source: None,
        image_path: None,
        water: false,
        fluid: false,
        model_spin: 1.0,
        model_display: 0.0,
    }
}

/// A particle layer with every feature, so any op can land on it.
fn full_layer() -> Layer {
    Layer {
        sim: Some(SimState {
            emit_rate: 100.0,
            burst: 0,
            lifetime: 2.0,
            speed: 0.5,
            size: 0.01,
            drag: 0.9,
            trail_length: 0,
            max_particles: 1000,
        }),
        source: SourceState { kind: "none", ..SourceState::default() },
        obstacle: Some(obstacle()),
        morph: Some(MorphView { progress: 1.0, hold: 1.5, auto_cycle: 1, ..MorphView::default() }),
        lattice: Some(LatticeParams::default()),
        helix: Some(HelixParams::default()),
        splat: true,
    }
}

struct Stub {
    layers: Vec<Layer>,
    active: usize,
    quality: ParticleQuality,
    vol: (bool, VolumetricParams),
    builtins: Vec<String>,
    cams: Vec<(u32, String)>,
    loading: bool,
    downloading: bool,
    /// Source loads in flight.
    loads: Vec<SourceLoading>,
    /// What the stub was asked to do, in order.
    did: Vec<String>,
    registry: Registry<Stub>,
}

impl Stub {
    /// Layer 0 has no particle system; layer 1 (active) has everything.
    fn new() -> Self {
        let mut s = Self {
            layers: vec![Layer::default(), full_layer()],
            active: 1,
            quality: ParticleQuality::High,
            vol: (false, VolumetricParams::default()),
            builtins: vec!["fosfora".into(), "skull".into(), "phoenix".into()],
            cams: vec![(0, "Integrated Camera".into()), (2, "USB Cam".into())],
            loading: false,
            downloading: false,
            loads: Vec::new(),
            did: Vec::new(),
            registry: Registry::default(),
        };
        s.registry.register(&Addon as &dyn ControlAddon<Stub>);
        s
    }

    fn call(&mut self, op: &str, args: Value) -> Result<Value, String> {
        run(self, op, &args)
    }

    fn layer(&mut self, i: usize) -> Result<&mut Layer, String> {
        self.layers
            .get_mut(i)
            .filter(|l| l.sim.is_some())
            .ok_or_else(|| format!("layer {i} has no particle system"))
    }
}

impl OpHost for Stub {
    fn registry(&self) -> &Registry<Self> {
        &self.registry
    }
}

impl ParticleLayers for Stub {
    fn active_layer(&self) -> usize {
        self.active
    }
    fn layer_count(&self) -> usize {
        self.layers.len()
    }
}

impl ParticleSimPort for Stub {
    fn sim_state(&self, layer: usize) -> Option<SimState> {
        self.layers.get(layer)?.sim
    }
    fn apply_sim(&mut self, layer: usize, edit: &SimEdit) -> Result<(), String> {
        let l = self.layer(layer)?;
        l.sim = l.sim.map(|s| s.with(edit));
        Ok(())
    }
    fn particle_quality(&self) -> ParticleQuality {
        self.quality
    }
    fn set_particle_quality(&mut self, quality: ParticleQuality) {
        self.quality = quality;
    }
    fn volumetric(&self) -> (bool, VolumetricParams) {
        self.vol
    }
    fn set_volumetric(&mut self, enabled: bool, params: VolumetricParams) {
        self.vol = (enabled, params);
    }
    fn lattice(&self, layer: usize) -> Option<LatticeParams> {
        self.layers.get(layer)?.lattice
    }
    fn set_lattice(&mut self, layer: usize, params: LatticeParams, reseed: bool) -> Result<(), String> {
        self.layer(layer)?.lattice = Some(params);
        self.did.push(format!("lattice reseed={reseed}"));
        Ok(())
    }
    fn helix(&self, layer: usize) -> Option<HelixParams> {
        self.layers.get(layer)?.helix
    }
    fn set_helix(&mut self, layer: usize, params: HelixParams) -> Result<(), String> {
        self.layer(layer)?.helix = Some(params);
        Ok(())
    }
}

impl ParticleSourcePort for Stub {
    fn source_state(&self, layer: usize) -> Option<SourceState> {
        let l = self.layers.get(layer)?;
        l.sim.map(|_| l.source.clone())
    }
    fn builtin_images(&self) -> Vec<String> {
        self.builtins.clone()
    }
    fn load_source(&mut self, layer: usize, load: SourceLoad) -> Result<(), String> {
        if self.loading {
            return Err("a particle source is still loading".into());
        }
        let l = self.layer(layer)?;
        let what = match load {
            SourceLoad::Builtin(name) => {
                l.source = SourceState { kind: "image", path: Some(name.clone()), ..SourceState::default() };
                format!("builtin {name}")
            }
            SourceLoad::File { kind, path: Some(p) } => {
                l.source = SourceState {
                    kind: kind.name(),
                    path: Some(p.display().to_string()),
                    video: (kind == MediaKind::Video)
                        .then_some(VideoState { playing: true, looping: true, speed: 1.0, frame: 0, frames: 10 }),
                    ..SourceState::default()
                };
                format!("{} {}", kind.name(), p.display())
            }
            SourceLoad::File { kind, path: None } => format!("{} dialog", kind.name()),
        };
        self.did.push(what);
        Ok(())
    }
    fn webcam_devices(&self) -> Vec<(u32, String)> {
        self.cams.clone()
    }
    fn use_webcam(&mut self, layer: usize, device: Option<String>) -> Result<(), String> {
        if self.cams.is_empty() {
            return Err("no camera".into());
        }
        self.layer(layer)?.source = SourceState { kind: "webcam", ..SourceState::default() };
        self.did.push(format!("webcam {device:?}"));
        Ok(())
    }
    fn edit_video(&mut self, layer: usize, edit: &VideoEdit) -> Result<(), String> {
        let v = self.layer(layer)?.source.video.as_mut().ok_or("not a video")?;
        v.playing = edit.playing.unwrap_or(v.playing);
        v.looping = edit.looping.unwrap_or(v.looping);
        v.speed = edit.speed.unwrap_or(v.speed);
        Ok(())
    }
    fn set_model_pose(&mut self, layer: usize, pose: &ModelSampleDef) -> Result<(), String> {
        let l = self.layer(layer)?;
        if l.source.kind != "model" {
            return Err("not a model".into());
        }
        l.source.pose = pose.clone();
        Ok(())
    }
    fn splat_state(&self, layer: usize) -> Option<SplatState> {
        let l = self.layers.get(layer)?;
        l.sim.map(|_| SplatState { is_splat: l.splat, downloading: self.downloading, ..SplatState::default() })
    }
    fn splat_load(&mut self, layer: usize, path: Option<PathBuf>) -> Result<(), String> {
        if !self.layer(layer)?.splat {
            return Err("not a splat effect".into());
        }
        self.did.push(format!("splat {path:?}"));
        Ok(())
    }
    fn splat_download_demo(&mut self) -> Result<(), String> {
        if std::mem::replace(&mut self.downloading, true) {
            return Err("already downloading".into());
        }
        Ok(())
    }
    fn source_loads(&self) -> Vec<SourceLoading> {
        self.loads.clone()
    }
    fn cancel_source_load(&mut self, index: usize) -> Result<(), String> {
        if index >= self.loads.len() {
            return Err("out of range".into());
        }
        self.loads.remove(index);
        for (i, l) in self.loads.iter_mut().enumerate() {
            l.index = i;
        }
        Ok(())
    }
}

impl ObstaclePort for Stub {
    fn obstacle_state(&self, layer: usize) -> Option<ObstacleState> {
        self.layers.get(layer)?.obstacle.clone()
    }
    fn obstacle_command(&mut self, layer: usize, cmd: ObstacleCommand) -> Result<ObstacleFollowUp, String> {
        let o = self.layer(layer)?.obstacle.as_mut().ok_or("no obstacle")?;
        match cmd {
            ObstacleCommand::SetEnabled(v) => o.enabled = v,
            ObstacleCommand::SetMode(v) => o.mode = v,
            ObstacleCommand::SetFit(v) => o.fit = v,
            ObstacleCommand::SetThreshold(v) => o.threshold = v,
            ObstacleCommand::SetElasticity(v) => o.elasticity = v,
            ObstacleCommand::SetWaterEnabled(v) => o.water = v,
            ObstacleCommand::SetFluidEnabled(v) => o.fluid = v,
            ObstacleCommand::SetModelSpin(v) => o.model_spin = v,
            ObstacleCommand::SetModelDisplay(v) => o.model_display = v,
            ObstacleCommand::Clear => {
                o.enabled = false;
                o.source = None;
                o.image_path = None;
            }
            ObstacleCommand::UseWebcam => {
                o.enabled = true;
                o.source = Some("webcam");
            }
            ObstacleCommand::LoadImage => return Ok(ObstacleFollowUp::ImageDialog),
            ObstacleCommand::LoadModel => return Ok(ObstacleFollowUp::ModelDialog),
            _ => {}
        }
        Ok(ObstacleFollowUp::Done)
    }
    fn load_obstacle(&mut self, layer: usize, kind: MediaKind, path: Option<PathBuf>) -> Result<(), String> {
        let o = self.layer(layer)?.obstacle.as_mut().ok_or("no obstacle")?;
        match (kind, path) {
            (MediaKind::Video, None) => self.did.push("obstacle video dialog".into()),
            (_, None) => return Err("give the obstacle file as :path".into()),
            (k, Some(p)) => {
                o.enabled = true;
                o.source = Some(k.name());
                o.image_path = Some(p.display().to_string());
            }
        }
        Ok(())
    }
    fn switch_obstacle_webcam(&mut self, index: u32) -> Result<(), String> {
        self.did.push(format!("obstacle cam {index}"));
        Ok(())
    }
}

impl MorphPort for Stub {
    fn morph_state(&self, layer: usize) -> Option<MorphView> {
        self.layers.get(layer)?.morph.clone()
    }
    fn apply_morph(&mut self, layer: usize, edit: &MorphEdit, slot: Option<u32>) -> Result<(), String> {
        let m = self.layer(layer)?.morph.as_mut().ok_or("no morph targets")?;
        if let Some(t) = edit.trigger.filter(|&t| t < m.target_count) {
            m.source = m.dest;
            m.dest = t;
        }
        m.auto_cycle = edit.auto_cycle.unwrap_or(m.auto_cycle);
        m.style = edit.style.unwrap_or(m.style);
        m.hold = edit.hold.unwrap_or(m.hold);
        m.source = edit.set_source.unwrap_or(m.source);
        m.progress = edit.blend.unwrap_or(m.progress);
        if let Some(c) = edit.clear_slot.map(|c| c as usize).filter(|&c| c < m.slots.len()) {
            m.slots.remove(c);
        }
        let label = edit
            .add_geometry
            .as_ref()
            .map(|g| format!("geometry:{g}"))
            .or_else(|| edit.add_text.as_ref().map(|t| format!("text:{t}")))
            .or_else(|| edit.snapshot.then(|| "snapshot".to_owned()));
        if let Some(label) = label {
            let at = slot.map_or(m.slots.len(), |s| s as usize).min(3);
            while m.slots.len() <= at {
                m.slots.push(String::new());
            }
            m.slots[at] = label;
        }
        if let Some((kind, path)) = &edit.load {
            self.did.push(format!("morph {} {path:?}", kind.name()));
        }
        let m = self.layer(layer)?.morph.as_mut().ok_or("no morph targets")?;
        m.target_count = u32::try_from(m.slots.len()).unwrap_or(4);
        Ok(())
    }
}
