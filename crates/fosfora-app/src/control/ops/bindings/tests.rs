//! Unit + property tests for the binding ops, against an in-memory stub of
//! the three ports. The integration leg (a live app over nREPL) is
//! `tests/repl_ops.rs`.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::Addon;
use super::pipeline::*;
use super::port::*;
use crate::audio::StructureConfig;
use crate::bindings::lfo;
use crate::bindings::templates::builtin_templates;
use crate::bindings::types::{Binding, BindingScope, BindingTarget, TransformDef};
use crate::control::testkit::cases;
use crate::control::{ControlAddon, OpHost, Registry, run};
use crate::midi::mapping::MidiMapping;
use crate::midi::types::{MidiMsgType, TriggerAction};
use crate::settings::BandScale;

#[derive(Default)]
struct Stub {
    bindings: Vec<Binding>,
    next_id: u32,
    learning: Option<String>,
    layers: usize,
    active_layer: usize,
    current_preset: Option<String>,
    saved: Vec<String>,
    devices: Vec<String>,
    device: Option<String>,
    trim: f32,
    band: BandScale,
    sync: f32,
    tuning: StructureConfig,
    ports: Vec<String>,
    port: Option<String>,
    enabled: [bool; 2],
    maps: [BTreeMap<String, MapTarget>; 2],
    map_learning: [Option<MapTarget>; 2],
    osc_net: Option<OscSettings>,
    registry: Registry<Stub>,
}

fn osc_defaults() -> OscSettings {
    OscSettings {
        enabled: true,
        rx_port: 9000,
        rx_lan: false,
        active: false,
        last_address: None,
        tx_enabled: false,
        tx_host: "127.0.0.1".into(),
        tx_port: 9001,
        tx_rate_hz: 30,
        tx_prefix: "fosfora".into(),
    }
}

fn slot(p: Proto) -> usize {
    match p {
        Proto::Midi => 0,
        Proto::Osc => 1,
    }
}

impl Stub {
    fn new() -> Self {
        let mut s = Self {
            layers: 2,
            devices: vec!["Built-in".into(), "USB Audio".into()],
            ports: vec!["MPD218".into(), "nanoKONTROL".into()],
            enabled: [true, true],
            osc_net: Some(osc_defaults()),
            ..Self::default()
        };
        s.registry.register(&Addon as &dyn ControlAddon<Stub>);
        s
    }

    fn call(&mut self, op: &str, args: Value) -> Result<Value, String> {
        run(self, op, &args)
    }

    fn with_bindings(n: usize) -> Self {
        let mut s = Self::new();
        for i in 0..n {
            s.call(
                "bindings/add",
                json!({"source": format!("audio.band{i}"), "target": format!("layer.{i}.opacity")}),
            )
            .unwrap();
        }
        s
    }
}

impl OpHost for Stub {
    fn registry(&self) -> &Registry<Self> {
        &self.registry
    }
}

impl BindingsPort for Stub {
    fn bindings(&self) -> Vec<Binding> {
        self.bindings.clone()
    }
    fn binding_io(&self, _: &str) -> (Option<f32>, Option<f32>) {
        (Some(0.5), Some(0.25))
    }
    fn sources(&self) -> Vec<SourceRow> {
        ["audio.kick", "audio.rms", "lfo.sine.bar"]
            .iter()
            .map(|k| SourceRow {
                group: "Audio".into(),
                key: (*k).into(),
                value: Some(0.1),
            })
            .collect()
    }
    fn targets(&self) -> Vec<TargetRow> {
        (0..self.layers)
            .map(|l| TargetRow {
                target: format!("layer.{l}.opacity"),
                label: "opacity".into(),
                group: "Layers".into(),
            })
            .collect()
    }
    fn add_binding(&mut self, scope: BindingScope, e: BindingEdit) -> String {
        self.next_id += 1;
        let id = format!("b_{:03}", self.next_id);
        self.bindings.push(Binding {
            id: id.clone(),
            name: e.name,
            enabled: e.enabled,
            scope,
            source: e.source,
            target: e.target,
            transforms: e.transforms,
        });
        id
    }
    fn edit_binding(&mut self, id: &str, e: BindingEdit) -> Result<(), String> {
        let b = self
            .bindings
            .iter_mut()
            .find(|b| b.id == id)
            .ok_or("no binding")?;
        b.source = e.source;
        b.target = e.target;
        b.name = e.name;
        b.enabled = e.enabled;
        b.transforms = e.transforms;
        Ok(())
    }
    fn remove_binding(&mut self, id: &str) {
        self.bindings.retain(|b| b.id != id);
    }
    fn duplicate_binding(&mut self, id: &str) -> Option<String> {
        let b = self.bindings.iter().find(|b| b.id == id)?.clone();
        Some(self.add_binding(b.scope.clone(), BindingEdit::of(&b)))
    }
    fn learning(&self) -> Option<String> {
        self.learning.clone()
    }
    fn set_learning(&mut self, id: Option<&str>) {
        self.learning = id.map(str::to_owned);
    }
    fn active_layer(&self) -> usize {
        self.active_layer
    }
    fn apply_template(&mut self, template: usize, layer: usize) -> Result<Vec<String>, String> {
        if template >= builtin_templates().len() || layer >= self.layers {
            return Err("out of range".into());
        }
        let edit = BindingEdit {
            source: "audio.rms".into(),
            target: BindingTarget::from(format!("layer.{layer}.opacity")),
            ..BindingEdit::default()
        };
        Ok(vec![self.add_binding(BindingScope::Preset, edit)])
    }
    fn save_bindings(&mut self, scope: BindingScope) -> Result<String, String> {
        let what = match scope {
            BindingScope::Global => "global".to_owned(),
            BindingScope::Preset => self.current_preset.clone().ok_or("no preset is loaded")?,
        };
        self.saved.push(what.clone());
        Ok(what)
    }
}

impl AudioPort for Stub {
    fn audio_devices(&mut self) -> Vec<String> {
        self.devices.clone()
    }
    fn audio_state(&self) -> AudioState {
        AudioState {
            device: self.device.clone().unwrap_or_else(|| "default".into()),
            active: true,
            error: None,
            saved: self.device.clone(),
        }
    }
    fn switch_audio_device(&mut self, device: Option<&str>) {
        self.device = device.map(str::to_owned);
    }
    fn input_trim_db(&self) -> f32 {
        self.trim
    }
    fn set_input_trim(&mut self, db: f32) {
        self.trim = db;
    }
    fn band_scale(&self) -> BandScale {
        self.band
    }
    fn set_band_scale(&mut self, scale: BandScale) {
        self.band = scale;
    }
    fn sync_offset_ms(&self) -> f32 {
        self.sync
    }
    fn set_sync_offset(&mut self, ms: f32) {
        self.sync = ms;
    }
    fn structure_tuning(&self) -> StructureConfig {
        self.tuning
    }
    fn set_structure_tuning(&mut self, tuning: StructureConfig) {
        self.tuning = tuning;
    }
}

impl MappingPort for Stub {
    fn midi_ports(&self) -> Vec<String> {
        self.ports.clone()
    }
    fn midi_port(&self) -> Option<String> {
        self.port.clone()
    }
    fn midi_connect(&mut self, port: Option<&str>) -> Result<(), String> {
        self.port = port.map(str::to_owned);
        Ok(())
    }
    fn mapping_enabled(&self, proto: Proto) -> bool {
        self.enabled[slot(proto)]
    }
    fn set_mapping_enabled(&mut self, proto: Proto, on: bool) {
        self.enabled[slot(proto)] = on;
    }
    fn mappings(&self, proto: Proto) -> Vec<MappingRow> {
        self.maps[slot(proto)]
            .values()
            .map(|t| MappingRow {
                target: t.clone(),
                source: match proto {
                    Proto::Midi => {
                        MappingSource::Midi(MidiMapping::from_learn(42, 0, MidiMsgType::Cc))
                    }
                    Proto::Osc => MappingSource::Osc("/fader/1".into()),
                },
            })
            .collect()
    }
    fn mapping_learning(&self, proto: Proto) -> Option<MapTarget> {
        self.map_learning[slot(proto)].clone()
    }
    fn set_mapping_learn(&mut self, proto: Proto, target: Option<MapTarget>) {
        self.map_learning[slot(proto)] = target;
    }
    fn clear_mapping(&mut self, proto: Proto, target: &MapTarget) -> bool {
        self.maps[slot(proto)].remove(&target_key(target)).is_some()
    }
}

impl Stub {
    fn net(&mut self) -> &mut OscSettings {
        self.osc_net.as_mut().unwrap()
    }
}

impl OscNetPort for Stub {
    fn osc_settings(&self) -> OscSettings {
        let mut s = self.osc_net.clone().unwrap();
        s.enabled = self.enabled[1];
        s
    }
    fn set_osc_enabled(&mut self, on: bool) {
        self.enabled[1] = on;
    }
    fn set_osc_rx_port(&mut self, port: u16) {
        self.net().rx_port = port;
    }
    fn set_osc_rx_lan(&mut self, lan: bool) {
        self.net().rx_lan = lan;
    }
    fn set_osc_tx_enabled(&mut self, on: bool) {
        self.net().tx_enabled = on;
    }
    fn set_osc_tx_target(&mut self, host: &str, port: u16) {
        self.net().tx_host = host.into();
        self.net().tx_port = port;
    }
    fn set_osc_tx_rate(&mut self, hz: u32) {
        self.net().tx_rate_hz = hz;
    }
    fn set_osc_tx_prefix(&mut self, prefix: &str) {
        self.net().tx_prefix = prefix.into();
    }
}

// ── Unit: registration ─────────────────────────────────────────────────────

#[test]
fn every_op_is_namespaced_and_every_claimed_signal_names_a_registered_op() {
    let s = Stub::new();
    let catalog = s.registry.catalog();
    let names: Vec<&str> = catalog
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap())
        .collect();
    assert!(
        names.iter().all(|n| n.starts_with("bindings/")),
        "{names:?}"
    );
    for (signal, op) in ControlAddon::<Stub>::ui_signals(&Addon) {
        assert!(
            names.contains(op),
            "{signal} claims {op}, which is not registered"
        );
    }
    for signal in [
        "set_band_scale",
        "set_input_trim",
        "switch_audio_device",
        "set_sync_offset",
        "structure_tuning_dirty",
    ] {
        assert!(
            ControlAddon::<Stub>::ui_signals(&Addon)
                .iter()
                .any(|(s, _)| *s == signal),
            "{signal} unclaimed"
        );
    }
}

// ── Unit: matrix ───────────────────────────────────────────────────────────

#[test]
fn add_returns_the_binding_and_list_and_get_read_it_back() {
    let mut s = Stub::new();
    let b = s
        .call("bindings/add", json!({"source": "audio.kick", "target": "layer.0.opacity", "smoothing": 0.3, "scope": "global"}))
        .unwrap();
    assert_eq!(b["source"], "audio.kick");
    assert_eq!(b["target"], "layer.0.opacity");
    assert_eq!(b["scope"], "global");
    assert_eq!(
        b["transforms"],
        json!([{"type": "smooth", "factor": 0.3_f32}])
    );
    assert_eq!(b["label"], "audio.kick -> layer.0.opacity");
    let list = s.call("bindings/list", json!({})).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(
        s.call("bindings/list", json!({"scope": "preset"})).unwrap(),
        json!([])
    );
    let got = s.call("bindings/get", json!({"id": b["id"]})).unwrap();
    assert_eq!(got["id"], b["id"]);
    assert_eq!(got["output"], 0.25);
}

#[test]
fn add_defaults_to_an_empty_preset_binding_like_the_plus_button() {
    let mut s = Stub::new();
    let b = s.call("bindings/add", json!({})).unwrap();
    assert_eq!(b["scope"], "preset");
    assert_eq!(b["target"], "");
    assert_eq!(b["enabled"], true);
}

#[test]
fn bad_targets_sources_scopes_and_transforms_are_refused_and_change_nothing() {
    let mut s = Stub::new();
    for bad in [
        json!({"target": "uniform.kik"}),
        json!({"source": "lfo.wobble.bar"}),
        json!({"scope": "everywhere"}),
        json!({"curve": "wiggle"}),
        json!({"smoothing": 3.0}),
        json!({"range": [0.0]}),
        json!({"transforms": [{"type": "quantize", "steps": 1}]}),
        json!({"transforms": "nope"}),
        json!({"source": 7}),
    ] {
        assert!(
            s.call("bindings/add", bad.clone()).is_err(),
            "{bad} accepted"
        );
    }
    assert!(s.bindings.is_empty());
}

#[test]
fn update_edits_only_what_is_given_and_refuses_a_scope_change() {
    let mut s = Stub::with_bindings(1);
    let r = s
        .call(
            "bindings/update",
            json!({"index": 0, "curve": "ease_in", "invert": true, "range": [0.2, 0.8]}),
        )
        .unwrap();
    assert_eq!(r["source"], "audio.band0");
    let kinds: Vec<&str> = s.bindings[0]
        .transforms
        .iter()
        .map(transform_kind)
        .collect();
    assert_eq!(kinds, ["curve", "remap", "invert"]);
    s.call(
        "bindings/update",
        json!({"index": 0, "curve": null, "invert": false}),
    )
    .unwrap();
    assert_eq!(s.bindings[0].transforms.len(), 1);
    assert!(
        s.call("bindings/update", json!({"index": 0, "scope": "global"}))
            .is_err()
    );
    assert!(
        s.call(
            "bindings/update",
            json!({"index": 0, "target": "postfx.nope"})
        )
        .is_err()
    );
    assert_eq!(
        s.bindings[0].target.to_string(),
        "layer.0.opacity",
        "a refused edit changes nothing"
    );
}

#[test]
fn bindings_are_picked_by_id_index_or_label() {
    let mut s = Stub::with_bindings(3);
    assert_eq!(
        s.call("bindings/get", json!({"id": "b_002"})).unwrap()["index"],
        1
    );
    assert_eq!(
        s.call("bindings/get", json!({"name": "band2"})).unwrap()["id"],
        "b_003"
    );
    assert!(
        s.call("bindings/get", json!({"name": "band"})).is_err(),
        "ambiguous"
    );
    assert!(
        s.call("bindings/get", json!({"index": 3})).is_err(),
        "out of range"
    );
    assert!(s.call("bindings/get", json!({"id": "b_999"})).is_err());
    assert!(s.call("bindings/get", json!({})).is_err());
}

#[test]
fn enable_toggles_or_sets_and_remove_and_duplicate_report_the_binding() {
    let mut s = Stub::with_bindings(1);
    assert_eq!(
        s.call("bindings/enable", json!({"index": 0})).unwrap()["enabled"],
        false
    );
    assert_eq!(
        s.call("bindings/enable", json!({"index": 0, "on": true}))
            .unwrap()["enabled"],
        true
    );
    let copy = s.call("bindings/duplicate", json!({"index": 0})).unwrap();
    assert_eq!(copy["source"], "audio.band0");
    assert_ne!(copy["id"], "b_001");
    let gone = s.call("bindings/remove", json!({"id": "b_001"})).unwrap();
    assert_eq!(gone["id"], "b_001");
    assert_eq!(s.bindings.len(), 1);
    assert!(s.call("bindings/remove", json!({"id": "b_001"})).is_err());
}

#[test]
fn learn_marks_the_binding_and_cancel_stops() {
    let mut s = Stub::with_bindings(1);
    let r = s.call("bindings/learn", json!({"index": 0})).unwrap();
    assert_eq!(r["learning"], true);
    assert_eq!(s.learning.as_deref(), Some("b_001"));
    assert_eq!(
        s.call("bindings/learn", json!({"cancel": true})).unwrap(),
        json!({"learning": null})
    );
    assert!(s.learning.is_none());
}

#[test]
fn templates_list_and_apply_to_the_active_or_a_given_layer() {
    let mut s = Stub::new();
    let list = s.call("bindings/templates", json!({})).unwrap();
    assert_eq!(list.as_array().unwrap().len(), builtin_templates().len());
    let name = builtin_templates()[1].name;
    let r = s
        .call("bindings/apply-template", json!({"name": name}))
        .unwrap();
    assert_eq!(r["template"], name);
    assert_eq!(r["layer"], 0);
    assert_eq!(r["added"][0]["target"], "layer.0.opacity");
    let r = s
        .call("bindings/apply-template", json!({"index": 0, "layer": 1}))
        .unwrap();
    assert_eq!(r["layer"], 1);
    assert!(
        s.call("bindings/apply-template", json!({"index": 99}))
            .is_err()
    );
    assert!(
        s.call("bindings/apply-template", json!({"index": 0, "layer": 5}))
            .is_err()
    );
}

#[test]
fn save_writes_one_scope_and_refuses_a_preset_save_with_none_loaded() {
    let mut s = Stub::with_bindings(2);
    let r = s.call("bindings/save", json!({"scope": "global"})).unwrap();
    assert_eq!(r, json!({"scope": "global", "saved": "global", "count": 0}));
    assert!(s.call("bindings/save", json!({"scope": "preset"})).is_err());
    assert!(s.call("bindings/save", json!({})).is_err());
    s.current_preset = Some("Night".into());
    assert_eq!(
        s.call("bindings/save", json!({"scope": "preset"})).unwrap()["count"],
        2
    );
    assert_eq!(s.saved, ["global", "Night"]);
}

#[test]
fn sources_and_targets_filter_case_insensitively() {
    let mut s = Stub::new();
    assert_eq!(
        s.call("bindings/sources", json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let kick = s
        .call("bindings/sources", json!({"filter": "KICK"}))
        .unwrap();
    assert_eq!(
        kick,
        json!([{"group": "Audio", "key": "audio.kick", "value": 0.1_f32}])
    );
    assert_eq!(
        s.call("bindings/targets", json!({"filter": "layer.1"}))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn lfos_list_every_base_source_and_build_offset_keys() {
    let mut s = Stub::new();
    let all = s.call("bindings/lfos", json!({})).unwrap();
    assert_eq!(all.as_array().unwrap().len(), lfo::base_keys().count());
    let r = s
        .call(
            "bindings/lfos",
            json!({"shape": "saw", "division": "beat", "degrees": 90}),
        )
        .unwrap();
    assert_eq!(r["key"], "lfo.saw.beat.90");
    assert!(
        s.call(
            "bindings/lfos",
            json!({"shape": "saw", "division": "beat", "degrees": 360})
        )
        .is_err()
    );
    assert!(
        s.call(
            "bindings/lfos",
            json!({"shape": "wobble", "division": "beat"})
        )
        .is_err()
    );
    assert!(s.call("bindings/lfos", json!({"shape": "saw"})).is_err());
}

// ── Unit: audio ────────────────────────────────────────────────────────────

#[test]
fn audio_setters_take_in_range_values_and_refuse_others() {
    let mut s = Stub::new();
    assert_eq!(
        s.call("bindings/input-trim", json!({"db": 6.0})).unwrap(),
        json!({"db": 6.0})
    );
    assert!(s.call("bindings/input-trim", json!({"db": 99.0})).is_err());
    assert!(
        s.call("bindings/input-trim", json!({"db": "loud"}))
            .is_err()
    );
    assert_eq!(s.trim, 6.0, "a refused call changes nothing");
    assert_eq!(
        s.call("bindings/sync-offset", json!({"ms": -40})).unwrap(),
        json!({"ms": -40.0})
    );
    assert!(s.call("bindings/sync-offset", json!({"ms": 900})).is_err());
    let r = s
        .call("bindings/band-scale", json!({"scale": "legacy"}))
        .unwrap();
    assert_eq!(r["scale"], "legacy");
    assert!(
        s.call("bindings/band-scale", json!({"scale": "mel"}))
            .is_err()
    );
    assert!(s.call("bindings/band-scale", json!({"scale": 1})).is_err());
    assert_eq!(s.band, BandScale::Legacy);
    let all = s.call("bindings/audio", json!({})).unwrap();
    assert_eq!(all["input-trim-db"], 6.0);
    assert_eq!(all["band-scale"], "legacy");
}

#[test]
fn audio_device_switches_by_name_index_or_to_the_default() {
    let mut s = Stub::new();
    assert_eq!(
        s.call("bindings/audio-device", json!({"name": "usb"}))
            .unwrap()["device"],
        "USB Audio"
    );
    s.call("bindings/audio-device", json!({"index": 0}))
        .unwrap();
    assert_eq!(s.device.as_deref(), Some("Built-in"));
    s.call("bindings/audio-device", json!({"default": true}))
        .unwrap();
    assert_eq!(s.device, None);
    assert!(
        s.call("bindings/audio-device", json!({"name": "Theremin"}))
            .is_err()
    );
    assert!(
        s.call("bindings/audio-device", json!({"index": 7}))
            .is_err()
    );
}

#[test]
fn structure_tuning_patches_fields_resets_and_refuses_unknown_ones() {
    let mut s = Stub::new();
    let r = s
        .call(
            "bindings/structure-tuning",
            json!({"drop-loud-jump": 0.1, "buildup_bias": -2.0}),
        )
        .unwrap();
    assert_eq!(r["drop_loud_jump"], json!(0.1_f32));
    assert_eq!(s.tuning.buildup_bias, -2.0);
    assert!(
        s.call("bindings/structure-tuning", json!({"drop_volume": 1.0}))
            .is_err()
    );
    assert!(
        s.call(
            "bindings/structure-tuning",
            json!({"drop_loud_jump": "big"})
        )
        .is_err()
    );
    s.call("bindings/structure-tuning", json!({"reset": true}))
        .unwrap();
    assert_eq!(s.tuning, StructureConfig::default());
}

// ── Unit: MIDI and OSC ─────────────────────────────────────────────────────

#[test]
fn midi_connects_by_name_and_disconnects() {
    let mut s = Stub::new();
    assert_eq!(
        s.call("bindings/midi-connect", json!({"name": "nano"}))
            .unwrap(),
        json!({"port": "nanoKONTROL"})
    );
    assert_eq!(
        s.call("bindings/midi-connect", json!({"disconnect": true}))
            .unwrap(),
        json!({"port": null})
    );
    assert!(
        s.call("bindings/midi-connect", json!({"name": "Launchpad"}))
            .is_err()
    );
    let st = s.call("bindings/midi", json!({})).unwrap();
    assert_eq!(st["ports"], json!(["MPD218", "nanoKONTROL"]));
}

#[test]
fn learn_clear_and_enable_work_per_protocol() {
    let mut s = Stub::new();
    let r = s
        .call("bindings/midi-learn", json!({"trigger": "next-preset"}))
        .unwrap();
    assert_eq!(r, json!({"learning": "trigger:next_preset"}));
    assert_eq!(
        s.call("bindings/osc", json!({})).unwrap()["learning"],
        Value::Null,
        "OSC untouched"
    );
    s.call("bindings/osc-learn", json!({"param": "speed"}))
        .unwrap();
    assert_eq!(s.map_learning[1], Some(MapTarget::Param("speed".into())));
    assert_eq!(
        s.call("bindings/midi-learn", json!({"cancel": true}))
            .unwrap(),
        json!({"learning": null})
    );
    assert!(
        s.call("bindings/midi-learn", json!({"trigger": "explode"}))
            .is_err()
    );
    assert!(s.call("bindings/midi-learn", json!({})).is_err());

    s.maps[0].insert("param:speed".into(), MapTarget::Param("speed".into()));
    let st = s.call("bindings/midi", json!({})).unwrap();
    assert_eq!(st["mappings"][0]["param"], "speed");
    assert_eq!(st["mappings"][0]["midi"]["cc"], 42);
    assert_eq!(
        s.call("bindings/midi-clear", json!({"param": "speed"}))
            .unwrap(),
        json!({"cleared": "param:speed"})
    );
    assert!(
        s.call("bindings/midi-clear", json!({"param": "speed"}))
            .is_err(),
        "nothing left to clear"
    );

    assert_eq!(
        s.call("bindings/osc-enable", json!({"on": false})).unwrap(),
        json!({"enabled": false})
    );
    assert!(s.enabled[0] && !s.enabled[1]);
    assert!(s.call("bindings/midi-enable", json!({})).is_err());
}

// ── Property ───────────────────────────────────────────────────────────────

fn random_transform(rng: &mut crate::control::testkit::Rng) -> TransformDef {
    let f = |rng: &mut crate::control::testkit::Rng| rng.below(100) as f32 / 100.0;
    match rng.below(5) {
        0 => TransformDef::Smooth { factor: f(rng) },
        1 => TransformDef::Invert,
        2 => TransformDef::Curve {
            curve_type: (*rng.pick(crate::bindings::transforms::CURVE_TYPES).unwrap()).to_owned(),
        },
        3 => TransformDef::Remap {
            in_lo: 0.0,
            in_hi: 1.0,
            out_lo: f(rng),
            out_hi: f(rng),
        },
        _ => TransformDef::Gate { threshold: f(rng) },
    }
}

#[test]
fn set_transform_is_idempotent_keeps_one_of_its_kind_and_the_rest_in_order() {
    cases(400, 0x5EED_B001, |rng, _| {
        let list: Vec<TransformDef> = (0..rng.below(6)).map(|_| random_transform(rng)).collect();
        let new = random_transform(rng);
        let kind = transform_kind(&new);
        let to = rng.coin().then_some(new);
        let once = set_transform(&list, kind, to.clone());
        assert_eq!(set_transform(&once, kind, to.clone()), once, "idempotent");
        let of_kind = once.iter().filter(|t| transform_kind(t) == kind).count();
        assert_eq!(of_kind, usize::from(to.is_some()));
        let others = |l: &[TransformDef]| {
            l.iter()
                .filter(|t| transform_kind(t) != kind)
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(others(&once), others(&list));
    });
}

#[test]
fn trigger_names_round_trip_in_every_spelling() {
    for &a in TriggerAction::ALL {
        let key = trigger_key(a);
        assert_eq!(parse_trigger(&key), Some(a));
        assert_eq!(parse_trigger(&key.replace('_', "-")), Some(a));
        assert_eq!(parse_trigger(&format!("{a:?}")), Some(a));
        assert_eq!(
            map_target(&json!({"trigger": key})),
            Ok(MapTarget::Trigger(a))
        );
    }
    assert_eq!(parse_trigger("nope"), None);
}

#[test]
fn lfo_keys_round_trip_through_the_bus_parser() {
    let shapes = ["sine", "triangle", "saw", "square", "sh"];
    let divisions = ["4bar", "2bar", "bar", "half", "beat", "8th", "16th"];
    cases(300, 0x5EED_B002, |rng, _| {
        let shape = *rng.pick(&shapes).unwrap();
        let division = *rng.pick(&divisions).unwrap();
        let degrees = u16::try_from(rng.below(400)).unwrap();
        match lfo_key(shape, division, degrees) {
            Ok(key) => {
                assert!(degrees < 360);
                assert_eq!(lfo::parse(&key).map(|s| s.key()), Some(key.clone()));
                assert!(validate_source(&key).is_ok());
            }
            Err(_) => assert!(degrees >= 360),
        }
    });
}

#[test]
fn patch_tuning_is_identity_on_its_own_values_and_sets_what_it_is_given() {
    let base = StructureConfig::default();
    let own = serde_json::to_value(base).unwrap();
    assert_eq!(patch_tuning(base, &own), Ok(base));
    let fields: Vec<String> = own.as_object().unwrap().keys().cloned().collect();
    cases(300, 0x5EED_B003, |rng, _| {
        let field = rng.pick(&fields).unwrap().clone();
        let v = rng.range_i(-500, 500) as f32 / 100.0;
        let key = if rng.coin() {
            field.replace('_', "-")
        } else {
            field.clone()
        };
        let patched = patch_tuning(base, &json!({ key: v })).unwrap();
        let back = serde_json::to_value(patched).unwrap();
        assert_eq!(back[&field], json!(v));
        assert_eq!(
            patch_tuning(patched, &json!({ field.clone(): v })),
            Ok(patched),
            "idempotent"
        );
    });
}

#[test]
fn patch_transforms_is_total_and_only_returns_valid_chains() {
    let keys = ["curve", "smoothing", "range", "invert", "transforms"];
    let values = [
        json!(null),
        json!(true),
        json!(0.4),
        json!(7),
        json!("ease_out"),
        json!("bogus"),
        json!([0.1, 0.9]),
        json!([1]),
        json!([{"type": "invert"}]),
    ];
    cases(500, 0x5EED_B004, |rng, _| {
        let mut a = serde_json::Map::new();
        for _ in 0..rng.below(4) {
            a.insert(
                (*rng.pick(&keys).unwrap()).to_owned(),
                rng.pick(&values).unwrap().clone(),
            );
        }
        if let Ok(list) = patch_transforms(&[TransformDef::Invert], &Value::Object(a)) {
            assert!(list.iter().all(|t| validate_transform(t).is_ok()));
        }
    });
}

#[test]
fn random_op_sequences_keep_ids_unique_and_results_pointing_at_live_bindings() {
    let ops = [
        "bindings/add",
        "bindings/update",
        "bindings/enable",
        "bindings/remove",
        "bindings/duplicate",
        "bindings/get",
        "bindings/list",
        "bindings/learn",
        "bindings/apply-template",
        "bindings/save",
    ];
    let sources = ["audio.kick", "lfo.sine.bar", "lfo.bad", "midi.MPD.cc.0.1"];
    let targets = ["layer.0.opacity", "uniform.kick", "uniform.kik", ""];
    cases(200, 0x5EED_B005, |rng, _| {
        let mut s = Stub::new();
        for _ in 0..30 {
            let op = *rng.pick(&ops).unwrap();
            let len = s.bindings.len();
            let mut args = match rng.below(3) {
                0 => json!({"index": rng.below(len + 2)}),
                1 => json!({"name": rng.name()}),
                _ => json!({}),
            };
            if rng.coin() {
                args["source"] = json!(*rng.pick(&sources).unwrap());
                args["target"] = json!(*rng.pick(&targets).unwrap());
            }
            let before = s.bindings.len();
            let result = s.call(op, args);
            let mut ids: Vec<&str> = s.bindings.iter().map(|b| b.id.as_str()).collect();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), s.bindings.len(), "ids unique after {op}");
            assert!(
                s.bindings
                    .iter()
                    .all(|b| !matches!(b.target, BindingTarget::Unknown(_))),
                "{op}"
            );
            match (op, &result) {
                ("bindings/add" | "bindings/duplicate" | "bindings/apply-template", Ok(_)) => {
                    assert_eq!(s.bindings.len(), before + 1, "{op}");
                }
                ("bindings/remove", Ok(_)) => assert_eq!(s.bindings.len(), before - 1),
                _ => assert_eq!(s.bindings.len(), before, "{op} changed the list"),
            }
            if let (Ok(v), false) = (&result, op == "bindings/remove") {
                if let Some(id) = v.get("id").and_then(Value::as_str) {
                    assert!(s.bindings.iter().any(|b| b.id == id), "{op} -> {v}");
                }
            }
        }
    });
}

// ── Unit: OSC listener and sender settings ─────────────────────────────────

#[test]
fn osc_config_reads_changes_and_refuses_bad_ports() {
    let mut s = Stub::new();
    let r = s.call("bindings/osc-config", json!({})).unwrap();
    assert_eq!(r["port"], 9000);
    assert_eq!(r["lan"], false);
    let r = s
        .call("bindings/osc-config", json!({"port": 9100, "lan": true}))
        .unwrap();
    assert_eq!(
        (r["port"].as_u64(), r["lan"].as_bool()),
        (Some(9100), Some(true))
    );
    for bad in [
        json!({"port": 53}),
        json!({"port": 70000}),
        json!({"port": "x"}),
    ] {
        assert!(s.call("bindings/osc-config", bad.clone()).is_err(), "{bad}");
    }
    assert_eq!(s.net().rx_port, 9100, "refused calls change nothing");
    assert_eq!(
        s.call("bindings/osc-config", json!({"enabled": false}))
            .unwrap()["enabled"],
        false
    );
    assert!(!s.enabled[1]);
}

#[test]
fn osc_send_reads_changes_and_refuses_bad_values() {
    let mut s = Stub::new();
    let r = s.call("bindings/osc-send", json!({})).unwrap();
    assert_eq!(
        r,
        json!({"enabled": false, "host": "127.0.0.1", "port": 9001, "rate": 30, "prefix": "fosfora"})
    );
    let r = s
        .call(
            "bindings/osc-send",
            json!({"enabled": true, "host": "10.0.0.5", "rate": 60, "prefix": "/Phosphor/"}),
        )
        .unwrap();
    assert_eq!(
        r,
        json!({"enabled": true, "host": "10.0.0.5", "port": 9001, "rate": 60, "prefix": "phosphor"})
    );
    for bad in [
        json!({"host": ""}),
        json!({"host": "a b"}),
        json!({"host": 7}),
        json!({"rate": 0}),
        json!({"rate": 500}),
        json!({"prefix": "osc"}),
        json!({"prefix": 1}),
        json!({"port": 1}),
        json!({"port": 9002, "rate": 0}),
    ] {
        assert!(s.call("bindings/osc-send", bad.clone()).is_err(), "{bad}");
    }
    assert_eq!(s.net().tx_port, 9001, "a refused call changes nothing");
    assert_eq!(s.net().tx_rate_hz, 60);
}

#[test]
fn osc_send_change_from_args_is_total_and_keeps_values_in_range() {
    use super::osc_net::{OscSendChange, TX_RATE_RANGE};
    let values = [
        json!(null),
        json!(true),
        json!(0),
        json!(30),
        json!(9001),
        json!(70000),
        json!("fosfora"),
        json!("host.local"),
        json!(""),
    ];
    let keys = ["enabled", "host", "port", "rate", "prefix"];
    cases(400, 0x5EED_B006, |rng, _| {
        let mut a = serde_json::Map::new();
        for _ in 0..rng.below(4) {
            a.insert(
                (*rng.pick(&keys).unwrap()).to_owned(),
                rng.pick(&values).unwrap().clone(),
            );
        }
        if let Ok(c) = OscSendChange::from_args(&Value::Object(a)) {
            assert!(c.rate.is_none_or(|r| TX_RATE_RANGE.contains(&r)));
            assert!(c.port.is_none_or(|p| p >= 1024));
            assert!(c.host.as_deref().is_none_or(|h| !h.is_empty()));
        }
    });
}
