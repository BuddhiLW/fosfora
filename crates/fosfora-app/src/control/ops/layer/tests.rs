//! Unit + property tests for the layer ops, against an in-memory app.
//! The integration leg (a live app over nREPL) is `tests/repl_ops.rs`.

use serde_json::{Value, json};

use super::*;
use crate::control::testkit::cases;
use crate::control::{ControlAddon, OpHost, Registry, run};
use crate::gpu::layer::{adjusted_active_after_move, adjusted_active_after_remove};
use crate::settings::RtmpStream;

const MAX: usize = crate::bindings::catalog::MAX_LAYERS;

#[derive(Clone)]
struct StubLayer {
    view: LayerView,
    params: Vec<(ParamDef, ParamValue)>,
    transport: Option<TransportView>,
}

struct Stub {
    layers: Vec<StubLayer>,
    active: usize,
    effects: Vec<EffectView>,
    cameras: Option<WebcamList>,
    streams: Vec<RtmpStream>,
    pp: PostProcessDef,
    pp_prev: Option<PostProcessDef>,
    editor: Option<String>,
    registry: Registry<Stub>,
}

fn defs() -> Vec<ParamDef> {
    vec![
        ParamDef::Float { name: "speed".into(), default: 1.0, min: 0.0, max: 10.0 },
        ParamDef::Color { name: "tint".into(), default: [1.0, 1.0, 1.0, 1.0] },
        ParamDef::Bool { name: "glow".into(), default: false },
        ParamDef::Point2D { name: "center".into(), default: [0.0, 0.0], min: [-1.0, -1.0], max: [1.0, 1.0] },
    ]
}

fn effect_view(name: &str, category: &str, builtin: bool) -> EffectView {
    EffectView { name: name.into(), category: category.into(), builtin, favorite: false, hidden: false }
}

fn effect_layer(name: &str, effect: Option<&str>) -> StubLayer {
    StubLayer {
        view: LayerView {
            name: name.into(),
            custom_name: None,
            kind: LayerKind::Effect,
            effect: effect.map(Into::into),
            media: None,
            blend: BlendMode::Normal,
            opacity: 1.0,
            displace: 0.35,
            enabled: true,
            locked: false,
            pinned: false,
            shader_error: None,
        },
        params: if effect.is_some() { defs().into_iter().map(|d| { let v = d.default_value(); (d, v) }).collect() } else { Vec::new() },
        transport: None,
    }
}

fn media_layer(name: &str, file: &str, live: bool) -> StubLayer {
    let mut l = effect_layer(name, None);
    l.view.kind = if live { LayerKind::Webcam } else { LayerKind::Media };
    l.view.media = Some(file.into());
    l.transport = Some(TransportView {
        playing: true,
        looping: true,
        speed: 1.0,
        direction: PlayDirection::Forward,
        position_secs: 0.0,
        duration_secs: 30.0,
        video: !live,
        live,
    });
    l
}

impl Stub {
    fn new() -> Self {
        let mut s = Self {
            layers: vec![effect_layer("Layer 1", None), effect_layer("Layer 2", Some("Fosfora"))],
            active: 0,
            effects: vec![
                effect_view("Fosfora", "particles", true),
                effect_view("Sumi", "fluid", true),
                effect_view("Tide", "fluid", true),
                effect_view("Mine", "effect", false),
            ],
            cameras: None,
            streams: Vec::new(),
            pp: PostProcessDef::default(),
            pp_prev: None,
            editor: None,
            registry: Registry::default(),
        };
        s.effects[3].hidden = false;
        s.registry.register(&Addon as &dyn ControlAddon<Stub>);
        s
    }

    fn with_cameras(mut self) -> Self {
        self.cameras = Some(WebcamList {
            devices: vec![(0, "Built-in Camera".into()), (2, "USB Cam".into())],
            default: 0,
            ffmpeg: false,
        });
        self
    }

    fn call(&mut self, op: &str, args: Value) -> Result<Value, String> {
        run(self, op, &args)
    }

    fn snapshot(&self) -> Vec<LayerView> {
        self.layers.iter().map(|l| l.view.clone()).collect()
    }
}

impl OpHost for Stub {
    fn registry(&self) -> &Registry<Self> {
        &self.registry
    }
}

impl StackPort for Stub {
    fn layers(&self) -> Vec<LayerView> {
        self.snapshot()
    }
    fn active_layer(&self) -> usize {
        self.active
    }
    fn add_layer(&mut self) -> Result<usize, String> {
        if self.layers.len() >= MAX {
            return Err("the stack is full".into());
        }
        self.layers.push(effect_layer(&format!("Layer {}", self.layers.len() + 1), None));
        self.active = self.layers.len() - 1;
        Ok(self.active)
    }
    fn remove_layer(&mut self, index: usize) -> Result<(), String> {
        if self.layers.len() <= 1 || index >= self.layers.len() {
            return Err("refused".into());
        }
        self.layers.remove(index);
        self.active = adjusted_active_after_remove(self.active, index, self.layers.len());
        Ok(())
    }
    fn clear_layers(&mut self) {
        self.layers = vec![effect_layer("Layer 1", None), effect_layer("Layer 2", Some("Fosfora"))];
        self.active = 0;
    }
    fn move_layer(&mut self, from: usize, to: usize) {
        let n = self.layers.len();
        if from >= n || to >= n || from == to {
            return;
        }
        let l = self.layers.remove(from);
        self.layers.insert(to, l);
        self.active = adjusted_active_after_move(self.active, from, to);
    }
    fn select_layer(&mut self, index: usize) {
        if index < self.layers.len() {
            self.active = index;
        }
    }
    fn edit_layer(&mut self, index: usize, edit: &LayerEdit) -> Result<(), String> {
        let l = &mut self.layers.get_mut(index).ok_or("out of range")?.view;
        if let Some(why) = edit_refusal(index, l.locked, edit) {
            return Err(why);
        }
        l.locked = edit.locked.unwrap_or(l.locked);
        l.pinned = edit.pinned.unwrap_or(l.pinned);
        if let Some(n) = &edit.name {
            l.custom_name = n.clone();
        }
        l.blend = edit.blend.unwrap_or(l.blend);
        l.opacity = edit.opacity.unwrap_or(l.opacity);
        l.displace = edit.displace.unwrap_or(l.displace);
        l.enabled = edit.enabled.unwrap_or(l.enabled);
        Ok(())
    }
}

impl EffectPort for Stub {
    fn effects(&self) -> Vec<EffectView> {
        self.effects.clone()
    }
    fn load_effect(&mut self, layer: usize, effect: usize) -> Result<(), String> {
        let name = self.effects[effect].name.clone();
        let l = self.layers.get_mut(layer).ok_or("out of range")?;
        if l.view.locked {
            return Err("locked".into());
        }
        *l = StubLayer { view: LayerView { effect: Some(name), ..effect_layer("", None).view }, ..effect_layer("", Some("x")) };
        l.view.name = format!("Layer {}", layer + 1);
        Ok(())
    }
    fn set_favorite(&mut self, name: &str, on: bool) {
        if let Some(e) = self.effects.iter_mut().find(|e| e.name == name) {
            e.favorite = on;
        }
    }
    fn create_effect(&mut self, name: &str) -> Result<(), String> {
        self.effects.push(effect_view(name, "effect", false));
        Ok(())
    }
    fn copy_effect(&mut self, _source: usize, name: &str) -> Result<(), String> {
        self.create_effect(name)
    }
    fn delete_effect(&mut self, index: usize) -> Result<String, String> {
        Ok(self.effects.remove(index).name)
    }
    fn open_shader_editor(&mut self, layer: usize) -> Result<String, String> {
        let e = self.layers[layer].view.effect.clone().ok_or("runs no effect")?;
        let path = format!("/shaders/{e}.wgsl");
        self.editor = Some(path.clone());
        Ok(path)
    }
    fn save_shader_editor(&mut self) -> Result<Vec<String>, String> {
        Ok(self.editor.iter().cloned().collect())
    }
}

impl ParamPort for Stub {
    fn param_defs(&self, layer: usize) -> Vec<ParamDef> {
        self.layers[layer].params.iter().map(|(d, _)| d.clone()).collect()
    }
    fn param_value(&self, layer: usize, name: &str) -> Option<ParamValue> {
        self.layers[layer].params.iter().find(|(d, _)| d.name() == name).map(|(_, v)| v.clone())
    }
    fn set_param(&mut self, layer: usize, name: &str, value: ParamValue) {
        if let Some(p) = self.layers[layer].params.iter_mut().find(|(d, _)| d.name() == name) {
            p.1 = value;
        }
    }
    fn reset_params(&mut self, layer: usize, name: Option<&str>) {
        for (d, v) in &mut self.layers[layer].params {
            if name.is_none_or(|n| n == d.name()) {
                *v = d.default_value();
            }
        }
    }
}

impl MediaPort for Stub {
    fn add_media(&mut self, path: &str) -> Result<(), String> {
        if !std::path::Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("mp4")) {
            return Err(format!("no such file: {path}"));
        }
        if self.layers.len() >= MAX {
            return Err("the stack is full".into());
        }
        let n = self.layers.len() + 1;
        self.layers.push(media_layer(&format!("Layer {n}"), path, false));
        self.active = n - 1;
        Ok(())
    }
    fn transport(&self, layer: usize) -> Option<TransportView> {
        self.layers.get(layer)?.transport.clone()
    }
    fn edit_transport(&mut self, layer: usize, edit: &TransportEdit) -> Result<(), String> {
        let t = self.layers[layer].transport.as_mut().ok_or("not media")?;
        if edit.toggle {
            t.playing = !t.playing;
        }
        t.playing = edit.playing.unwrap_or(t.playing);
        t.looping = edit.looping.unwrap_or(t.looping);
        t.speed = edit.speed.unwrap_or(t.speed);
        t.direction = edit.direction.unwrap_or(t.direction);
        t.position_secs = edit.seek_secs.map_or(t.position_secs, |s| s.min(t.duration_secs));
        Ok(())
    }
    fn webcams(&mut self) -> Result<WebcamList, String> {
        self.cameras.clone().ok_or_else(|| "this build has no webcam support".into())
    }
    fn add_webcam(&mut self, device: Option<&str>) -> Result<String, String> {
        let cams = self.webcams()?;
        let name = device.map_or_else(|| cams.devices[0].1.clone(), str::to_owned);
        let n = self.layers.len() + 1;
        self.layers.push(media_layer(&format!("Layer {n}"), &name, true));
        self.active = n - 1;
        Ok(name)
    }
    fn switch_webcam(&mut self, layer: usize, device: u32) -> Result<String, String> {
        let cams = self.webcams()?;
        let name = cams.devices.iter().find(|(i, _)| *i == device).ok_or("gone")?.1.clone();
        self.layers[layer].view.media = Some(name.clone());
        Ok(name)
    }
    fn set_mirror(&mut self, _layer: usize, _mirror: bool) -> Result<(), String> {
        self.webcams().map(|_| ())
    }
    fn set_ffmpeg_webcam(&mut self, on: bool) -> Result<usize, String> {
        let cams = self.cameras.as_mut().ok_or("no webcam support")?;
        cams.ffmpeg = on;
        Ok(cams.devices.len())
    }
    fn rtmp_streams(&self) -> Vec<RtmpStream> {
        self.streams.clone()
    }
    fn set_rtmp_streams(&mut self, streams: Vec<RtmpStream>) -> Result<(), String> {
        self.streams = streams;
        Ok(())
    }
}

impl PostFxPort for Stub {
    fn postprocess(&self) -> PostProcessDef {
        self.pp.clone()
    }
    fn has_previous_postprocess(&self) -> bool {
        self.pp_prev.is_some()
    }
    fn adopt_postprocess(&mut self, _layer: usize) {
        self.replace_postprocess(PostProcessDef { tonemap: "linear".into(), ..PostProcessDef::default() });
    }
    fn restore_previous_postprocess(&mut self) {
        if let Some(prev) = self.pp_prev.clone() {
            self.replace_postprocess(prev);
        }
    }
    fn replace_postprocess(&mut self, def: PostProcessDef) {
        if def != self.pp {
            self.pp_prev = Some(std::mem::replace(&mut self.pp, def));
        }
    }
}

// ── Unit: registration ───────────────────────────────────────────────────

#[test]
fn every_op_is_registered_under_layer_and_every_signal_maps_to_one() {
    let s = Stub::new();
    let names: Vec<String> = s.registry.catalog().as_array().unwrap().iter()
        .map(|o| o["name"].as_str().unwrap().to_owned()).collect();
    assert!(names.iter().all(|n| n.starts_with("layer/")), "{names:?}");
    for (signal, op) in ControlAddon::<Stub>::ui_signals(&Addon) {
        assert!(names.iter().any(|n| n == op), "{signal} -> {op} is not registered");
    }
}

// ── Unit: stack ──────────────────────────────────────────────────────────

#[test]
fn list_and_get_default_to_the_active_layer_and_pick_by_what_a_layer_shows() {
    let mut s = Stub::new();
    let l = s.call("layer/list", json!({})).unwrap();
    assert_eq!(l.as_array().unwrap().len(), 2);
    assert_eq!(l[0]["active"], true);
    assert_eq!(s.call("layer/get", json!({})).unwrap()["index"], 0);
    let r = s.call("layer/get", json!({"name": "fosfora"})).unwrap();
    assert_eq!(r["index"], 1);
    assert_eq!(r["effect"], "Fosfora");
    assert!(s.call("layer/get", json!({"index": 5})).is_err());
    assert!(s.call("layer/get", json!({"name": "nope"})).is_err());
}

#[test]
fn add_selects_the_new_layer_until_the_stack_is_full() {
    let mut s = Stub::new();
    let r = s.call("layer/add", json!({})).unwrap();
    assert_eq!(r["index"], 2);
    assert_eq!(r["active"], true);
    while s.layers.len() < MAX {
        s.call("layer/add", json!({})).unwrap();
    }
    assert!(s.call("layer/add", json!({})).is_err());
    assert_eq!(s.layers.len(), MAX);
}

#[test]
fn remove_reports_what_went_and_never_removes_the_last_layer() {
    let mut s = Stub::new();
    let r = s.call("layer/remove", json!({"index": 1})).unwrap();
    assert_eq!(r["effect"], "Fosfora");
    assert_eq!(s.layers.len(), 1);
    assert!(s.call("layer/remove", json!({"index": 0})).is_err());
    assert!(s.call("layer/remove", json!({"index": 3})).is_err());
    assert_eq!(s.layers.len(), 1);
}

#[test]
fn move_lands_where_asked_and_refuses_a_target_off_the_stack() {
    let mut s = Stub::new();
    let r = s.call("layer/move", json!({"index": 1, "to": 0})).unwrap();
    assert_eq!(r["index"], 0);
    assert_eq!(r["effect"], "Fosfora");
    assert!(s.call("layer/move", json!({"index": 0, "to": 2})).is_err());
    assert!(s.call("layer/move", json!({"index": 0})).is_err());
}

#[test]
fn select_and_clear() {
    let mut s = Stub::new();
    s.call("layer/add", json!({})).unwrap();
    assert_eq!(s.call("layer/select", json!({"name": "Layer 2"})).unwrap()["active"], true);
    assert_eq!(s.active, 1);
    let r = s.call("layer/clear", json!({})).unwrap();
    assert_eq!(r.as_array().unwrap().len(), 2);
}

#[test]
fn set_changes_only_what_is_given_and_reads_back_blend_by_name() {
    let mut s = Stub::new();
    let r = s.call("layer/set", json!({"index": 1, "opacity": 0.4, "blend": "Color Dodge"})).unwrap();
    assert_eq!(r["blend"], "color-dodge");
    assert!((r["opacity"].as_f64().unwrap() - 0.4).abs() < 1e-6);
    assert_eq!(r["enabled"], true);
    let r = s.call("layer/set", json!({"index": 1, "blend": 1})).unwrap();
    assert_eq!(r["blend"], "add");
    assert_eq!(s.call("layer/set", json!({"rename": "Sky"})).unwrap()["name"], "Sky");
    assert_eq!(s.call("layer/set", json!({"rename": null})).unwrap()["name"], "Layer 1");
    let before = s.snapshot();
    let e = s.call("layer/set", json!({"blend": "wipe"})).unwrap_err();
    assert!(e.contains("color-dodge"), "{e}");
    assert!(s.call("layer/set", json!({"opacity": 1.5})).is_err());
    assert!(s.call("layer/set", json!({"opacity": "half"})).is_err());
    assert!(s.call("layer/set", json!({"enabled": 1})).is_err());
    assert!(s.call("layer/set", json!({"blend": 99})).is_err());
    assert_eq!(s.snapshot(), before, "refused edits change nothing");
}

#[test]
fn a_locked_layer_refuses_look_changes_but_takes_pin_rename_and_unlock() {
    let mut s = Stub::new();
    s.call("layer/set", json!({"locked": true})).unwrap();
    assert!(s.call("layer/set", json!({"opacity": 0.2})).is_err());
    assert!(s.call("layer/set", json!({"enabled": false})).is_err());
    s.call("layer/set", json!({"pinned": true, "rename": "Keep"})).unwrap();
    let r = s.call("layer/set", json!({"locked": false, "opacity": 0.2})).unwrap();
    assert_eq!(r["locked"], false);
    assert_eq!(r["pinned"], true);
    assert_eq!(r["name"], "Keep");
}

#[test]
fn blend_modes_list_every_mode_with_its_family() {
    let mut s = Stub::new();
    let r = s.call("layer/blend-modes", json!({})).unwrap();
    let all = r.as_array().unwrap();
    assert_eq!(all.len(), BlendMode::ALL.len());
    assert_eq!(all.iter().filter(|m| m["family"] == "displacement").count(), 3);
}

// ── Unit: effects ────────────────────────────────────────────────────────

#[test]
fn effects_filter_by_category_favorite_and_query() {
    let mut s = Stub::new();
    s.effects[2].hidden = true;
    assert_eq!(s.call("layer/effects", json!({})).unwrap().as_array().unwrap().len(), 3);
    assert_eq!(s.call("layer/effects", json!({"hidden": true})).unwrap().as_array().unwrap().len(), 4);
    let fluid = s.call("layer/effects", json!({"category": "FLUID"})).unwrap();
    assert_eq!(fluid, json!([{"index": 1, "name": "Sumi", "category": "fluid", "builtin": true, "favorite": false}]));
    s.call("layer/favorite", json!({"effect": "Sumi"})).unwrap();
    let favs = s.call("layer/effects", json!({"favorites": true})).unwrap();
    assert_eq!(favs[0]["name"], "Sumi");
    assert_eq!(s.call("layer/effects", json!({"query": "fos"})).unwrap()[0]["index"], 0);
}

#[test]
fn load_effect_by_name_on_a_named_layer_and_refuse_a_locked_one() {
    let mut s = Stub::new();
    let r = s.call("layer/load-effect", json!({"effect": "sumi", "index": 0})).unwrap();
    assert_eq!(r["effect"], "Sumi");
    assert!(s.call("layer/load-effect", json!({"index": 0})).is_err());
    assert!(s.call("layer/load-effect", json!({"effect": 99})).is_err());
    s.call("layer/set", json!({"index": 0, "locked": true})).unwrap();
    assert!(s.call("layer/load-effect", json!({"effect": "Tide", "index": 0})).is_err());
    assert_eq!(s.layers[0].view.effect.as_deref(), Some("Sumi"));
}

#[test]
fn favorite_toggles_without_on_and_sets_with_it() {
    let mut s = Stub::new();
    assert_eq!(s.call("layer/favorite", json!({"effect": 1})).unwrap()["favorite"], true);
    assert_eq!(s.call("layer/favorite", json!({"effect": 1})).unwrap()["favorite"], false);
    assert_eq!(s.call("layer/favorite", json!({"effect": 1, "on": true})).unwrap()["favorite"], true);
    assert_eq!(s.call("layer/favorite", json!({"effect": 1, "on": true})).unwrap()["favorite"], true);
}

#[test]
fn create_and_copy_need_a_fresh_name_and_copy_defaults_to_the_active_effect() {
    let mut s = Stub::new();
    assert_eq!(s.call("layer/create-effect", json!({"name": " Glass "})).unwrap()["name"], "Glass");
    assert!(s.call("layer/create-effect", json!({"name": "glass"})).is_err());
    assert!(s.call("layer/create-effect", json!({"name": "  "})).is_err());
    assert!(s.call("layer/copy-effect", json!({"as": "Mine 2"})).is_err(), "layer 0 runs no effect");
    s.call("layer/select", json!({"index": 1})).unwrap();
    let r = s.call("layer/copy-effect", json!({"as": "Mine 2"})).unwrap();
    assert_eq!(r["from"], "Fosfora");
    assert_eq!(s.call("layer/copy-effect", json!({"as": "T2", "effect": "Tide"})).unwrap()["from"], "Tide");
}

#[test]
fn builtin_effects_cannot_be_deleted_user_ones_can() {
    let mut s = Stub::new();
    assert!(s.call("layer/delete-effect", json!({"effect": "Sumi"})).is_err());
    assert_eq!(s.call("layer/delete-effect", json!({"effect": "Mine"})).unwrap()["name"], "Mine");
    assert_eq!(s.effects.len(), 3);
}

#[test]
fn the_shader_editor_opens_on_an_effect_layer_only() {
    let mut s = Stub::new();
    assert!(s.call("layer/edit-shader", json!({})).is_err());
    let r = s.call("layer/edit-shader", json!({"index": 1})).unwrap();
    assert_eq!(r["path"], "/shaders/Fosfora.wgsl");
    assert_eq!(s.call("layer/save-shader", json!({})).unwrap()["saved"][0], "/shaders/Fosfora.wgsl");
}

// ── Unit: params ─────────────────────────────────────────────────────────

#[test]
fn params_list_ranges_and_normalized_values() {
    let mut s = Stub::new();
    let r = s.call("layer/params", json!({"index": 1})).unwrap();
    assert_eq!(r["effect"], "Fosfora");
    let speed = &r["params"][0];
    assert_eq!(speed["type"], "float");
    assert_eq!(speed["max"], 10.0);
    assert!((speed["normalized"].as_f64().unwrap() - 0.1).abs() < 1e-6);
    assert_eq!(s.call("layer/params", json!({})).unwrap()["params"], json!([]));
}

#[test]
fn set_param_takes_real_units_or_normalized_and_refuses_out_of_range() {
    let mut s = Stub::new();
    let at = json!({"index": 1});
    let mut a = at.clone();
    a["param"] = json!("speed");
    a["value"] = json!(2.5);
    assert_eq!(s.call("layer/set-param", a.clone()).unwrap()["value"], 2.5);
    a["value"] = json!(0.5);
    a["normalized"] = json!(true);
    assert_eq!(s.call("layer/set-param", a.clone()).unwrap()["value"], 5.0);
    a["value"] = json!(1.5);
    assert!(s.call("layer/set-param", a.clone()).is_err());
    a["normalized"] = json!(false);
    a["value"] = json!(11);
    assert!(s.call("layer/set-param", a.clone()).is_err());
    a["value"] = json!(true);
    assert!(s.call("layer/set-param", a).is_err());
    assert_eq!(s.call("layer/param", json!({"index": 1, "param": "speed"})).unwrap()["value"], 5.0);

    let r = s.call("layer/set-param", json!({"index": 1, "param": "tint", "value": [0.2, 0.4, 0.6]})).unwrap();
    assert_eq!(r["value"].as_array().unwrap().len(), 4);
    assert!(s.call("layer/set-param", json!({"index": 1, "param": "tint", "value": [2, 0, 0]})).is_err());
    assert_eq!(s.call("layer/set-param", json!({"index": 1, "param": "glow", "value": true})).unwrap()["value"], true);
    let r = s.call("layer/set-param", json!({"index": 1, "param": "center", "value": [1, 0.5], "normalized": true})).unwrap();
    assert_eq!(r["value"], json!([1.0, 0.0]));
    assert!(s.call("layer/set-param", json!({"index": 1, "param": "nope", "value": 1})).is_err());
    assert!(s.call("layer/set-param", json!({"index": 1, "param": "speed"})).is_err());
}

#[test]
fn reset_param_resets_one_or_all() {
    let mut s = Stub::new();
    s.call("layer/set-param", json!({"index": 1, "param": "speed", "value": 7})).unwrap();
    s.call("layer/set-param", json!({"index": 1, "param": "glow", "value": true})).unwrap();
    assert_eq!(s.call("layer/reset-param", json!({"index": 1, "param": "speed"})).unwrap()["value"], 1.0);
    assert_eq!(s.call("layer/param", json!({"index": 1, "param": "glow"})).unwrap()["value"], true);
    let all = s.call("layer/reset-param", json!({"index": 1})).unwrap();
    assert_eq!(all["params"][2]["value"], false);
}

// ── Unit: media and cameras ──────────────────────────────────────────────

#[test]
fn media_transport_toggles_seeks_and_names_its_direction() {
    let mut s = Stub::new();
    assert_eq!(s.call("layer/add-media", json!({"path": "/loops/a.mp4"})).unwrap()["pending"], true);
    assert!(s.call("layer/add-media", json!({"path": "/nope.txt"})).is_err());
    let r = s.call("layer/transport", json!({"toggle": true, "direction": "Ping Pong", "seek": 12})).unwrap();
    assert_eq!(r["playing"], false);
    assert_eq!(r["direction"], "ping-pong");
    assert_eq!(r["position"], 12.0);
    assert_eq!(s.call("layer/media", json!({})).unwrap()["layer"], 2);
    for bad in [json!({"speed": 0}), json!({"speed": -1}), json!({"seek": -2}), json!({"direction": "up"}), json!({"loop": "yes"})] {
        assert!(s.call("layer/transport", bad.clone()).is_err(), "{bad}");
    }
    assert!(s.call("layer/transport", json!({"index": 0, "toggle": true})).is_err());
    assert!(s.call("layer/media", json!({"index": 1})).is_err());
}

#[test]
fn cameras_are_refused_without_support_and_picked_by_index_or_name_with_it() {
    let mut s = Stub::new();
    assert!(s.call("layer/webcams", json!({})).is_err());
    assert!(s.call("layer/add-webcam", json!({})).is_err());
    let mut s = Stub::new().with_cameras();
    assert_eq!(s.call("layer/webcams", json!({})).unwrap()["devices"][1]["index"], 2);
    let r = s.call("layer/add-webcam", json!({"device": 2})).unwrap();
    assert_eq!(r["kind"], "webcam");
    assert_eq!(r["device"], "USB Cam");
    assert!(s.call("layer/add-webcam", json!({"device": 7})).is_err());
    assert_eq!(s.call("layer/switch-webcam", json!({"device": "built"})).unwrap()["device"], "Built-in Camera");
    assert!(s.call("layer/switch-webcam", json!({"index": 0, "device": 0})).is_err(), "not a camera layer");
    assert_eq!(s.call("layer/mirror", json!({"on": true})).unwrap()["mirror"], true);
    assert!(s.call("layer/mirror", json!({})).is_err());
    assert_eq!(s.call("layer/webcam-backend", json!({"ffmpeg": true})).unwrap()["devices"], 2);
}

#[test]
fn streams_read_and_replace() {
    let mut s = Stub::new();
    assert_eq!(s.call("layer/streams", json!({})).unwrap(), json!([]));
    let r = s.call("layer/streams", json!({"streams": [{"name": "OBS", "url": "rtmp://x/live"}]})).unwrap();
    assert_eq!(r[0]["enabled"], true);
    assert!(s.call("layer/streams", json!({"streams": "rtmp://x"})).is_err());
    assert_eq!(s.streams.len(), 1);
}

// ── Unit: post-processing ────────────────────────────────────────────────

#[test]
fn postfx_set_merges_refuses_unknown_keys_and_goes_back() {
    let mut s = Stub::new();
    assert!(s.call("layer/postfx-previous", json!({})).is_err());
    let r = s.call("layer/postfx", json!({"set": {"bloom-intensity": 0.9, "tonemap": "linear"}})).unwrap();
    assert_eq!(r["tonemap"], "linear");
    assert_eq!(r["has-previous"], true);
    assert!(s.call("layer/postfx", json!({"set": {"sparkle": 1}})).is_err());
    assert!(s.call("layer/postfx", json!({"set": {"vignette": "lots"}})).is_err());
    assert_eq!(s.call("layer/postfx-previous", json!({})).unwrap()["tonemap"], "aces");
    assert_eq!(s.call("layer/adopt-postfx", json!({"index": 1})).unwrap()["tonemap"], "linear");
    assert_eq!(s.call("layer/postfx-defaults", json!({})).unwrap()["tonemap"], "aces");
}

// ── Property ─────────────────────────────────────────────────────────────

#[test]
fn blend_names_round_trip_and_any_string_parses_or_is_refused() {
    for &m in BlendMode::ALL {
        assert_eq!(blend_from_name(&blend_name(m)), Ok(m));
        assert_eq!(blend_from_name(&m.display_name().to_uppercase()), Ok(m));
    }
    cases(500, 0x1A7E_0001, |rng, _| {
        let s = rng.name();
        if let Ok(m) = blend_from_name(&s) {
            assert_eq!(squash(&blend_name(m)), squash(&s));
        }
    });
}

#[test]
fn directions_round_trip() {
    for d in [PlayDirection::Forward, PlayDirection::Reverse, PlayDirection::PingPong] {
        assert_eq!(direction_from_name(direction_name(d)), Ok(d));
    }
}

#[allow(clippy::cast_precision_loss)]
fn unit(rng: &mut crate::control::testkit::Rng) -> f64 {
    rng.below(10_001) as f64 / 10_000.0
}

#[test]
fn normalized_and_real_units_round_trip_and_out_of_range_is_refused() {
    let speed = defs()[0].clone();
    let center = defs()[3].clone();
    cases(500, 0x1A7E_0002, |rng, _| {
        let t = unit(rng);
        let v = promote_param(&speed, &json!(t), true).unwrap();
        let back = normalize(&speed, &v).unwrap().as_f64().unwrap();
        assert!((back - t).abs() < 1e-5, "{t} -> {back}");
        let real = 10.0 * t;
        assert!(promote_param(&speed, &json!(real), false).is_ok());
        assert!(promote_param(&speed, &json!(10.0 + 0.01 + real), false).is_err());
        assert!(promote_param(&speed, &json!(-0.01 - real), false).is_err());
        let p = promote_param(&center, &json!([t, 1.0 - t]), true).unwrap();
        let n = normalize(&center, &p).unwrap();
        assert!((n[0].as_f64().unwrap() - t).abs() < 1e-5);
    });
}

#[test]
fn promote_param_is_total_on_arbitrary_json() {
    let pool = [json!(null), json!("x"), json!(true), json!(-3), json!(0.5), json!([1]), json!([0.1, 0.2]),
                json!([0.1, 0.2, 0.3]), json!([0.1, 0.2, 0.3, 0.4, 0.5]), json!({"a": 1}), json!(1e40)];
    cases(300, 0x1A7E_0003, |rng, _| {
        let def = &defs()[rng.below(4)];
        let raw = rng.pick(&pool).unwrap();
        if let Ok(v) = promote_param(def, raw, rng.coin()) {
            assert!(v.is_finite());
            assert_eq!(std::mem::discriminant(&v), std::mem::discriminant(&def.default_value()));
        }
    });
}

fn random_edit(rng: &mut crate::control::testkit::Rng) -> LayerEdit {
    #[allow(clippy::cast_possible_truncation)]
    let f = |rng: &mut crate::control::testkit::Rng| rng.coin().then(|| unit(rng) as f32);
    let b = |rng: &mut crate::control::testkit::Rng| rng.coin().then(|| rng.coin());
    LayerEdit {
        opacity: f(rng),
        blend: rng.coin().then(|| *rng.pick(BlendMode::ALL).unwrap()),
        displace: f(rng),
        enabled: b(rng),
        locked: b(rng),
        pinned: b(rng),
        name: rng.coin().then(|| rng.coin().then(|| rng.name())),
    }
}

#[test]
fn a_lock_refuses_exactly_the_look_changes_that_do_not_unlock() {
    cases(500, 0x1A7E_0004, |rng, _| {
        let locked = rng.coin();
        let e = random_edit(rng);
        let looks = e.opacity.is_some() || e.blend.is_some() || e.displace.is_some() || e.enabled.is_some();
        let refused = edit_refusal(0, locked, &e).is_some();
        assert_eq!(refused, locked && looks && e.locked != Some(false), "{e:?}");
    });
}

#[test]
fn merging_postfx_is_identity_on_nothing_and_idempotent() {
    let base = PostProcessDef::default();
    assert_eq!(merge_postfx(&base, &json!({})).unwrap(), base);
    let keys = ["bloom_threshold", "vignette", "grain_rate", "ca-intensity"];
    cases(200, 0x1A7E_0005, |rng, _| {
        let mut patch = serde_json::Map::new();
        for _ in 0..rng.below(4) {
            patch.insert((*rng.pick(&keys).unwrap()).into(), json!(unit(rng)));
        }
        if rng.coin() {
            patch.insert("bloom_enabled".into(), json!(rng.coin()));
        }
        let patch = Value::Object(patch);
        let once = merge_postfx(&base, &patch).unwrap();
        assert_eq!(merge_postfx(&once, &patch).unwrap(), once);
    });
}

#[test]
fn effect_filters_keep_a_sorted_subset_that_matches() {
    let cats = ["fluid", "life", "effect"];
    cases(200, 0x1A7E_0006, |rng, _| {
        let effects: Vec<EffectView> = (0..rng.below(12))
            .map(|_| EffectView {
                name: rng.name(),
                category: (*rng.pick(&cats).unwrap()).into(),
                builtin: rng.coin(),
                favorite: rng.coin(),
                hidden: rng.coin(),
            })
            .collect();
        let cat = rng.coin().then(|| *rng.pick(&cats).unwrap());
        let favs = rng.coin();
        let keep = filter_effects(&effects, cat, favs, None, false);
        assert!(keep.windows(2).all(|w| w[0] < w[1]));
        for i in keep {
            let e = &effects[i];
            assert!(!e.hidden && (!favs || e.favorite) && cat.is_none_or(|c| e.category == c));
        }
    });
}

#[test]
fn random_op_sequences_keep_the_stack_consistent_and_refusals_change_nothing() {
    let ops = ["layer/list", "layer/add", "layer/remove", "layer/move", "layer/select", "layer/set",
               "layer/load-effect", "layer/clear", "layer/add-media", "layer/transport", "layer/set-param"];
    cases(150, 0x1A7E_0007, |rng, _| {
        let mut s = Stub::new();
        for _ in 0..40 {
            let op = *rng.pick(&ops).unwrap();
            let len = s.layers.len();
            let mut a = match rng.below(3) {
                0 => json!({"index": rng.below(len + 2)}),
                1 => json!({"name": rng.name()}),
                _ => json!({}),
            };
            a["to"] = json!(rng.below(len + 1));
            a["effect"] = json!(rng.below(5));
            a["path"] = json!(if rng.coin() { "/a.mp4" } else { "/a.txt" });
            a["param"] = json!("speed");
            a["value"] = json!(unit(rng) * 12.0);
            match rng.below(4) {
                0 => a["opacity"] = json!(unit(rng) * 1.2),
                1 => a["locked"] = json!(rng.coin()),
                2 => a["toggle"] = json!(true),
                _ => a["blend"] = json!(rng.name()),
            }
            let before = s.snapshot();
            let result = s.call(op, a.clone());
            assert!((1..=MAX).contains(&s.layers.len()), "{op}");
            assert!(s.active < s.layers.len(), "{op}");
            if result.is_err() {
                assert_eq!(s.snapshot(), before, "{op} {a} failed but changed the stack");
            }
            if let Ok(v) = &result {
                if let Some(i) = v.get("index").and_then(Value::as_u64) {
                    if op != "layer/remove" {
                        assert!((i as usize) < s.layers.len(), "{op} -> {v}");
                    }
                }
            }
        }
    });
}

#[test]
fn layer_picks_never_land_off_the_stack() {
    cases(300, 0x1A7E_0008, |rng, _| {
        let views: Vec<LayerView> = (0..=rng.below(6))
            .map(|i| {
                let mut v = effect_layer(&format!("Layer {}", i + 1), rng.coin().then_some("Sumi")).view;
                v.custom_name = rng.coin().then(|| rng.name());
                v
            })
            .collect();
        let a = if rng.coin() { json!({"index": rng.below(8)}) } else { json!({"name": rng.name()}) };
        if let Ok(i) = pick_layer(&views, &a) {
            assert!(i < views.len());
        }
    });
}
