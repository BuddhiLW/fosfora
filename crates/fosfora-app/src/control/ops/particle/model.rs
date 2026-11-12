//! Domain values shared by the particle ops, the App actions behind them and
//! the panels' signal handlers in `main.rs`. Plain data: no App, no GPU.

use std::path::PathBuf;

use crate::gpu::particle::types::{ModelSampleDef, ObstacleFit, ObstacleMode};

/// A partial edit of a particle system's emission params; `None` leaves the
/// field as it is.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SimEdit {
    pub emit_rate: Option<f32>,
    pub burst: Option<u32>,
    pub lifetime: Option<f32>,
    pub speed: Option<f32>,
    pub size: Option<f32>,
    pub drag: Option<f32>,
    pub trail_length: Option<u32>,
}

impl SimEdit {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The emission params of one particle system, as the Particles panel shows them.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SimState {
    pub emit_rate: f32,
    pub burst: u32,
    pub lifetime: f32,
    pub speed: f32,
    pub size: f32,
    pub drag: f32,
    pub trail_length: u32,
    pub max_particles: u32,
}

impl SimState {
    /// This state with `edit` applied.
    pub fn with(self, edit: &SimEdit) -> Self {
        Self {
            emit_rate: edit.emit_rate.unwrap_or(self.emit_rate),
            burst: edit.burst.unwrap_or(self.burst),
            lifetime: edit.lifetime.unwrap_or(self.lifetime),
            speed: edit.speed.unwrap_or(self.speed),
            size: edit.size.unwrap_or(self.size),
            drag: edit.drag.unwrap_or(self.drag),
            trail_length: edit.trail_length.unwrap_or(self.trail_length),
            max_particles: self.max_particles,
        }
    }
}

/// What kind of file a load reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
    Model,
}

impl MediaKind {
    pub const ALL: &'static [MediaKind] = &[Self::Image, Self::Video, Self::Model];

    pub fn name(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::Model => "model",
        }
    }

    /// The kind [`Self::name`] names, any case.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|k| k.name().eq_ignore_ascii_case(s.trim()))
    }
}

/// A particle-source load: a built-in picture by display name, or a file
/// (`path: None` opens the native file dialog).
#[derive(Debug, Clone, PartialEq)]
pub enum SourceLoad {
    Builtin(String),
    File {
        kind: MediaKind,
        path: Option<PathBuf>,
    },
}

/// A partial edit of a video source's transport.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VideoEdit {
    pub playing: Option<bool>,
    pub looping: Option<bool>,
    pub speed: Option<f32>,
    pub seek: Option<f64>,
}

impl VideoEdit {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A video source's transport.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VideoState {
    pub playing: bool,
    pub looping: bool,
    pub speed: f32,
    pub frame: usize,
    pub frames: usize,
}

/// A particle system's source.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SourceState {
    /// `none`, `image`, `video`, `webcam` or `model`.
    pub kind: &'static str,
    pub path: Option<String>,
    pub video: Option<VideoState>,
    /// The pose a model source is sampled at.
    pub pose: ModelSampleDef,
    /// Names of the source loads still in flight (every layer).
    pub loading: Vec<String>,
}

/// A splat effect's scene state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SplatState {
    pub is_splat: bool,
    pub loading: bool,
    pub downloading: bool,
    pub last_error: Option<String>,
}

/// A batch of morph edits, applied in field order, as one frame of the
/// Morph panel. Indices are as the panel numbers them: `auto_cycle` 0 off,
/// 1 on beat, 2 timed; `style` 0 spring, 1 explode, 2 flow, 3 cascade,
/// 4 direct. New targets go to the given slot, else the next empty one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MorphEdit {
    pub trigger: Option<u32>,
    pub auto_cycle: Option<u32>,
    pub hold: Option<f32>,
    pub style: Option<u32>,
    pub set_source: Option<u32>,
    pub blend: Option<f32>,
    pub clear_slot: Option<u32>,
    pub add_geometry: Option<String>,
    pub add_text: Option<String>,
    pub snapshot: bool,
    /// A file target; `path: None` opens the native file dialog.
    pub load: Option<(MediaKind, Option<PathBuf>)>,
}

impl MorphEdit {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A particle system's morph state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MorphView {
    /// What each defined slot holds (`geometry:star`, `text:hi`, `snapshot`...).
    pub slots: Vec<String>,
    pub target_count: u32,
    pub source: u32,
    pub dest: u32,
    pub progress: f32,
    pub transitioning: bool,
    pub style: u32,
    pub auto_cycle: u32,
    pub hold: f32,
}

/// What an obstacle command still needs from the window: a native file
/// dialog, which belongs to the window loop rather than to `App`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObstacleFollowUp {
    Done,
    ImageDialog,
    ModelDialog,
}

/// A particle system's obstacle, as the Obstacle panel shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ObstacleState {
    pub enabled: bool,
    pub mode: ObstacleMode,
    pub fit: ObstacleFit,
    pub threshold: f32,
    pub elasticity: f32,
    /// `image`, `video`, `webcam`, `depth` or `model`.
    pub source: Option<&'static str>,
    pub image_path: Option<String>,
    pub water: bool,
    pub fluid: bool,
    pub model_spin: f32,
    pub model_display: f32,
}
