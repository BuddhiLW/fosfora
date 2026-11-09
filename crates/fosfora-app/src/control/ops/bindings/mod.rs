//! Binding ops: the Binding Matrix (bindings, sources, targets, templates,
//! LFOs, saving), the audio input, and MIDI/OSC mappings.
//!
//! - `port`: the three role-sized ports; `adapter`: their `App` impls.
//! - `pipeline`: Promote (args -> targets, scopes, triggers, picks) and the
//!   pure Pipeline (transform and tuning patches, LFO keys, JSON shapes).
//! - `matrix`, `input`, `mapping`: Boundary op bodies, one port call each.
//! - `actions`: `impl App` methods the UI signal handlers share with the ops.

pub mod actions;
mod adapter;
mod input;
mod mapping;
mod matrix;
pub mod pipeline;
pub mod port;
#[cfg(test)]
mod tests;

pub use self::pipeline::target_key;
pub use self::port::{AudioPort, BindingsPort, MappingPort};
use crate::control::{CljrsNs, ControlAddon, OpSpec};

pub struct Addon;

/// Everything a context needs for the binding ops.
pub trait BindingsHost: BindingsPort + AudioPort + MappingPort {}
impl<T: BindingsPort + AudioPort + MappingPort> BindingsHost for T {}

const PICK: &str = "{:id str | :index int | :name str}";

impl<C: BindingsHost + 'static> ControlAddon<C> for Addon {
    fn id(&self) -> &'static str {
        "bindings"
    }

    #[allow(clippy::too_many_lines)]
    fn ops(&self) -> Vec<OpSpec<C>> {
        vec![
            // Binding Matrix
            OpSpec::new(
                "bindings/list",
                "{:scope preset|global?, :filter str?}",
                "Every binding: index id name label enabled scope source target transforms input output.",
                matrix::list::<C>,
            ),
            OpSpec::new(
                "bindings/get",
                PICK,
                "One binding, with its last input and output.",
                matrix::get::<C>,
            ),
            OpSpec::new(
                "bindings/sources",
                "{:filter str?}",
                "Bindable sources this frame (audio, MIDI, OSC, ws.*, LFOs): group key value.",
                matrix::sources::<C>,
            ),
            OpSpec::new(
                "bindings/targets",
                "{:filter str?}",
                "Targets the matrix offers for the current stack: target label group.",
                matrix::targets::<C>,
            ),
            OpSpec::new(
                "bindings/add",
                "{:source str?, :target str?, :scope preset|global?, :name str?, :enabled bool?, :transforms [..]?, :curve str?, :smoothing num?, :range [lo hi]?, :invert bool?}",
                "Add a binding (preset scope by default); the new binding.",
                matrix::add::<C>,
            ),
            OpSpec::new(
                "bindings/update",
                "{:id|:index|:name, + any field of bindings/add except :scope}",
                "Edit a binding's source, target, name, enabled flag or transforms.",
                matrix::update::<C>,
            ),
            OpSpec::new(
                "bindings/enable",
                "{:id|:index|:name, :on bool?}",
                "Turn a binding on or off (toggles without :on).",
                matrix::enable::<C>,
            ),
            OpSpec::new(
                "bindings/remove",
                PICK,
                "Remove a binding; the binding removed.",
                matrix::remove::<C>,
            ),
            OpSpec::new(
                "bindings/duplicate",
                PICK,
                "Copy a binding under a new id; the copy.",
                matrix::duplicate::<C>,
            ),
            OpSpec::new(
                "bindings/learn",
                "{:id|:index|:name | :cancel true}",
                "Learn a binding's source from the next MIDI/OSC input, or stop learning.",
                matrix::learn::<C>,
            ),
            OpSpec::new(
                "bindings/templates",
                "{}",
                "The built-in binding templates.",
                matrix::templates::<C>,
            ),
            OpSpec::new(
                "bindings/apply-template",
                "{:index int | :name str, :layer int?}",
                "Apply a template to a layer (default the active one); the bindings it added.",
                matrix::apply_template::<C>,
            ),
            OpSpec::new(
                "bindings/save",
                "{:scope preset|global}",
                "Write the global bindings, or the loaded user preset's, to disk now.",
                matrix::save::<C>,
            ),
            OpSpec::new(
                "bindings/lfos",
                "{:shape str?, :division str?, :degrees int?}",
                "The LFO sources, or the key for one shape/division/phase.",
                matrix::lfos::<C>,
            ),
            // Audio input
            OpSpec::new(
                "bindings/audio",
                "{}",
                "The audio input: device active error saved devices input-trim-db band-scale sync-offset-ms.",
                input::audio::<C>,
            ),
            OpSpec::new(
                "bindings/audio-device",
                "{:index int | :name str | :default true}",
                "Listen to another input device (saved).",
                input::device::<C>,
            ),
            OpSpec::new(
                "bindings/input-trim",
                "{:db num?}",
                "Read or set the input trim in dB (saved).",
                input::input_trim::<C>,
            ),
            OpSpec::new(
                "bindings/band-scale",
                "{:scale db|legacy?}",
                "Read or set the band scaling (saved; reopens the input).",
                input::band_scale::<C>,
            ),
            OpSpec::new(
                "bindings/sync-offset",
                "{:ms num?}",
                "Read or set the A/V sync offset in ms (saved).",
                input::sync_offset::<C>,
            ),
            OpSpec::new(
                "bindings/structure-tuning",
                "{:<field> num ..., :reset bool?}",
                "Read or change the build-up/drop detector tuning (saved).",
                input::structure_tuning::<C>,
            ),
            // MIDI and OSC mappings
            OpSpec::new(
                "bindings/midi",
                "{}",
                "MIDI: enabled, port, ports, learning, mappings.",
                mapping::midi::<C>,
            ),
            OpSpec::new(
                "bindings/midi-connect",
                "{:index int | :name str | :disconnect true}",
                "Open a MIDI input port, or disconnect.",
                mapping::midi_connect::<C>,
            ),
            OpSpec::new(
                "bindings/midi-enable",
                "{:on bool}",
                "Turn MIDI input on or off.",
                mapping::midi_enable::<C>,
            ),
            OpSpec::new(
                "bindings/midi-learn",
                "{:param str | :trigger str | :cancel true}",
                "Map the next MIDI control to a param or trigger, or stop learning.",
                mapping::midi_learn::<C>,
            ),
            OpSpec::new(
                "bindings/midi-clear",
                "{:param str | :trigger str}",
                "Remove a MIDI mapping.",
                mapping::midi_clear::<C>,
            ),
            OpSpec::new(
                "bindings/osc",
                "{}",
                "OSC: enabled, learning, mappings.",
                mapping::osc::<C>,
            ),
            OpSpec::new(
                "bindings/osc-enable",
                "{:on bool}",
                "Turn the OSC listener on or off.",
                mapping::osc_enable::<C>,
            ),
            OpSpec::new(
                "bindings/osc-learn",
                "{:param str | :trigger str | :cancel true}",
                "Map the next OSC address to a param or trigger, or stop learning.",
                mapping::osc_learn::<C>,
            ),
            OpSpec::new(
                "bindings/osc-clear",
                "{:param str | :trigger str}",
                "Remove an OSC mapping.",
                mapping::osc_clear::<C>,
            ),
        ]
    }

    fn ui_signals(&self) -> &'static [(&'static str, &'static str)] {
        &[
            ("set_band_scale", "bindings/band-scale"),
            ("set_input_trim", "bindings/input-trim"),
            ("switch_audio_device", "bindings/audio-device"),
            ("set_sync_offset", "bindings/sync-offset"),
            ("structure_tuning_dirty", "bindings/structure-tuning"),
        ]
    }

    fn cljrs(&self) -> Option<CljrsNs> {
        Some(CljrsNs {
            ns: "fosfora.bindings",
            alias: "bind",
            src: include_str!("bindings.cljrs"),
        })
    }
}
