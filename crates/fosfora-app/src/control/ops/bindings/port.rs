//! What the binding ops need from the app (ISP), split by role: the Binding
//! Matrix ([`BindingsPort`]), the audio input ([`AudioPort`]) and MIDI/OSC
//! mappings ([`MappingPort`]). Implemented for `App` in `adapter.rs`, the
//! only code here that touches `App` internals (DIP).

use crate::audio::StructureConfig;
use crate::bindings::types::{Binding, BindingScope, BindingTarget, TransformDef};
use crate::midi::mapping::MidiMapping;
use crate::midi::types::TriggerAction;
use crate::settings::BandScale;

// ── Port vocabulary ────────────────────────────────────────────────────────

/// One bindable source as the matrix lists it, with its value last frame.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceRow {
    pub group: String,
    pub key: String,
    pub value: Option<f32>,
}

/// One target the matrix offers: its wire form, label and picker group.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetRow {
    pub target: String,
    pub label: String,
    pub group: String,
}

/// Every editable field of a binding (id and scope never change).
#[derive(Debug, Clone, PartialEq)]
pub struct BindingEdit {
    pub source: String,
    pub target: BindingTarget,
    pub name: String,
    pub enabled: bool,
    pub transforms: Vec<TransformDef>,
}

impl BindingEdit {
    pub fn of(b: &Binding) -> Self {
        Self {
            source: b.source.clone(),
            target: b.target.clone(),
            name: b.name.clone(),
            enabled: b.enabled,
            transforms: b.transforms.clone(),
        }
    }
}

impl Default for BindingEdit {
    /// What the matrix's "+" creates: no source, no target, enabled.
    fn default() -> Self {
        Self {
            source: String::new(),
            target: BindingTarget::Unset,
            name: String::new(),
            enabled: true,
            transforms: Vec::new(),
        }
    }
}

/// The audio input as the Audio page shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioState {
    /// The device listened to now.
    pub device: String,
    pub active: bool,
    pub error: Option<String>,
    /// The device saved in Settings (`None`: the default input).
    pub saved: Option<String>,
}

/// Which mapping system an op addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proto {
    Midi,
    Osc,
}

/// What a MIDI/OSC mapping drives: a param of the active layer, or a trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapTarget {
    Param(String),
    Trigger(TriggerAction),
}

/// Where a mapping listens.
#[derive(Debug, Clone)]
pub enum MappingSource {
    Midi(MidiMapping),
    Osc(String),
}

#[derive(Debug, Clone)]
pub struct MappingRow {
    pub target: MapTarget,
    pub source: MappingSource,
}

/// The OSC listener and sender settings, as Setup › Control shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OscSettings {
    pub enabled: bool,
    pub rx_port: u16,
    pub rx_lan: bool,
    /// A message arrived in the last 300 ms.
    pub active: bool,
    pub last_address: Option<String>,
    pub tx_enabled: bool,
    pub tx_host: String,
    pub tx_port: u16,
    pub tx_rate_hz: u32,
    pub tx_prefix: String,
}

// ── Ports ──────────────────────────────────────────────────────────────────

/// The Binding Matrix slice of the app. Bindings are addressed by id.
pub trait BindingsPort {
    /// Every binding, global and preset-scoped, in matrix order.
    fn bindings(&self) -> Vec<Binding>;
    /// A binding's last input and output value, if it ran.
    fn binding_io(&self, id: &str) -> (Option<f32>, Option<f32>);
    /// Every bindable source this frame, grouped as the matrix shows them.
    fn sources(&self) -> Vec<SourceRow>;
    /// Every target the matrix offers for the current stack.
    fn targets(&self) -> Vec<TargetRow>;
    /// Create a binding in `scope` with the fields of `edit`; its id.
    fn add_binding(&mut self, scope: BindingScope, edit: BindingEdit) -> String;
    /// Replace the editable fields of binding `id`.
    fn edit_binding(&mut self, id: &str, edit: BindingEdit) -> Result<(), String>;
    fn remove_binding(&mut self, id: &str);
    /// Clone binding `id` under a fresh id; that id.
    fn duplicate_binding(&mut self, id: &str) -> Option<String>;
    /// The binding whose source is being learned from the next MIDI/OSC input.
    fn learning(&self) -> Option<String>;
    /// Learn the source of `id` from the next MIDI/OSC input, or stop (`None`).
    fn set_learning(&mut self, id: Option<&str>);
    fn active_layer(&self) -> usize;
    /// Apply built-in template `template` to `layer`; the ids it created.
    fn apply_template(&mut self, template: usize, layer: usize) -> Result<Vec<String>, String>;
    /// Write one scope's bindings to disk now; what was saved (`"global"` or
    /// the preset name).
    fn save_bindings(&mut self, scope: BindingScope) -> Result<String, String>;
}

/// The audio-input slice of the app. Every setter applies live and persists.
pub trait AudioPort {
    /// Input devices (a cached list, rescanned in the background).
    fn audio_devices(&mut self) -> Vec<String>;
    fn audio_state(&self) -> AudioState;
    /// Listen to `device`, or the default input (`None`).
    fn switch_audio_device(&mut self, device: Option<&str>);
    fn input_trim_db(&self) -> f32;
    fn set_input_trim(&mut self, db: f32);
    fn band_scale(&self) -> BandScale;
    fn set_band_scale(&mut self, scale: BandScale);
    fn sync_offset_ms(&self) -> f32;
    fn set_sync_offset(&mut self, ms: f32);
    fn structure_tuning(&self) -> StructureConfig;
    fn set_structure_tuning(&mut self, tuning: StructureConfig);
}

/// The MIDI and OSC mapping slice of the app.
pub trait MappingPort {
    /// MIDI input ports seen at the last scan.
    fn midi_ports(&self) -> Vec<String>;
    fn midi_port(&self) -> Option<String>;
    /// Open `port`, or disconnect (`None`).
    fn midi_connect(&mut self, port: Option<&str>) -> Result<(), String>;
    fn mapping_enabled(&self, proto: Proto) -> bool;
    fn set_mapping_enabled(&mut self, proto: Proto, on: bool);
    /// Every mapping of `proto`.
    fn mappings(&self, proto: Proto) -> Vec<MappingRow>;
    fn mapping_learning(&self, proto: Proto) -> Option<MapTarget>;
    /// Learn `target` from the next message of `proto`, or stop (`None`).
    fn set_mapping_learn(&mut self, proto: Proto, target: Option<MapTarget>);
    /// Remove the mapping for `target`; whether there was one.
    fn clear_mapping(&mut self, proto: Proto, target: &MapTarget) -> bool;
}

/// The OSC network slice of the app: where it listens, where it sends. Every
/// setter applies live and persists, as the Setup page does.
pub trait OscNetPort {
    fn osc_settings(&self) -> OscSettings;
    fn set_osc_enabled(&mut self, on: bool);
    /// Listen on `port`; restarts the listener.
    fn set_osc_rx_port(&mut self, port: u16);
    /// Receive from other devices, or this computer only; restarts the listener.
    fn set_osc_rx_lan(&mut self, lan: bool);
    fn set_osc_tx_enabled(&mut self, on: bool);
    fn set_osc_tx_target(&mut self, host: &str, port: u16);
    fn set_osc_tx_rate(&mut self, hz: u32);
    fn set_osc_tx_prefix(&mut self, prefix: &str);
}
