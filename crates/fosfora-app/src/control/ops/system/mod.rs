//! System ops: tempo and Link, recording, outputs, settings, the window and
//! its shortcuts, triggers, quitting.
//!
//! Shape (see `ops::preset`): Promote and Pipeline in [`model`] (pure),
//! Boundary in [`boundary`] (one port call per op), the ports and their
//! `App` adapters in [`port`], and the UI code paths the ops share with
//! `main.rs` in `actions` (`impl App`).

mod actions;
mod appearance;
mod boundary;
pub mod model;
pub mod port;
mod port_outputs;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_props;
#[cfg(test)]
mod tests_stub;

pub use self::port::{OutputPort, RecordingPort, SettingsPort, ShellPort, SystemPort, TempoPort};
use crate::control::{CljrsNs, ControlAddon, OpSpec};
use boundary as b;

pub struct Addon;

impl<C: SystemPort + 'static> ControlAddon<C> for Addon {
    fn id(&self) -> &'static str {
        "system"
    }

    fn ops(&self) -> Vec<OpSpec<C>> {
        vec![
            // Tempo and Link
            OpSpec::new("system/tempo", "{}", "Tempo prior and shown BPM {bpm center sigma auto preset}.", b::tempo::<C>),
            OpSpec::new(
                "system/tempo-config",
                "{:preset str?, :center num?, :sigma num?, :auto bool?}",
                "Change the tempo prior (saved); returns system/tempo.",
                b::tempo_config::<C>,
            ),
            OpSpec::new("system/tempo-tap", "{}", "One tap of tap-tempo (locks after a few).", b::tempo_tap::<C>),
            OpSpec::new("system/tempo-set", "{:bpm num}", "Lock the detector onto a tempo.", b::tempo_set::<C>),
            OpSpec::new("system/tempo-half", "{}", "Force the detected tempo down an octave.", b::tempo_half::<C>),
            OpSpec::new("system/tempo-double", "{}", "Force the detected tempo up an octave.", b::tempo_double::<C>),
            OpSpec::new(
                "system/link",
                "{:enabled bool?, :mode follow|lead?, :quantum num?, :start-stop bool?}",
                "Read, or change, the Ableton Link session settings.",
                b::link::<C>,
            ),
            // Recording
            OpSpec::new("system/recording", "{}", "Recording status and settings.", b::recording::<C>),
            OpSpec::new("system/record-start", "{}", "Start recording the output; the file path.", b::record_start::<C>),
            OpSpec::new("system/record-stop", "{}", "Stop recording.", b::record_stop::<C>),
            OpSpec::new("system/record-toggle", "{}", "Start or stop recording (the Record button).", b::record_toggle::<C>),
            OpSpec::new(
                "system/recording-config",
                "{:codec h264|hevc|av1?, :resolution match|720p|1080p|4k|8k?, :fps int?, :quality int?, :container mp4|mkv?, :hw bool?, :audio bool?}",
                "Change recording settings (saved).",
                b::recording_config::<C>,
            ),
            // Outputs
            OpSpec::new("system/outputs", "{}", "Every output sink: ndi, spout, syphon, v4l2.", b::outputs::<C>),
            OpSpec::new(
                "system/output",
                "{:kind ndi|spout|syphon|v4l2, :enabled bool?, :name str?, :resolution str?, :alpha-from-luma bool?, :device str|auto?, :pixel-format yuyv|bgrx?}",
                "Read, switch or configure one output sink.",
                b::output::<C>,
            ),
            OpSpec::new("system/v4l2-refresh", "{}", "Re-scan v4l2 loopback devices.", b::v4l2_refresh::<C>),
            OpSpec::new(
                "system/output-window",
                "{:index int | :name str, :close bool?}",
                "Open the output window on a display, close it, or with {} list displays.",
                b::output_window::<C>,
            ),
            // Settings
            OpSpec::new(
                "system/settings",
                "{:flash-limit auto|standard|strict|off?, :output-alpha auto|opaque|luma|passthrough?, :auto-reconnect bool?}",
                "Read, or change, the general settings.",
                b::settings::<C>,
            ),
            OpSpec::new("system/theme", "{:index int | :name str}?", "List themes, or use one.", b::theme::<C>),
            OpSpec::new(
                "system/theme-reload",
                "{}",
                "Read the themes folder again (Reload); the theme list.",
                b::theme_reload::<C>,
            ),
            OpSpec::new(
                "system/theme-save-copy",
                "{}",
                "Save the theme in use as a new file to edit, and use it.",
                b::theme_save_copy::<C>,
            ),
            OpSpec::new(
                "system/theme-folder",
                "{:open bool?}",
                "Where theme files live; :open true shows it in the file browser.",
                b::theme_folder::<C>,
            ),
            OpSpec::new(
                "system/ui-scale",
                "{:scale num?}",
                "Read, or change, the interface scale (0.8..2.0, 1.0 = 100 %; saved).",
                b::ui_scale::<C>,
            ),
            OpSpec::new(
                "system/web",
                "{:enabled bool?, :lan bool?, :port int?}",
                "Read, or change, the web remote (a new :port restarts it).",
                b::web::<C>,
            ),
            OpSpec::new(
                "system/web-replace-key",
                "{}",
                "New web remote access key; connected devices drop.",
                b::web_replace_key::<C>,
            ),
            // Window, view, shortcuts
            OpSpec::new("system/view", "{}", "What is on screen {overlay fullscreen binding-matrix chain-editor workspace}.", b::view::<C>),
            OpSpec::new("system/overlay", "{:visible bool?}", "Show/hide the interface (D, Full output); {} toggles.", b::overlay::<C>),
            OpSpec::new("system/fullscreen", "{:on bool?}", "Main window full screen (F); {} toggles.", b::fullscreen::<C>),
            OpSpec::new("system/binding-matrix", "{:open bool?}", "Open/close the binding matrix (B); {} toggles.", b::binding_matrix::<C>),
            OpSpec::new("system/chain-editor", "{:open bool?}", "Open/close the chain editor (C); {} toggles.", b::chain_editor::<C>),
            OpSpec::new("system/workspace", "{:name perform|build|setup?}", "Read, or switch, the workspace.", b::workspace::<C>),
            OpSpec::new("system/tours", "{}", "The guided tours {index name current taken}.", b::tours::<C>),
            OpSpec::new(
                "system/tour",
                "{:index int | :name str | :end true}?",
                "Start a guided tour, end the one showing, or with {} say which shows.",
                b::tour::<C>,
            ),
            OpSpec::new("system/escape", "{}", "One step of the Esc key; what it did.", b::escape::<C>),
            OpSpec::new(
                "system/quit",
                "{:confirm bool?, :cancel bool?}",
                "Ask to quit, quit with :confirm true, or close the dialog with :cancel true.",
                b::quit::<C>,
            ),
            // Triggers
            OpSpec::new("system/triggers", "{}", "Every trigger action name.", b::triggers::<C>),
            OpSpec::new("system/trigger", "{:action str}", "Fire a MIDI/OSC/web trigger action.", b::trigger::<C>),
        ]
    }

    fn ui_signals(&self) -> &'static [(&'static str, &'static str)] {
        &[
            ("link_set_enabled", "system/link"),
            ("link_set_mode", "system/link"),
            ("link_set_quantum", "system/link"),
            ("link_set_start_stop", "system/link"),
            ("tempo_config_dirty", "system/tempo-config"),
            ("ndi_set_enabled", "system/output"),
            ("ndi_source_name", "system/output"),
            ("ndi_resolution_change", "system/output"),
            ("ndi_alpha_from_luma", "system/output"),
            ("spout_set_enabled", "system/output"),
            ("spout_sender_name", "system/output"),
            ("spout_resolution_change", "system/output"),
            ("syphon_set_enabled", "system/output"),
            ("syphon_server_name", "system/output"),
            ("syphon_resolution_change", "system/output"),
            ("v4l2_set_enabled", "system/output"),
            ("v4l2_device_path", "system/output"),
            ("v4l2_resolution_change", "system/output"),
            ("v4l2_pixel_format", "system/output"),
            ("v4l2_refresh_devices", "system/v4l2-refresh"),
            ("recording_toggle", "system/record-toggle"),
            ("rec_codec_change", "system/recording-config"),
            ("rec_resolution_change", "system/recording-config"),
            ("rec_fps_change", "system/recording-config"),
            ("rec_quality_change", "system/recording-config"),
            ("rec_container_change", "system/recording-config"),
            ("rec_hw_toggle", "system/recording-config"),
            ("rec_audio_toggle", "system/recording-config"),
            ("set_output_alpha", "system/settings"),
            ("set_flash_limit", "system/settings"),
            ("set_auto_reconnect", "system/settings"),
            ("request_hide_overlay", "system/overlay"),
            ("confirm_quit", "system/quit"),
            ("quit_dialog_shown", "system/quit"),
        ]
    }

    fn cljrs(&self) -> Option<CljrsNs> {
        Some(CljrsNs {
            ns: "fosfora.system",
            alias: "sys",
            src: include_str!("system.cljrs"),
        })
    }
}
