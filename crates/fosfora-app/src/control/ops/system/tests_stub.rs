//! An in-memory stub of every system port, hosting the system addon, for
//! the unit (`tests.rs`) and property (`tests_props.rs`) tests.

use std::path::PathBuf;

use serde_json::Value;

use super::Addon;
use super::model::*;
use super::port::*;
use crate::audio::beat::{TempoCommand, TempoConfig};
use crate::control::{ControlAddon, OpHost, Registry, run};
use crate::midi::types::TriggerAction;
use crate::recording::types::RecordingConfig;
use crate::settings::{AlphaOutputMode, FlashLimit};

pub(super) struct Stub {
    pub(super) tempo: TempoConfig,
    pub(super) commands: Vec<TempoCommand>,
    pub(super) taps: usize,
    pub(super) link: Option<LinkState>,
    pub(super) recording: bool,
    pub(super) ffmpeg: bool,
    pub(super) rec: RecordingConfig,
    pub(super) outputs: Vec<OutputStatus>,
    pub(super) displays: Vec<String>,
    pub(super) window_on: Option<String>,
    pub(super) requested_display: Option<usize>,
    pub(super) settings: SettingsView,
    pub(super) themes: Vec<String>,
    pub(super) theme: usize,
    pub(super) web: WebStatus,
    pub(super) key_replaced: usize,
    pub(super) ui_scale: f32,
    pub(super) theme_reloads: usize,
    pub(super) theme_save_fails: bool,
    pub(super) folder_opened: usize,
    pub(super) tours_taken: Vec<bool>,
    pub(super) view: ViewState,
    pub(super) tours: Vec<String>,
    pub(super) tour: Option<usize>,
    pub(super) tour_step: usize,
    pub(super) quit: Option<bool>,
    pub(super) triggers: Vec<TriggerAction>,
    pub(super) registry: Registry<Stub>,
}

impl Stub {
    pub(super) fn new() -> Self {
        let ndi = OutputStatus {
            available: true,
            name: Some("Fosfora".into()),
            alpha_from_luma: Some(false),
            ..OutputStatus::unavailable(OutputKind::Ndi)
        };
        let v4l2 = OutputStatus {
            available: true,
            pixel_format: Some(PixelFormat::Yuyv),
            devices: vec!["/dev/video10".into()],
            ..OutputStatus::unavailable(OutputKind::V4l2)
        };
        let mut s = Self {
            tempo: TempoConfig::default(),
            commands: Vec::new(),
            taps: 0,
            link: None,
            recording: false,
            ffmpeg: true,
            rec: RecordingConfig::default(),
            outputs: vec![
                ndi,
                OutputStatus::unavailable(OutputKind::Spout),
                OutputStatus::unavailable(OutputKind::Syphon),
                v4l2,
            ],
            displays: vec!["Built-in".into(), "Projector".into()],
            window_on: None,
            requested_display: None,
            settings: SettingsView {
                flash_limit: FlashLimit::Auto,
                output_alpha: AlphaOutputMode::Auto,
                auto_reconnect: true,
            },
            themes: vec!["Light".into(), "Gray".into(), "Black".into()],
            theme: 1,
            web: WebStatus { enabled: false, running: false, lan: false, port: 9090, clients: 0 },
            key_replaced: 0,
            ui_scale: 1.0,
            theme_reloads: 0,
            theme_save_fails: false,
            folder_opened: 0,
            tours_taken: vec![true, false],
            view: ViewState {
                overlay: true,
                fullscreen: false,
                binding_matrix: false,
                chain_editor: false,
                workspace: Workspace::Build,
            },
            tours: vec!["First run".into(), "Layers".into()],
            tour: None,
            tour_step: 0,
            quit: None,
            triggers: Vec::new(),
            registry: Registry::default(),
        };
        s.registry.register(&Addon as &dyn ControlAddon<Stub>);
        s
    }

    pub(super) fn with_link() -> Self {
        let mut s = Self::new();
        s.link = Some(LinkState {
            enabled: false,
            mode: LinkMode::Follow,
            quantum: 4.0,
            start_stop: false,
            peers: None,
            tempo: None,
            phase: None,
            playing: None,
        });
        s
    }

    pub(super) fn call(&mut self, op: &str, args: Value) -> Result<Value, String> {
        run(self, op, &args)
    }

    pub(super) fn out(&mut self, kind: OutputKind) -> &mut OutputStatus {
        self.outputs.iter_mut().find(|o| o.kind == kind).unwrap()
    }
}

impl OpHost for Stub {
    fn registry(&self) -> &Registry<Self> {
        &self.registry
    }
}

impl TempoPort for Stub {
    fn tempo_state(&self) -> (TempoConfig, f32) {
        (self.tempo, 128.0)
    }
    fn set_tempo_config(&mut self, cfg: TempoConfig) {
        self.tempo = cfg;
    }
    fn tempo_command(&mut self, cmd: TempoCommand) {
        self.commands.push(cmd);
    }
    fn tap_tempo(&mut self) {
        self.taps += 1;
    }
    fn link_state(&self) -> Result<LinkState, String> {
        self.link.ok_or_else(|| NO_LINK.to_string())
    }
    fn set_link(&mut self, c: &LinkChange) -> Result<(), String> {
        let l = self.link.as_mut().ok_or(NO_LINK)?;
        l.enabled = c.enabled.unwrap_or(l.enabled);
        l.mode = c.mode.unwrap_or(l.mode);
        l.quantum = c.quantum.map_or(l.quantum, |q| q.clamp(1.0, 16.0));
        l.start_stop = c.start_stop.unwrap_or(l.start_stop);
        Ok(())
    }
}

impl RecordingPort for Stub {
    fn recording_status(&self) -> RecordingStatus {
        RecordingStatus {
            recording: self.recording,
            path: self.recording.then(|| "/tmp/out.mp4".into()),
            seconds: self.recording.then_some(1.0),
            frames: 0,
            bytes: 0,
            audio: false,
            error: None,
            ffmpeg: self.ffmpeg,
        }
    }
    fn recording_config(&self) -> RecordingConfig {
        self.rec.clone()
    }
    fn start_recording(&mut self) -> Result<PathBuf, String> {
        if !self.ffmpeg {
            return Err("ffmpeg not found on PATH".into());
        }
        self.recording = true;
        Ok(PathBuf::from("/tmp/out.mp4"))
    }
    fn stop_recording(&mut self) {
        self.recording = false;
    }
    fn configure_recording(&mut self, change: &RecordingChange) {
        change.apply(&mut self.rec);
    }
}

impl OutputPort for Stub {
    fn output_status(&self, kind: OutputKind) -> OutputStatus {
        self.outputs.iter().find(|o| o.kind == kind).cloned().unwrap()
    }
    fn set_output_enabled(&mut self, kind: OutputKind, on: bool) -> Result<(), String> {
        let o = self.out(kind);
        if !o.available {
            return Err(not_built(kind));
        }
        o.enabled = on;
        o.running = on;
        Ok(())
    }
    fn configure_output(&mut self, kind: OutputKind, changes: &[OutputChange]) -> Result<(), String> {
        let o = self.out(kind);
        if !o.available {
            return Err(not_built(kind));
        }
        for c in changes {
            assert!(accepts(kind, c), "{c:?} reached the port for {kind:?}");
            match c {
                OutputChange::Name(n) => o.name = Some(n.clone()),
                OutputChange::Resolution(r) => o.resolution = *r,
                OutputChange::AlphaFromLuma(b) => o.alpha_from_luma = Some(*b),
                OutputChange::Device(d) => o.device.clone_from(d),
                OutputChange::PixelFormat(p) => o.pixel_format = Some(*p),
            }
        }
        Ok(())
    }
    fn refresh_v4l2_devices(&mut self) -> Result<(), String> {
        self.out(OutputKind::V4l2).devices.push("/dev/video11".into());
        Ok(())
    }
    fn output_window(&self) -> (Vec<String>, Option<String>) {
        (self.displays.clone(), self.window_on.clone())
    }
    fn open_output_window(&mut self, index: usize) {
        self.requested_display = Some(index);
    }
    fn close_output_window(&mut self) {
        self.window_on = None;
    }
}

impl SettingsPort for Stub {
    fn settings_view(&self) -> SettingsView {
        self.settings
    }
    fn set_flash_limit(&mut self, limit: FlashLimit) {
        self.settings.flash_limit = limit;
    }
    fn set_output_alpha(&mut self, mode: AlphaOutputMode) {
        self.settings.output_alpha = mode;
    }
    fn set_auto_reconnect(&mut self, on: bool) {
        self.settings.auto_reconnect = on;
    }
    fn themes(&self) -> (Vec<String>, Option<usize>) {
        (self.themes.clone(), Some(self.theme))
    }
    fn set_theme(&mut self, index: usize) {
        self.theme = index;
    }
    fn web_status(&self) -> WebStatus {
        self.web
    }
    fn set_web(&mut self, enabled: Option<bool>, lan: Option<bool>) {
        self.web.lan = lan.unwrap_or(self.web.lan);
        self.web.enabled = enabled.unwrap_or(self.web.enabled);
        self.web.running = self.web.enabled;
    }
    fn replace_web_key(&mut self) {
        self.key_replaced += 1;
    }
    fn set_web_port(&mut self, port: u16) {
        self.web.port = port;
    }
    fn ui_scale(&self) -> f32 {
        self.ui_scale
    }
    fn set_ui_scale(&mut self, scale: f32) {
        self.ui_scale = scale;
    }
    fn reload_themes(&mut self) {
        self.theme_reloads += 1;
    }
    fn save_theme_copy(&mut self) -> Result<String, String> {
        if self.theme_save_fails {
            return Err("Could not save the theme: read-only".into());
        }
        let name = format!("{} copy", self.themes[self.theme]);
        self.themes.push(name.clone());
        self.theme = self.themes.len() - 1;
        Ok(name)
    }
    fn themes_folder(&self) -> PathBuf {
        PathBuf::from("/cfg/themes")
    }
    fn open_themes_folder(&mut self) -> Result<(), String> {
        self.folder_opened += 1;
        Ok(())
    }
}

impl ShellPort for Stub {
    fn view(&self) -> ViewState {
        self.view
    }
    fn set_overlay_visible(&mut self, visible: bool) {
        self.view.overlay = visible;
    }
    fn set_fullscreen(&mut self, on: bool) {
        self.view.fullscreen = on;
    }
    fn set_binding_matrix_open(&mut self, open: bool) {
        self.view.binding_matrix = open;
        if open {
            self.view.chain_editor = false;
        }
    }
    fn set_chain_editor_open(&mut self, open: bool) {
        self.view.chain_editor = open;
        if open {
            self.view.binding_matrix = false;
        }
    }
    fn set_workspace(&mut self, ws: Workspace) {
        self.view.workspace = ws;
    }
    fn tour_names(&self) -> Vec<String> {
        self.tours.clone()
    }
    fn tours_taken(&self) -> Vec<bool> {
        self.tours_taken.clone()
    }
    fn current_tour(&self) -> Option<(usize, usize)> {
        self.tour.map(|t| (t, self.tour_step))
    }
    fn start_tour(&mut self, index: usize) {
        self.tour = Some(index);
        self.tour_step = 0;
    }
    fn end_tour(&mut self) {
        if let Some(t) = self.tour.take() {
            self.tours_taken[t] = true;
        }
    }
    fn escape(&mut self) -> &'static str {
        if self.view.binding_matrix {
            self.view.binding_matrix = false;
            "closed-binding-matrix"
        } else if !self.view.overlay {
            self.view.overlay = true;
            "showed-interface"
        } else {
            self.quit = Some(false);
            "asked-to-quit"
        }
    }
    fn request_quit(&mut self, confirm: bool) {
        self.quit = Some(confirm);
    }
    fn cancel_quit(&mut self) {
        self.quit = None;
    }
    fn run_trigger(&mut self, action: TriggerAction) {
        self.triggers.push(action);
    }
}
