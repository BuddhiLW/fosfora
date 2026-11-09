//! The system actions the UI and the ops share: one `App` method per action,
//! called from `main.rs`'s signal handlers, shortcuts and trigger loop, and
//! from the [`super::port`] adapters.

use std::path::PathBuf;

use crate::app::App;
use crate::midi::types::TriggerAction;
use crate::recording::types::RecordingConfig;
use crate::settings::{AlphaOutputMode, FlashLimit};
use crate::ui::theme::ThemeMode;

impl App {
    /// What a MIDI, OSC or web trigger does.
    pub fn run_trigger(&mut self, trigger: TriggerAction) {
        // Visible (non-hidden) effect indices, for cycling.
        let visible: Vec<usize> = self
            .effect_loader
            .effects
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.hidden)
            .map(|(i, _)| i)
            .collect();
        match trigger {
            TriggerAction::NextEffect if !visible.is_empty() => {
                let current = self.layer_stack.active().and_then(|l| l.effect_index()).unwrap_or(0);
                let pos = visible.iter().position(|&i| i == current).unwrap_or(0);
                self.load_effect(visible[(pos + 1) % visible.len()]);
            }
            TriggerAction::PrevEffect if !visible.is_empty() => {
                let current = self.layer_stack.active().and_then(|l| l.effect_index()).unwrap_or(0);
                let pos = visible.iter().position(|&i| i == current).unwrap_or(0);
                self.load_effect(visible[if pos == 0 { visible.len() - 1 } else { pos - 1 }]);
            }
            TriggerAction::TogglePostProcess => {
                self.post_process.enabled = !self.post_process.enabled;
                self.master_postprocess.enabled = self.post_process.enabled;
            }
            TriggerAction::ToggleOverlay => self.egui_overlay.toggle_visible(),
            TriggerAction::NextPreset if !self.preset_store.presets.is_empty() => {
                let num = self.preset_store.presets.len();
                let current = self.target_preset().unwrap_or(0);
                self.switch_preset((current + 1) % num);
            }
            TriggerAction::PrevPreset if !self.preset_store.presets.is_empty() => {
                let num = self.preset_store.presets.len();
                let current = self.target_preset().unwrap_or(0);
                self.switch_preset(if current == 0 { num - 1 } else { current - 1 });
            }
            TriggerAction::NextLayer if self.layer_stack.layers.len() > 1 => {
                let num = self.layer_stack.layers.len();
                self.layer_stack.active_layer = (self.layer_stack.active_layer + 1) % num;
                self.sync_active_layer();
            }
            TriggerAction::PrevLayer if self.layer_stack.layers.len() > 1 => {
                let num = self.layer_stack.layers.len();
                let current = self.layer_stack.active_layer;
                self.layer_stack.active_layer = if current == 0 { num - 1 } else { current - 1 };
                self.sync_active_layer();
            }
            TriggerAction::SceneGoNext => {
                let event = self.timeline.go_next();
                self.process_timeline_event(event);
            }
            TriggerAction::SceneGoPrev => {
                let event = self.timeline.go_prev();
                self.process_timeline_event(event);
            }
            TriggerAction::TempoHalf => {
                self.audio.send_tempo_command(crate::audio::TempoCommand::ShiftOctave(-1));
            }
            TriggerAction::TempoDouble => {
                self.audio.send_tempo_command(crate::audio::TempoCommand::ShiftOctave(1));
            }
            TriggerAction::TempoTap => self.audio.tap_tempo(),
            TriggerAction::ToggleTimeline => {
                if self.timeline.active {
                    self.timeline.stop();
                } else if !self.timeline.cues.is_empty() {
                    let event = self.timeline.start(0);
                    self.process_timeline_event(event);
                }
            }
            _ => {}
        }
    }

    /// One step of the Escape key: cancel a half-finished click-to-bind, close
    /// the binding matrix, bring the interface back, let a tour end itself,
    /// close the chain editor, close the output window, then ask to quit
    /// (unless the shader editor is open). Returns the step taken.
    pub fn escape(&mut self) -> &'static str {
        if self.binding_matrix.open && self.binding_matrix.armed.is_some() {
            self.binding_matrix.armed = None;
            "cancelled-bind"
        } else if self.binding_matrix.open {
            self.binding_matrix.open = false;
            "closed-binding-matrix"
        } else if !self.egui_overlay.visible {
            self.egui_overlay.toggle_visible();
            "showed-interface"
        } else if crate::ui::tour::current(&self.egui_overlay.context()).is_some() {
            // The tour sees the same Escape in its own frame and ends itself.
            "tour"
        } else if self.trama.canvas_open {
            self.trama.canvas_open = false;
            "closed-chain-editor"
        } else if self.output_window.is_some() {
            self.close_output_window();
            "closed-output-window"
        } else if !self.shader_editor.open {
            self.quit_requested = true;
            "asked-to-quit"
        } else {
            "nothing"
        }
    }

    pub fn set_fullscreen(&mut self, on: bool) {
        self.window
            .set_fullscreen(on.then_some(winit::window::Fullscreen::Borderless(None)));
    }

    pub fn toggle_fullscreen(&mut self) {
        let on = self.window.fullscreen().is_none();
        self.set_fullscreen(on);
    }

    /// Show or hide the interface (the output stays).
    pub fn set_overlay_visible(&mut self, visible: bool) {
        if self.egui_overlay.visible != visible {
            self.egui_overlay.toggle_visible();
        }
    }

    /// Open or close the binding matrix. One modal at a time: opening it
    /// closes the chain editor.
    pub fn set_binding_matrix_open(&mut self, open: bool) {
        self.binding_matrix.open = open;
        if open {
            self.trama.canvas_open = false;
        }
    }

    /// Open or close the chain editor; opening it closes the binding matrix.
    pub fn set_chain_editor_open(&mut self, open: bool) {
        self.trama.canvas_open = open;
        if open {
            self.binding_matrix.open = false;
        }
    }

    /// Show the quit dialog, or with `confirm` quit at the end of this frame
    /// (the `confirm_quit` signal the dialog's Quit button raises).
    pub fn request_quit(&mut self, confirm: bool) {
        if confirm {
            self.egui_overlay
                .context()
                .data_mut(|d| d.insert_temp(egui::Id::new("confirm_quit"), true));
        } else {
            self.quit_requested = true;
        }
    }

    /// Persist what must survive a quit; the caller then ends the event loop.
    pub fn prepare_quit(&mut self) {
        self.gpu.save_pipeline_cache();
        // A global binding edit still inside its 1 s debounce window.
        self.binding_bus.flush();
    }

    pub fn set_output_alpha(&mut self, mode: AlphaOutputMode) {
        self.settings.output_alpha = mode;
        self.settings.save();
    }

    pub fn set_flash_limit(&mut self, limit: FlashLimit) {
        self.settings.flash_limit = limit;
        self.settings.save();
    }

    pub fn set_auto_reconnect(&mut self, on: bool) {
        self.settings.auto_reconnect = on;
        self.settings.save();
        self.audio.set_auto_reconnect(on);
    }

    /// Save the live tempo prior into Settings. The audio thread already has
    /// it through the shared mutex.
    pub fn save_tempo_config(&mut self) {
        self.settings.tempo = self.audio.tempo().lock().unwrap_or_else(|e| e.into_inner()).config;
        self.settings.save();
    }

    /// Every theme: the built-in ones, then the theme files.
    pub fn theme_list(&self) -> Vec<ThemeMode> {
        ThemeMode::BUILT_IN
            .into_iter()
            .chain(self.custom_themes.iter().map(|t| ThemeMode::Custom(t.stem.clone())))
            .collect()
    }

    /// Draw with `theme` and remember it.
    pub fn apply_theme(&mut self, theme: ThemeMode) {
        self.egui_overlay.set_palette(theme.palette(&self.custom_themes));
        if theme != self.settings.theme {
            self.settings.theme = theme;
            self.settings.save();
        }
    }

    /// Start recording the output, with audio when capture is running.
    pub fn try_start_recording(&mut self) -> Result<PathBuf, String> {
        let audio_source = self.audio.active.then(|| crate::recording::encoder::AudioSource {
            ring: self.audio.recording_ring.clone(),
            sample_rate: self.audio.recording_rate,
        });
        let (w, h) = self.render_size();
        self.recording.start(&self.gpu.device, self.gpu.format, w, h, audio_source)
    }

    pub fn stop_recording(&mut self) {
        self.recording.stop();
    }

    /// The Record button: stop, or start and put a failure on the status bar.
    pub fn toggle_recording(&mut self) {
        if self.recording.is_recording() {
            self.stop_recording();
        } else if let Err(e) = self.try_start_recording() {
            log::error!("Failed to start recording: {e}");
            self.status_error = Some((
                format!("Recording failed to start: {e}"),
                std::time::Instant::now(),
            ));
        }
    }

    /// Change recording settings and save them; the next recording uses them.
    pub fn configure_recording(&mut self, f: impl FnOnce(&mut RecordingConfig)) {
        f(&mut self.recording.config);
        self.recording.config.save();
    }
}

#[cfg(feature = "ndi")]
impl App {
    pub fn set_ndi_enabled(&mut self, on: bool) {
        let (w, h) = self.render_size();
        self.ndi.set_enabled(on, &self.gpu.device, self.gpu.format, w, h);
    }

    /// Change NDI settings and save them; the stream restarts when its name
    /// or resolution changed.
    pub fn configure_ndi(&mut self, f: impl FnOnce(&mut crate::ndi::types::NdiConfig)) {
        let before = (self.ndi.config.source_name.clone(), self.ndi.config.resolution);
        f(&mut self.ndi.config);
        self.ndi.config.save();
        if before != (self.ndi.config.source_name.clone(), self.ndi.config.resolution) {
            let (w, h) = self.render_size();
            self.ndi.restart(&self.gpu.device, self.gpu.format, w, h);
        }
    }
}

#[cfg(all(target_os = "linux", feature = "v4l2"))]
impl App {
    pub fn set_v4l2_enabled(&mut self, on: bool) {
        let (w, h) = self.render_size();
        self.v4l2.set_enabled(on, &self.gpu.device, self.gpu.format, w, h);
    }

    /// Change v4l2 settings and save them; the stream restarts when anything
    /// it negotiated changed.
    pub fn configure_v4l2(&mut self, f: impl FnOnce(&mut crate::v4l2::types::V4l2Config)) {
        let key = |c: &crate::v4l2::types::V4l2Config| {
            (c.device_path.clone(), c.resolution, c.pixel_format)
        };
        let before = key(&self.v4l2.config);
        f(&mut self.v4l2.config);
        self.v4l2.config.save();
        if before != key(&self.v4l2.config) {
            let (w, h) = self.render_size();
            self.v4l2.restart(&self.gpu.device, self.gpu.format, w, h);
        }
    }
}

#[cfg(all(target_os = "windows", feature = "spout"))]
impl App {
    pub fn set_spout_enabled(&mut self, on: bool) {
        let (w, h) = self.render_size();
        self.spout.set_enabled(on, &self.gpu.device, self.gpu.format, w, h);
    }

    /// Change Spout settings and save them; restarts on a name or resolution change.
    pub fn configure_spout(&mut self, f: impl FnOnce(&mut crate::spout::types::SpoutConfig)) {
        let before = (self.spout.config.sender_name.clone(), self.spout.config.resolution);
        f(&mut self.spout.config);
        self.spout.config.save();
        if before != (self.spout.config.sender_name.clone(), self.spout.config.resolution) {
            let (w, h) = self.render_size();
            self.spout.restart(&self.gpu.device, self.gpu.format, w, h);
        }
    }
}

#[cfg(all(target_os = "macos", feature = "syphon"))]
impl App {
    pub fn set_syphon_enabled(&mut self, on: bool) {
        let (w, h) = self.render_size();
        self.syphon.set_enabled(on, &self.gpu.device, self.gpu.format, w, h);
    }

    /// Change Syphon settings and save them; restarts on a name or resolution change.
    pub fn configure_syphon(&mut self, f: impl FnOnce(&mut crate::syphon::types::SyphonConfig)) {
        let before = (self.syphon.config.server_name.clone(), self.syphon.config.resolution);
        f(&mut self.syphon.config);
        self.syphon.config.save();
        if before != (self.syphon.config.server_name.clone(), self.syphon.config.resolution) {
            let (w, h) = self.render_size();
            self.syphon.restart(&self.gpu.device, self.gpu.format, w, h);
        }
    }
}
