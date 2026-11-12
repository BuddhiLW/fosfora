//! What the system ops need from the app, as five role-sized ports (ISP),
//! and the adapters that reach into [`App`] for them (DIP); the
//! [`OutputPort`] adapter is in `port_outputs`. Besides `actions` (the UI
//! code paths themselves), nothing else in the domain touches `App`.

use std::path::PathBuf;

#[cfg(not(feature = "link"))]
use super::model::NO_LINK;
use super::model::{
    LinkChange, LinkState, OutputChange, OutputKind, OutputStatus, RecordingChange,
    RecordingStatus, SettingsView, ViewState, WebStatus, Workspace,
};
use crate::app::App;
use crate::audio::beat::{TempoCommand, TempoConfig};
use crate::midi::types::TriggerAction;
use crate::recording::types::RecordingConfig;
use crate::settings::{AlphaOutputMode, FlashLimit};

/// Tempo detection and Ableton Link.
pub trait TempoPort {
    /// The tempo prior and the BPM shown on screen.
    fn tempo_state(&self) -> (TempoConfig, f32);
    /// Replace the tempo prior, live and in Settings.
    fn set_tempo_config(&mut self, cfg: TempoConfig);
    /// Queue a one-shot override (octave shift, tempo lock).
    fn tempo_command(&mut self, cmd: TempoCommand);
    /// One tap of tap-tempo; the average locks once enough taps landed.
    fn tap_tempo(&mut self);
    /// Err when built without Link.
    fn link_state(&self) -> Result<LinkState, String>;
    fn set_link(&mut self, change: &LinkChange) -> Result<(), String>;
}

/// Video recording.
pub trait RecordingPort {
    fn recording_status(&self) -> RecordingStatus;
    fn recording_config(&self) -> RecordingConfig;
    /// Start recording the output; the file it writes.
    fn start_recording(&mut self) -> Result<PathBuf, String>;
    fn stop_recording(&mut self);
    /// Change recording settings (saved).
    fn configure_recording(&mut self, change: &RecordingChange);
}

/// Output sinks and the second output window.
pub trait OutputPort {
    fn output_status(&self, kind: OutputKind) -> OutputStatus;
    fn set_output_enabled(&mut self, kind: OutputKind, on: bool) -> Result<(), String>;
    /// Apply `changes` (all accepted by `kind`), saved, restarting the sink once.
    fn configure_output(
        &mut self,
        kind: OutputKind,
        changes: &[OutputChange],
    ) -> Result<(), String>;
    /// Re-scan v4l2 loopback devices.
    fn refresh_v4l2_devices(&mut self) -> Result<(), String>;
    /// Every display (by label) and the one the output window is on.
    fn output_window(&self) -> (Vec<String>, Option<String>);
    /// Ask the frame loop to open the output window on display `index`.
    fn open_output_window(&mut self, index: usize);
    fn close_output_window(&mut self);
}

/// General settings, theme and the web remote.
pub trait SettingsPort {
    fn settings_view(&self) -> SettingsView;
    fn set_flash_limit(&mut self, limit: FlashLimit);
    fn set_output_alpha(&mut self, mode: AlphaOutputMode);
    fn set_auto_reconnect(&mut self, on: bool);
    /// Every theme name (built-in first) and the one in use.
    fn themes(&self) -> (Vec<String>, Option<usize>);
    fn set_theme(&mut self, index: usize);
    fn web_status(&self) -> WebStatus;
    fn set_web(&mut self, enabled: Option<bool>, lan: Option<bool>);
    /// Serve the web remote on `port` (saved); restarts it.
    fn set_web_port(&mut self, port: u16);
    /// New access key; every connected device drops.
    fn replace_web_key(&mut self);
    /// The interface scale in use (1.0 = 100 %).
    fn ui_scale(&self) -> f32;
    /// Rescale the interface (the Appearance slider; applied and saved by
    /// the next frame).
    fn set_ui_scale(&mut self, scale: f32);
    /// Read the themes folder again.
    fn reload_themes(&mut self);
    /// Save the theme in use as a new file and use it; its name.
    fn save_theme_copy(&mut self) -> Result<String, String>;
    /// Where theme files live.
    fn themes_folder(&self) -> PathBuf;
    /// Show the themes folder in the system file browser.
    fn open_themes_folder(&mut self) -> Result<(), String>;
}

/// The window, its modals, the shortcuts and quitting.
pub trait ShellPort {
    fn view(&self) -> ViewState;
    fn set_overlay_visible(&mut self, visible: bool);
    fn set_fullscreen(&mut self, on: bool);
    fn set_binding_matrix_open(&mut self, open: bool);
    fn set_chain_editor_open(&mut self, open: bool);
    fn set_workspace(&mut self, ws: Workspace);
    fn tour_names(&self) -> Vec<String>;
    /// Per tour, whether it was finished or skipped (Setup › Tutorials' "taken").
    fn tours_taken(&self) -> Vec<bool>;
    /// The tour showing and its step.
    fn current_tour(&self) -> Option<(usize, usize)>;
    fn start_tour(&mut self, index: usize);
    /// Stop the tour showing, as Skip tour does; it counts as taken.
    fn end_tour(&mut self);
    /// One step of the Escape key's ladder; what it did.
    fn escape(&mut self) -> &'static str;
    /// Show the quit dialog, or with `confirm` quit at the end of the frame.
    fn request_quit(&mut self, confirm: bool);
    /// The quit dialog's Cancel.
    fn cancel_quit(&mut self);
    /// What a MIDI/OSC/web trigger does.
    fn run_trigger(&mut self, action: TriggerAction);
}

/// Everything the system addon needs.
pub trait SystemPort: TempoPort + RecordingPort + OutputPort + SettingsPort + ShellPort {}
impl<T: TempoPort + RecordingPort + OutputPort + SettingsPort + ShellPort> SystemPort for T {}

// ── App adapters ─────────────────────────────────────────────────────────

impl TempoPort for App {
    fn tempo_state(&self) -> (TempoConfig, f32) {
        let cfg = self
            .audio
            .tempo()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .config;
        (cfg, self.uniforms.bpm * 300.0)
    }

    fn set_tempo_config(&mut self, cfg: TempoConfig) {
        self.audio
            .tempo()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .config = cfg;
        self.save_tempo_config();
    }

    fn tempo_command(&mut self, cmd: TempoCommand) {
        self.audio.send_tempo_command(cmd);
    }

    fn tap_tempo(&mut self) {
        self.audio.tap_tempo();
    }

    #[cfg(feature = "link")]
    fn link_state(&self) -> Result<LinkState, String> {
        use super::model::LinkMode;
        let c = &self.link.config;
        let tick = self.link.last_tick();
        Ok(LinkState {
            enabled: c.enabled,
            mode: match c.mode {
                crate::link::LinkMode::Follow => LinkMode::Follow,
                crate::link::LinkMode::Lead => LinkMode::Lead,
            },
            quantum: c.quantum,
            start_stop: c.start_stop_sync,
            peers: tick.map(|t| t.peers),
            tempo: tick.map(|t| t.tempo),
            phase: tick.map(|t| t.quantum_phase),
            playing: tick.map(|t| t.playing),
        })
    }

    #[cfg(not(feature = "link"))]
    fn link_state(&self) -> Result<LinkState, String> {
        Err(NO_LINK.into())
    }

    #[cfg(feature = "link")]
    fn set_link(&mut self, change: &LinkChange) -> Result<(), String> {
        use super::model::LinkMode;
        if let Some(on) = change.enabled {
            self.link.set_enabled(on);
        }
        if let Some(mode) = change.mode {
            self.link.set_mode(match mode {
                LinkMode::Follow => crate::link::LinkMode::Follow,
                LinkMode::Lead => crate::link::LinkMode::Lead,
            });
        }
        if let Some(q) = change.quantum {
            self.link.set_quantum(q);
        }
        if let Some(on) = change.start_stop {
            self.link.set_start_stop_sync(on);
        }
        Ok(())
    }

    #[cfg(not(feature = "link"))]
    fn set_link(&mut self, _change: &LinkChange) -> Result<(), String> {
        Err(NO_LINK.into())
    }
}

impl RecordingPort for App {
    fn recording_status(&self) -> RecordingStatus {
        let r = &self.recording;
        RecordingStatus {
            recording: r.is_recording(),
            path: r.output_path().map(|p| p.display().to_string()),
            seconds: r.duration().map(|d| d.as_secs_f64()),
            frames: r.frames_encoded(),
            bytes: r.total_bytes_written(),
            audio: r.has_audio(),
            error: match &r.state {
                crate::recording::RecordingState::Error(e) => Some(e.clone()),
                _ => None,
            },
            ffmpeg: r.encoder_info.ffmpeg_found,
        }
    }

    fn recording_config(&self) -> RecordingConfig {
        self.recording.config.clone()
    }

    fn start_recording(&mut self) -> Result<PathBuf, String> {
        self.try_start_recording()
    }

    fn stop_recording(&mut self) {
        App::stop_recording(self);
    }

    fn configure_recording(&mut self, change: &RecordingChange) {
        App::configure_recording(self, |c| change.apply(c));
    }
}

impl SettingsPort for App {
    fn settings_view(&self) -> SettingsView {
        SettingsView {
            flash_limit: self.settings.flash_limit,
            output_alpha: self.settings.output_alpha,
            auto_reconnect: self.settings.auto_reconnect,
        }
    }

    fn set_flash_limit(&mut self, limit: FlashLimit) {
        App::set_flash_limit(self, limit);
    }

    fn set_output_alpha(&mut self, mode: AlphaOutputMode) {
        App::set_output_alpha(self, mode);
    }

    fn set_auto_reconnect(&mut self, on: bool) {
        App::set_auto_reconnect(self, on);
    }

    fn themes(&self) -> (Vec<String>, Option<usize>) {
        let list = self.theme_list();
        let names = list
            .iter()
            .map(|t| t.display_name(&self.custom_themes))
            .collect();
        (names, list.iter().position(|t| *t == self.settings.theme))
    }

    fn set_theme(&mut self, index: usize) {
        if let Some(theme) = self.theme_list().into_iter().nth(index) {
            self.apply_theme(theme);
        }
    }

    fn web_status(&self) -> WebStatus {
        let c = &self.web.config;
        WebStatus {
            enabled: c.enabled,
            running: self.web.is_running(),
            lan: c.lan,
            port: c.port,
            clients: self.web.client_count,
        }
    }

    fn set_web(&mut self, enabled: Option<bool>, lan: Option<bool>) {
        if let Some(lan) = lan {
            self.web.set_lan(lan);
        }
        if let Some(on) = enabled {
            self.web.set_enabled(on);
        }
    }

    fn replace_web_key(&mut self) {
        self.web.replace_access_key();
    }

    fn set_web_port(&mut self, port: u16) {
        self.web.set_port(port);
    }

    fn ui_scale(&self) -> f32 {
        self.settings.ui_scale
    }

    fn set_ui_scale(&mut self, scale: f32) {
        self.request_ui_scale(scale);
    }

    fn reload_themes(&mut self) {
        App::reload_themes(self);
    }

    fn save_theme_copy(&mut self) -> Result<String, String> {
        let theme = App::save_theme_copy(self)?;
        Ok(theme.display_name(&self.custom_themes))
    }

    fn themes_folder(&self) -> PathBuf {
        crate::ui::theme::custom::themes_dir()
    }

    fn open_themes_folder(&mut self) -> Result<(), String> {
        App::open_themes_folder(self).map(|_| ())
    }
}

impl ShellPort for App {
    fn view(&self) -> ViewState {
        use crate::ui::shell::Workspace as Ws;
        ViewState {
            overlay: self.egui_overlay.visible,
            fullscreen: self.window.fullscreen().is_some(),
            binding_matrix: self.binding_matrix.open,
            chain_editor: self.trama.canvas_open,
            workspace: match Ws::read(&self.egui_overlay.context()) {
                Ws::Perform => Workspace::Perform,
                Ws::Build => Workspace::Build,
                Ws::Setup => Workspace::Setup,
            },
        }
    }

    fn set_overlay_visible(&mut self, visible: bool) {
        App::set_overlay_visible(self, visible);
    }

    fn set_fullscreen(&mut self, on: bool) {
        App::set_fullscreen(self, on);
    }

    fn set_binding_matrix_open(&mut self, open: bool) {
        App::set_binding_matrix_open(self, open);
    }

    fn set_chain_editor_open(&mut self, open: bool) {
        App::set_chain_editor_open(self, open);
    }

    fn set_workspace(&mut self, ws: Workspace) {
        use crate::ui::shell::Workspace as Ws;
        let ws = match ws {
            Workspace::Perform => Ws::Perform,
            Workspace::Build => Ws::Build,
            Workspace::Setup => Ws::Setup,
        };
        ws.write(&self.egui_overlay.context());
    }

    fn tour_names(&self) -> Vec<String> {
        crate::ui::tour::Tour::ALL
            .iter()
            .map(|t| t.name().to_string())
            .collect()
    }

    fn tours_taken(&self) -> Vec<bool> {
        let done = &self.settings.tours_done;
        crate::ui::tour::Tour::ALL
            .iter()
            .map(|t| done.iter().any(|k| k == t.key()))
            .collect()
    }

    fn current_tour(&self) -> Option<(usize, usize)> {
        use crate::ui::tour::{Tour, current};
        let (tour, step) = current(&self.egui_overlay.context())?;
        Tour::ALL.iter().position(|&t| t == tour).map(|i| (i, step))
    }

    fn start_tour(&mut self, index: usize) {
        if let Some(&t) = crate::ui::tour::Tour::ALL.get(index) {
            crate::ui::tour::start(&self.egui_overlay.context(), t);
        }
    }

    fn end_tour(&mut self) {
        crate::ui::tour::end(&self.egui_overlay.context());
    }

    fn escape(&mut self) -> &'static str {
        App::escape(self)
    }

    fn request_quit(&mut self, confirm: bool) {
        App::request_quit(self, confirm);
    }

    fn cancel_quit(&mut self) {
        App::cancel_quit(self);
    }

    fn run_trigger(&mut self, action: TriggerAction) {
        App::run_trigger(self, action);
    }
}
