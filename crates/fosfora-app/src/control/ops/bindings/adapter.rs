//! The binding ports implemented for [`App`]: the only code in this domain
//! that reaches into `App` internals.

use super::port::{
    AudioPort, AudioState, BindingEdit, BindingsPort, MapTarget, MappingPort, MappingRow,
    MappingSource, Proto, SourceRow, TargetRow,
};
use crate::app::App;
use crate::audio::StructureConfig;
use crate::bindings::types::{Binding, BindingScope, LearnField, LearnState};
use crate::midi::types::LearnTarget;
use crate::osc::types::OscLearnTarget;
use crate::settings::BandScale;

impl BindingsPort for App {
    fn bindings(&self) -> Vec<Binding> {
        self.binding_bus.bindings.clone()
    }

    fn binding_io(&self, id: &str) -> (Option<f32>, Option<f32>) {
        self.binding_bus
            .runtime(id)
            .map_or((None, None), |r| (r.last_input, r.last_output))
    }

    fn sources(&self) -> Vec<SourceRow> {
        let snap = &self.binding_bus.last_snapshot;
        crate::ui::panels::binding_helpers::all_source_groups(snap)
            .into_iter()
            .flat_map(|g| {
                let label = g.label;
                g.keys.into_iter().map(move |key| SourceRow {
                    value: snap.get(key.as_str()).map(|(v, _)| *v),
                    group: label.clone(),
                    key,
                })
            })
            .collect()
    }

    fn targets(&self) -> Vec<TargetRow> {
        use crate::ui::panels::binding_helpers::{BindingPanelInfo, build_target_options};
        let info = BindingPanelInfo {
            layers: super::actions::layer_param_infos(&self.layer_stack, &self.effect_loader),
            active_layer: self.layer_stack.active_layer,
            layer_count: self.layer_stack.layers.len(),
            preset_name: String::new(),
        };
        build_target_options(&info)
            .into_iter()
            .map(|o| TargetRow {
                target: o.id.to_string(),
                label: o.label,
                group: o.group.into_owned(),
            })
            .collect()
    }

    fn add_binding(&mut self, scope: BindingScope, edit: BindingEdit) -> String {
        let id = self
            .binding_bus
            .add_binding(edit.source.clone(), edit.target.clone(), scope);
        let _ = BindingsPort::edit_binding(self, &id, edit);
        id
    }

    fn edit_binding(&mut self, id: &str, edit: BindingEdit) -> Result<(), String> {
        if self.binding_bus.get_binding(id).is_none() {
            return Err(format!("no binding {id:?}"));
        }
        if let Some(b) = self.binding_bus.get_binding_mut(id) {
            b.source = edit.source;
            b.target = edit.target;
            b.name = edit.name;
            b.enabled = edit.enabled;
            b.transforms = edit.transforms;
        }
        Ok(())
    }

    fn remove_binding(&mut self, id: &str) {
        self.binding_bus.remove_binding(id);
    }

    fn duplicate_binding(&mut self, id: &str) -> Option<String> {
        self.binding_bus.duplicate_binding(id)
    }

    fn learning(&self) -> Option<String> {
        self.binding_bus
            .learn_target
            .as_ref()
            .filter(|l| l.field == LearnField::Source)
            .map(|l| l.binding_id.clone())
    }

    fn set_learning(&mut self, id: Option<&str>) {
        self.binding_bus.learn_target = id.map(|id| LearnState {
            binding_id: id.to_owned(),
            field: LearnField::Source,
        });
    }

    fn active_layer(&self) -> usize {
        self.layer_stack.active_layer
    }

    fn apply_template(&mut self, template: usize, layer: usize) -> Result<Vec<String>, String> {
        self.apply_binding_template(template, layer)
    }

    fn save_bindings(&mut self, scope: BindingScope) -> Result<String, String> {
        self.try_save_bindings(scope)
    }
}

impl AudioPort for App {
    fn audio_devices(&mut self) -> Vec<String> {
        self.audio.list_devices()
    }

    fn audio_state(&self) -> AudioState {
        AudioState {
            device: self.audio.device_name.clone(),
            active: self.audio.active,
            error: self.audio.last_error.clone(),
            saved: self.settings.audio_device.clone(),
        }
    }

    fn switch_audio_device(&mut self, device: Option<&str>) {
        self.apply_audio_device(device.map(str::to_owned));
    }

    fn input_trim_db(&self) -> f32 {
        self.audio.input_trim_db()
    }

    fn set_input_trim(&mut self, db: f32) {
        self.apply_input_trim(db);
    }

    fn band_scale(&self) -> BandScale {
        self.settings.band_scale
    }

    fn set_band_scale(&mut self, scale: BandScale) {
        self.apply_band_scale(scale);
    }

    fn sync_offset_ms(&self) -> f32 {
        self.audio
            .tempo()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sync_offset_ms()
    }

    fn set_sync_offset(&mut self, ms: f32) {
        self.apply_sync_offset(ms);
    }

    fn structure_tuning(&self) -> StructureConfig {
        *self
            .audio
            .tuning()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn set_structure_tuning(&mut self, tuning: StructureConfig) {
        self.apply_structure_tuning(tuning);
    }
}

impl MappingPort for App {
    fn midi_ports(&self) -> Vec<String> {
        self.midi.available_ports.clone()
    }

    fn midi_port(&self) -> Option<String> {
        self.midi.connected_port().map(str::to_owned)
    }

    fn midi_connect(&mut self, port: Option<&str>) -> Result<(), String> {
        let Some(p) = port else {
            self.midi.disconnect();
            return Ok(());
        };
        self.midi.connect(p);
        if self.midi.connected_port() == Some(p) {
            Ok(())
        } else {
            Err(format!("could not open MIDI port {p:?} (the log says why)"))
        }
    }

    fn mapping_enabled(&self, proto: Proto) -> bool {
        match proto {
            Proto::Midi => self.midi.config.enabled,
            Proto::Osc => self.osc.config.enabled,
        }
    }

    fn set_mapping_enabled(&mut self, proto: Proto, on: bool) {
        match proto {
            Proto::Midi => self.midi.set_enabled(on),
            Proto::Osc => self.osc.set_enabled(on),
        }
    }

    fn mappings(&self, proto: Proto) -> Vec<MappingRow> {
        let row = |target, source| MappingRow { target, source };
        let mut rows: Vec<MappingRow> = match proto {
            Proto::Midi => {
                let c = &self.midi.config;
                let params = c
                    .params
                    .iter()
                    .map(|(p, m)| row(MapTarget::Param(p.clone()), MappingSource::Midi(m.clone())));
                let triggers = c
                    .triggers
                    .iter()
                    .map(|(a, m)| row(MapTarget::Trigger(*a), MappingSource::Midi(m.clone())));
                params.chain(triggers).collect()
            }
            Proto::Osc => {
                let c = &self.osc.config;
                let params = c.params.iter().map(|(p, m)| {
                    row(
                        MapTarget::Param(p.clone()),
                        MappingSource::Osc(m.address.clone()),
                    )
                });
                let triggers = c.triggers.iter().map(|(a, m)| {
                    row(
                        MapTarget::Trigger(*a),
                        MappingSource::Osc(m.address.clone()),
                    )
                });
                params.chain(triggers).collect()
            }
        };
        rows.sort_by_key(|r| super::target_key(&r.target));
        rows
    }

    fn mapping_learning(&self, proto: Proto) -> Option<MapTarget> {
        match proto {
            Proto::Midi => self.midi.learn_target.clone().map(|t| match t {
                LearnTarget::Param(p) => MapTarget::Param(p),
                LearnTarget::Trigger(a) => MapTarget::Trigger(a),
            }),
            Proto::Osc => self.osc.learn_target.clone().map(|t| match t {
                OscLearnTarget::Param(p) => MapTarget::Param(p),
                OscLearnTarget::Trigger(a) => MapTarget::Trigger(a),
            }),
        }
    }

    fn set_mapping_learn(&mut self, proto: Proto, target: Option<MapTarget>) {
        match (proto, target) {
            (Proto::Midi, Some(MapTarget::Param(p))) => {
                self.midi.start_learn(LearnTarget::Param(p));
            }
            (Proto::Midi, Some(MapTarget::Trigger(a))) => {
                self.midi.start_learn(LearnTarget::Trigger(a));
            }
            (Proto::Midi, None) => self.midi.cancel_learn(),
            (Proto::Osc, Some(MapTarget::Param(p))) => {
                self.osc.start_learn(OscLearnTarget::Param(p));
            }
            (Proto::Osc, Some(MapTarget::Trigger(a))) => {
                self.osc.start_learn(OscLearnTarget::Trigger(a));
            }
            (Proto::Osc, None) => self.osc.cancel_learn(),
        }
    }

    fn clear_mapping(&mut self, proto: Proto, target: &MapTarget) -> bool {
        match (proto, target) {
            (Proto::Midi, MapTarget::Param(p)) => {
                let had = self.midi.config.params.contains_key(p);
                self.midi.clear_param_mapping(p);
                had
            }
            (Proto::Midi, MapTarget::Trigger(a)) => {
                let had = self.midi.config.triggers.contains_key(a);
                self.midi.clear_trigger_mapping(*a);
                had
            }
            (Proto::Osc, MapTarget::Param(p)) => {
                let had = self.osc.config.params.contains_key(p);
                self.osc.clear_param_mapping(p);
                had
            }
            (Proto::Osc, MapTarget::Trigger(a)) => {
                let had = self.osc.config.triggers.contains_key(a);
                self.osc.clear_trigger_mapping(*a);
                had
            }
        }
    }
}
