//! System domain values (Promote) and the pure functions over them
//! (Pipeline). No app access: everything here is unit and property tested.

use serde_json::{Value, json};

use crate::audio::beat::{BPM_MAX, BPM_MIN, TempoConfig, TempoPreset};
use crate::control::args;
use crate::gpu::types::OutputResolution;
use crate::midi::types::TriggerAction;
use crate::recording::types::{Container, RecordingConfig, VideoCodec};
use crate::settings::{AlphaOutputMode, FlashLimit};

// ── Names ────────────────────────────────────────────────────────────────

/// `s` reduced to lower-case ASCII letters and digits: names compare by this,
/// so `drum-and-bass`, `DrumAndBass` and `drum_and_bass` are one name.
pub fn slug(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The item of `all` whose `name` slugs like `raw`; the error lists the
/// accepted names.
pub fn parse_named<T: Copy>(
    raw: &str,
    all: &[T],
    name: fn(T) -> &'static str,
    what: &str,
) -> Result<T, String> {
    let want = slug(raw);
    all.iter()
        .copied()
        .find(|&t| slug(name(t)) == want)
        .ok_or_else(|| {
            let names: Vec<&str> = all.iter().map(|&t| name(t)).collect();
            format!("{what} must be one of {}, got {raw:?}", names.join(", "))
        })
}

/// `a[key]` as one of `all`; absent is `None`.
pub fn opt_named<T: Copy>(
    a: &Value,
    key: &str,
    all: &[T],
    name: fn(T) -> &'static str,
) -> Result<Option<T>, String> {
    args::opt_str(a, key)
        .map(|raw| parse_named(raw, all, name, &format!(":{key}")))
        .transpose()
}

pub fn flash_limit_name(l: FlashLimit) -> &'static str {
    match l {
        FlashLimit::Auto => "auto",
        FlashLimit::Standard => "standard",
        FlashLimit::Strict => "strict",
        FlashLimit::Off => "off",
    }
}

pub fn alpha_mode_name(m: AlphaOutputMode) -> &'static str {
    match m {
        AlphaOutputMode::Auto => "auto",
        AlphaOutputMode::Opaque => "opaque",
        AlphaOutputMode::Luma => "luma",
        AlphaOutputMode::Passthrough => "passthrough",
    }
}

pub fn codec_name(c: VideoCodec) -> &'static str {
    match c {
        VideoCodec::H264 => "h264",
        VideoCodec::Hevc => "hevc",
        VideoCodec::AV1 => "av1",
    }
}

pub fn container_name(c: Container) -> &'static str {
    match c {
        Container::Mp4 => "mp4",
        Container::Mkv => "mkv",
    }
}

pub fn resolution_name(r: OutputResolution) -> &'static str {
    match r {
        OutputResolution::Match => "match",
        OutputResolution::Res720p => "720p",
        OutputResolution::Res1080p => "1080p",
        OutputResolution::Res4K => "4k",
        OutputResolution::Res8K => "8k",
    }
}

pub fn tempo_preset_name(p: TempoPreset) -> &'static str {
    match p {
        TempoPreset::Neutral => "neutral",
        TempoPreset::Wide => "wide",
        TempoPreset::House => "house",
        TempoPreset::DrumAndBass => "drum-and-bass",
        TempoPreset::HipHop => "hip-hop",
        TempoPreset::Ambient => "ambient",
    }
}

/// A trigger's op name, kebab-case (`next-effect`).
pub fn trigger_name(t: TriggerAction) -> &'static str {
    match t {
        TriggerAction::NextEffect => "next-effect",
        TriggerAction::PrevEffect => "prev-effect",
        TriggerAction::TogglePostProcess => "toggle-postprocess",
        TriggerAction::ToggleOverlay => "toggle-overlay",
        TriggerAction::NextPreset => "next-preset",
        TriggerAction::PrevPreset => "prev-preset",
        TriggerAction::NextLayer => "next-layer",
        TriggerAction::PrevLayer => "prev-layer",
        TriggerAction::SceneGoNext => "scene-go-next",
        TriggerAction::SceneGoPrev => "scene-go-prev",
        TriggerAction::ToggleTimeline => "toggle-timeline",
        TriggerAction::TempoHalf => "tempo-half",
        TriggerAction::TempoDouble => "tempo-double",
        TriggerAction::TempoTap => "tempo-tap",
    }
}

// ── Outputs ──────────────────────────────────────────────────────────────

/// A video output sink. Each is built only with its feature on its OS; the
/// op vocabulary is the same in every build, and a missing one says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    Ndi,
    Spout,
    Syphon,
    V4l2,
}

impl OutputKind {
    pub const ALL: &'static [Self] = &[Self::Ndi, Self::Spout, Self::Syphon, Self::V4l2];

    pub fn name(self) -> &'static str {
        match self {
            Self::Ndi => "ndi",
            Self::Spout => "spout",
            Self::Syphon => "syphon",
            Self::V4l2 => "v4l2",
        }
    }

    /// Whether the sink announces itself under a name (v4l2 writes to a device).
    pub fn has_name(self) -> bool {
        !matches!(self, Self::V4l2)
    }
}

/// The v4l2 pixel format, as the op names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Yuyv,
    Bgrx,
}

impl PixelFormat {
    pub const ALL: &'static [Self] = &[Self::Yuyv, Self::Bgrx];

    pub fn name(self) -> &'static str {
        match self {
            Self::Yuyv => "yuyv",
            Self::Bgrx => "bgrx",
        }
    }
}

/// One change to an output's settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputChange {
    /// NDI source / Spout sender / Syphon server name.
    Name(String),
    Resolution(OutputResolution),
    /// NDI only.
    AlphaFromLuma(bool),
    /// v4l2 only; `None` is the first loopback device found.
    Device(Option<String>),
    /// v4l2 only.
    PixelFormat(PixelFormat),
}

/// Whether `kind` has the setting `change` sets.
pub fn accepts(kind: OutputKind, change: &OutputChange) -> bool {
    match change {
        OutputChange::Name(_) => kind.has_name(),
        OutputChange::Resolution(_) => true,
        OutputChange::AlphaFromLuma(_) => kind == OutputKind::Ndi,
        OutputChange::Device(_) | OutputChange::PixelFormat(_) => kind == OutputKind::V4l2,
    }
}

/// The changes `{:name :resolution :alpha-from-luma :device :pixel-format}`
/// ask of `kind`. A setting `kind` does not have is refused; `:device` takes
/// a path, or `"auto"` / nil for the first loopback device.
pub fn output_changes(kind: OutputKind, a: &Value) -> Result<Vec<OutputChange>, String> {
    let mut out = Vec::new();
    if let Some(name) = args::opt_str(a, "name") {
        if name.trim().is_empty() {
            return Err(":name cannot be empty".into());
        }
        out.push(OutputChange::Name(name.to_owned()));
    }
    if let Some(r) = opt_named(a, "resolution", OutputResolution::ALL, resolution_name)? {
        out.push(OutputChange::Resolution(r));
    }
    if let Some(b) = args::opt_bool(a, "alpha-from-luma") {
        out.push(OutputChange::AlphaFromLuma(b));
    }
    match a.get("device") {
        None => {}
        Some(Value::Null) => out.push(OutputChange::Device(None)),
        Some(Value::String(s)) if s.eq_ignore_ascii_case("auto") || s.is_empty() => {
            out.push(OutputChange::Device(None));
        }
        Some(Value::String(s)) => out.push(OutputChange::Device(Some(s.clone()))),
        Some(other) => return Err(format!(":device must be a path or \"auto\", got {other}")),
    }
    if let Some(f) = opt_named(a, "pixel-format", PixelFormat::ALL, PixelFormat::name)? {
        out.push(OutputChange::PixelFormat(f));
    }
    if let Some(bad) = out.iter().find(|c| !accepts(kind, c)) {
        return Err(format!("{} output has no setting for {bad:?}", kind.name()));
    }
    Ok(out)
}

/// What an output reports.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputStatus {
    pub kind: OutputKind,
    /// Built into this binary.
    pub available: bool,
    pub enabled: bool,
    pub running: bool,
    pub error: Option<String>,
    pub name: Option<String>,
    pub resolution: OutputResolution,
    pub alpha_from_luma: Option<bool>,
    /// v4l2: the configured device (`None` = auto).
    pub device: Option<String>,
    pub pixel_format: Option<PixelFormat>,
    /// v4l2: loopback devices found.
    pub devices: Vec<String>,
    pub frames_sent: u64,
}

impl OutputStatus {
    /// An output not built into this binary.
    pub fn unavailable(kind: OutputKind) -> Self {
        Self {
            kind,
            available: false,
            enabled: false,
            running: false,
            error: None,
            name: None,
            resolution: OutputResolution::Match,
            alpha_from_luma: None,
            device: None,
            pixel_format: None,
            devices: Vec::new(),
            frames_sent: 0,
        }
    }

    pub fn to_json(&self) -> Value {
        let mut v = json!({
            "kind": self.kind.name(),
            "available": self.available,
        });
        if !self.available {
            return v;
        }
        v["enabled"] = json!(self.enabled);
        v["running"] = json!(self.running);
        v["error"] = json!(self.error);
        v["resolution"] = json!(resolution_name(self.resolution));
        v["frames-sent"] = json!(self.frames_sent);
        if self.kind.has_name() {
            v["name"] = json!(self.name);
        }
        if let Some(b) = self.alpha_from_luma {
            v["alpha-from-luma"] = json!(b);
        }
        if self.kind == OutputKind::V4l2 {
            v["device"] = json!(self.device.as_deref().unwrap_or("auto"));
            v["pixel-format"] = json!(self.pixel_format.map(PixelFormat::name));
            v["devices"] = json!(self.devices);
        }
        v
    }
}

/// The message for an output this binary was built without.
pub fn not_built(kind: OutputKind) -> String {
    format!("{} output is not built into this binary", kind.name())
}

// ── Tempo ────────────────────────────────────────────────────────────────

/// Prior width bounds in octaves, as the estimator clamps them.
pub const SIGMA_MIN: f32 = 0.05;
pub const SIGMA_MAX: f32 = 4.0;

/// A change to the tempo prior. A preset applies first, then the fields.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TempoChange {
    pub preset: Option<TempoPreset>,
    pub center: Option<f32>,
    pub sigma: Option<f32>,
    pub auto: Option<bool>,
}

impl TempoChange {
    pub fn from_args(a: &Value) -> Result<Self, String> {
        let finite = |key: &str| match args::opt_f32(a, key) {
            Some(v) if !v.is_finite() => Err(format!(":{key} must be a finite number")),
            v => Ok(v),
        };
        Ok(Self {
            preset: opt_named(a, "preset", TempoPreset::ALL, tempo_preset_name)?,
            center: finite("center")?,
            sigma: finite("sigma")?,
            auto: args::opt_bool(a, "auto"),
        })
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// `cfg` with `change` applied, the centre clamped to the reportable BPM
/// range and the width to [`SIGMA_MIN`]..[`SIGMA_MAX`].
#[allow(clippy::cast_possible_truncation)]
pub fn apply_tempo_change(cfg: TempoConfig, change: &TempoChange) -> TempoConfig {
    let mut out = cfg;
    if let Some(p) = change.preset {
        let (c, s) = p.values();
        out.prior_center_bpm = c;
        out.prior_sigma = s;
    }
    if let Some(c) = change.center {
        out.prior_center_bpm = c;
    }
    if let Some(s) = change.sigma {
        out.prior_sigma = s;
    }
    if let Some(a) = change.auto {
        out.auto_prior = a;
    }
    out.prior_center_bpm = out.prior_center_bpm.clamp(BPM_MIN as f32, BPM_MAX as f32);
    out.prior_sigma = out.prior_sigma.clamp(SIGMA_MIN, SIGMA_MAX);
    out
}

/// A tempo to lock onto, checked against the reportable range.
pub fn tempo_bpm(bpm: f64) -> Result<f64, String> {
    if bpm.is_finite() && (BPM_MIN..=BPM_MAX).contains(&bpm) {
        Ok(bpm)
    } else {
        Err(format!(
            ":bpm must be between {BPM_MIN} and {BPM_MAX}, got {bpm}"
        ))
    }
}

/// `{bpm center sigma auto preset}`; `preset` is nil when hand-tuned.
pub fn tempo_json(cfg: &TempoConfig, bpm: f32) -> Value {
    json!({
        "bpm": bpm,
        "center": cfg.prior_center_bpm,
        "sigma": cfg.prior_sigma,
        "auto": cfg.auto_prior,
        "preset": TempoPreset::from_config(cfg).map(tempo_preset_name),
    })
}

// ── Link ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkMode {
    Follow,
    Lead,
}

impl LinkMode {
    pub const ALL: &'static [Self] = &[Self::Follow, Self::Lead];

    pub fn name(self) -> &'static str {
        match self {
            Self::Follow => "follow",
            Self::Lead => "lead",
        }
    }
}

/// A change to the Link session settings.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LinkChange {
    pub enabled: Option<bool>,
    pub mode: Option<LinkMode>,
    pub quantum: Option<f64>,
    pub start_stop: Option<bool>,
}

impl LinkChange {
    pub fn from_args(a: &Value) -> Result<Self, String> {
        let quantum = a.get("quantum").map(Value::as_f64);
        let quantum = match quantum {
            None => None,
            Some(Some(q)) if q.is_finite() && q > 0.0 => Some(q),
            Some(_) => return Err(":quantum must be a positive number of beats".into()),
        };
        Ok(Self {
            enabled: args::opt_bool(a, "enabled"),
            mode: opt_named(a, "mode", LinkMode::ALL, LinkMode::name)?,
            quantum,
            start_stop: args::opt_bool(a, "start-stop"),
        })
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The Link session as the app sees it. The live fields are `None` while
/// Link is off.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinkState {
    pub enabled: bool,
    pub mode: LinkMode,
    pub quantum: f64,
    pub start_stop: bool,
    pub peers: Option<u64>,
    pub tempo: Option<f64>,
    pub phase: Option<f64>,
    pub playing: Option<bool>,
}

impl LinkState {
    pub fn to_json(&self) -> Value {
        json!({
            "enabled": self.enabled,
            "mode": self.mode.name(),
            "quantum": self.quantum,
            "start-stop": self.start_stop,
            "peers": self.peers,
            "tempo": self.tempo,
            "phase": self.phase,
            "playing": self.playing,
        })
    }
}

/// The message for a binary built without Link.
pub const NO_LINK: &str = "Ableton Link is not built into this binary (cargo feature `link`)";

// ── Recording ────────────────────────────────────────────────────────────

pub const FPS_RANGE: std::ops::RangeInclusive<u32> = 1..=240;
pub const QUALITY_RANGE: std::ops::RangeInclusive<u32> = 0..=63;

/// A change to the recording settings; `None` keeps a field.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecordingChange {
    pub codec: Option<VideoCodec>,
    pub resolution: Option<OutputResolution>,
    pub fps: Option<u32>,
    pub quality: Option<u32>,
    pub container: Option<Container>,
    pub hw: Option<bool>,
    pub audio: Option<bool>,
}

impl RecordingChange {
    pub fn from_args(a: &Value) -> Result<Self, String> {
        let ranged =
            |key: &str, range: std::ops::RangeInclusive<u32>| -> Result<Option<u32>, String> {
                match a.get(key) {
                    None => Ok(None),
                    Some(v) => v
                        .as_u64()
                        .and_then(|n| u32::try_from(n).ok())
                        .filter(|n| range.contains(n))
                        .map(Some)
                        .ok_or_else(|| {
                            format!(
                                ":{key} must be an integer in {}..={}",
                                range.start(),
                                range.end()
                            )
                        }),
                }
            };
        Ok(Self {
            codec: opt_named(a, "codec", VideoCodec::ALL, codec_name)?,
            resolution: opt_named(a, "resolution", OutputResolution::ALL, resolution_name)?,
            fps: ranged("fps", FPS_RANGE)?,
            quality: ranged("quality", QUALITY_RANGE)?,
            container: opt_named(a, "container", Container::ALL, container_name)?,
            hw: args::opt_bool(a, "hw"),
            audio: args::opt_bool(a, "audio"),
        })
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Write the given fields into `c`.
    pub fn apply(&self, c: &mut RecordingConfig) {
        if let Some(v) = self.codec {
            c.codec = v;
        }
        if let Some(v) = self.resolution {
            c.resolution = v;
        }
        if let Some(v) = self.fps {
            c.fps = v;
        }
        if let Some(v) = self.quality {
            c.quality = v;
        }
        if let Some(v) = self.container {
            c.container = v;
        }
        if let Some(v) = self.hw {
            c.use_hw_encoder = v;
        }
        if let Some(v) = self.audio {
            c.record_audio = v;
        }
    }
}

pub fn recording_config_json(c: &RecordingConfig) -> Value {
    json!({
        "codec": codec_name(c.codec),
        "resolution": resolution_name(c.resolution),
        "fps": c.fps,
        "quality": c.quality,
        "container": container_name(c.container),
        "hw": c.use_hw_encoder,
        "audio": c.record_audio,
        "dir": c.output_dir.display().to_string(),
    })
}

/// Where a recording stands.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordingStatus {
    pub recording: bool,
    pub path: Option<String>,
    pub seconds: Option<f64>,
    pub frames: u64,
    pub bytes: u64,
    pub audio: bool,
    pub error: Option<String>,
    pub ffmpeg: bool,
}

impl RecordingStatus {
    pub fn to_json(&self) -> Value {
        json!({
            "recording": self.recording,
            "path": self.path,
            "seconds": self.seconds,
            "frames": self.frames,
            "bytes": self.bytes,
            "audio": self.audio,
            "error": self.error,
            "ffmpeg": self.ffmpeg,
        })
    }
}

// ── Settings, web, view ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsView {
    pub flash_limit: FlashLimit,
    pub output_alpha: AlphaOutputMode,
    pub auto_reconnect: bool,
}

impl SettingsView {
    pub fn to_json(&self) -> Value {
        json!({
            "flash-limit": flash_limit_name(self.flash_limit),
            "output-alpha": alpha_mode_name(self.output_alpha),
            "auto-reconnect": self.auto_reconnect,
        })
    }
}

/// A change to the general settings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SettingsChange {
    pub flash_limit: Option<FlashLimit>,
    pub output_alpha: Option<AlphaOutputMode>,
    pub auto_reconnect: Option<bool>,
}

impl SettingsChange {
    pub fn from_args(a: &Value) -> Result<Self, String> {
        Ok(Self {
            flash_limit: opt_named(a, "flash-limit", &FlashLimit::ALL, flash_limit_name)?,
            output_alpha: opt_named(a, "output-alpha", AlphaOutputMode::ALL, alpha_mode_name)?,
            auto_reconnect: args::opt_bool(a, "auto-reconnect"),
        })
    }
}

/// The web remote. The access key is never reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebStatus {
    pub enabled: bool,
    pub running: bool,
    pub lan: bool,
    pub port: u16,
    pub clients: usize,
}

impl WebStatus {
    pub fn to_json(&self) -> Value {
        json!({
            "enabled": self.enabled,
            "running": self.running,
            "lan": self.lan,
            "port": self.port,
            "clients": self.clients,
        })
    }
}

/// The interface scale at `:scale` (1.0 = 100 %), inside the Appearance
/// slider's range; absent is `None`.
pub fn ui_scale_arg(a: &Value) -> Result<Option<f32>, String> {
    use crate::ui::panels::appearance_panel::{SCALE_MAX, SCALE_MIN};
    #[allow(clippy::cast_precision_loss)]
    let (lo, hi) = (SCALE_MIN as f32 / 100.0, SCALE_MAX as f32 / 100.0);
    match a.get("scale") {
        None => Ok(None),
        Some(_) => match args::opt_f32(a, "scale") {
            Some(s) if s.is_finite() && (lo..=hi).contains(&s) => Ok(Some(s)),
            _ => Err(format!(
                ":scale must be a number in {lo}..={hi} (1.0 = 100 %)"
            )),
        },
    }
}

/// `[{index name current taken}]`: the tours, which one shows, which were
/// finished or skipped.
pub fn tour_listing(names: &[String], current: Option<usize>, taken: &[bool]) -> Value {
    let mut v = listing(names, current);
    if let Value::Array(rows) = &mut v {
        for (row, t) in rows.iter_mut().zip(taken) {
            row["taken"] = json!(t);
        }
    }
    v
}

/// The interface's workspaces (top bar).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workspace {
    Perform,
    Build,
    Setup,
}

impl Workspace {
    pub const ALL: &'static [Self] = &[Self::Perform, Self::Build, Self::Setup];

    pub fn name(self) -> &'static str {
        match self {
            Self::Perform => "perform",
            Self::Build => "build",
            Self::Setup => "setup",
        }
    }
}

/// What is on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewState {
    pub overlay: bool,
    pub fullscreen: bool,
    pub binding_matrix: bool,
    pub chain_editor: bool,
    pub workspace: Workspace,
}

impl ViewState {
    pub fn to_json(&self) -> Value {
        json!({
            "overlay": self.overlay,
            "fullscreen": self.fullscreen,
            "binding-matrix": self.binding_matrix,
            "chain-editor": self.chain_editor,
            "workspace": self.workspace.name(),
        })
    }
}

/// `want`, or the opposite of `now` when no value is given: the
/// toggle-or-set rule of the view ops.
pub fn toggle_or(now: bool, want: Option<bool>) -> bool {
    want.unwrap_or(!now)
}

/// `[{index name current}]` for a list with an optional current item.
pub fn listing(names: &[String], current: Option<usize>) -> Value {
    Value::Array(
        names
            .iter()
            .enumerate()
            .map(|(i, n)| json!({"index": i, "name": n, "current": current == Some(i)}))
            .collect(),
    )
}
