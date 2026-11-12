//! Unit + property tests for the SVG Offset ops, against an in-memory port.
//! The integration leg (a live app over nREPL) is `tests/repl_ops.rs`.

use serde_json::{Value, json};

use super::*;
use crate::control::testkit::cases;
use crate::control::{ControlAddon, OpHost, Registry, run};

/// One layer of the stub: `None` is a layer without an SVG Offset.
type Slot = Option<SvgOffsetInfo>;

#[derive(Default)]
struct Stub {
    layers: Vec<Slot>,
    active: usize,
    /// Every command the ops sent, with its layer, in order.
    sent: Vec<(usize, SvgOffsetCommand)>,
    /// Paths that "exist" for a reload.
    files: Vec<String>,
    registry: Registry<Stub>,
}

fn svg_info(path: Option<&str>) -> SvgOffsetInfo {
    SvgOffsetInfo {
        path: path.map(str::to_owned),
        shapes: 3,
        settings: SvgOffsetSettings::default(),
    }
}

impl Stub {
    fn new(layers: Vec<Slot>) -> Self {
        let mut s = Self {
            layers,
            files: vec!["/art/logo.svg".into()],
            ..Self::default()
        };
        s.registry.register(&Addon as &dyn ControlAddon<Stub>);
        s
    }

    fn call(&mut self, op: &str, args: Value) -> Result<Value, String> {
        run(self, op, &args)
    }

    fn settings(&self, layer: usize) -> &SvgOffsetSettings {
        &self.layers[layer].as_ref().unwrap().settings
    }
}

impl OpHost for Stub {
    fn registry(&self) -> &Registry<Self> {
        &self.registry
    }
}

impl SvgPort for Stub {
    fn active_layer(&self) -> usize {
        self.active
    }
    fn layer_count(&self) -> usize {
        self.layers.len()
    }
    fn svg_state(&self, layer: usize) -> Option<SvgOffsetInfo> {
        self.layers.get(layer)?.clone()
    }
    fn apply_svg(&mut self, layer: usize, cmd: SvgOffsetCommand) -> Result<(), String> {
        let files = self.files.clone();
        let info = self
            .layers
            .get_mut(layer)
            .and_then(Option::as_mut)
            .ok_or("no svg")?;
        if cmd.reload {
            if let Some(p) = &cmd.path {
                if !files.contains(p) {
                    return Err(format!("no such file: {p}"));
                }
            }
            info.path = cmd.path.clone();
            info.shapes = if cmd.path.is_some() { 7 } else { 3 };
        }
        info.settings = cmd.settings.clone();
        self.sent.push((layer, cmd));
        Ok(())
    }
}

// ── Unit ────────────────────────────────────────────────────────────────────

#[test]
fn every_op_is_registered_under_the_svg_namespace() {
    let s = Stub::new(vec![]);
    let catalog = s.registry.catalog();
    let names: Vec<&str> = catalog
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["svg/load", "svg/reload", "svg/set", "svg/state"]);
}

#[test]
fn the_panel_signal_is_claimed_by_a_registered_op() {
    let signals = <Addon as ControlAddon<Stub>>::ui_signals(&Addon);
    assert_eq!(signals, &[("svg_offset_cmd", "svg/set")]);
    let s = Stub::new(vec![]);
    assert!(s.registry.get("svg/set").is_some());
}

#[test]
fn state_reads_the_active_layer_by_name() {
    let mut s = Stub::new(vec![None, Some(svg_info(None))]);
    s.active = 1;
    let r = s.call("svg/state", json!({})).unwrap();
    assert_eq!(r["layer"], 1);
    assert_eq!(r["builtin"], true);
    assert_eq!(r["path"], Value::Null);
    assert_eq!(r["shapes"], 3);
    assert_eq!(r["quality"], "full");
    assert_eq!(r["background"], "alpha");
    assert_eq!(r["offset"], json!([0.0, 0.0]));
    assert_eq!(r["key-repeat"], 1.0);
}

#[test]
fn a_layer_without_svg_offset_or_out_of_range_is_refused() {
    let mut s = Stub::new(vec![None, Some(svg_info(None))]);
    let e = s.call("svg/state", json!({})).unwrap_err();
    assert!(e.contains("no SVG Offset"), "{e}");
    let e = s
        .call("svg/set", json!({"layer": 5, "rotation": 1}))
        .unwrap_err();
    assert!(e.contains("out of range"), "{e}");
    assert!(s.call("svg/reload", json!({"layer": -1})).is_err());
    assert!(s.sent.is_empty());
}

#[test]
fn set_changes_only_what_is_given_without_a_reload() {
    let mut s = Stub::new(vec![Some(svg_info(Some("/art/logo.svg")))]);
    let r = s
        .call(
            "svg/set",
            json!({"rotation": 45, "background": "Black", "offset": [0.5, -0.25], "key-soft": 0.1}),
        )
        .unwrap();
    assert_eq!(r["rotation"], 45.0);
    assert_eq!(r["background"], "black");
    assert_eq!(r["offset"], json!([0.5, -0.25]));
    let (layer, cmd) = s.sent.last().unwrap();
    assert_eq!(*layer, 0);
    assert!(!cmd.reload, "settings alone do not re-rasterize");
    assert_eq!(
        cmd.path.as_deref(),
        Some("/art/logo.svg"),
        "the source is kept"
    );
    assert_eq!(s.settings(0).quality, 1);
    assert_eq!(s.settings(0).key_width, 1.0);
}

#[test]
fn a_new_quality_reloads_like_the_panel_and_the_same_one_does_not() {
    let mut s = Stub::new(vec![Some(svg_info(None))]);
    s.call("svg/set", json!({"quality": "half"})).unwrap();
    assert!(s.sent.last().unwrap().1.reload);
    assert_eq!(s.settings(0).quality, 2);
    s.call("svg/set", json!({"quality": 2})).unwrap();
    assert!(!s.sent.last().unwrap().1.reload);
}

#[test]
fn set_with_nothing_to_change_reads_back_without_acting() {
    let mut s = Stub::new(vec![Some(svg_info(None))]);
    let r = s.call("svg/set", json!({})).unwrap();
    assert_eq!(r["quality"], "full");
    assert!(s.sent.is_empty());
}

#[test]
fn out_of_range_or_unknown_values_are_refused_and_change_nothing() {
    let mut s = Stub::new(vec![Some(svg_info(None))]);
    for bad in [
        json!({"rotation": 270}),
        json!({"offset-x": 1.5}),
        json!({"offset": [0.1]}),
        json!({"offset": [0.1, "x"]}),
        json!({"key-soft": 0}),
        json!({"key-repeat": 17}),
        json!({"quality": "eighth"}),
        json!({"quality": 3}),
        json!({"background": "green"}),
        json!({"rotation": "fast"}),
    ] {
        assert!(s.call("svg/set", bad.clone()).is_err(), "{bad} accepted");
    }
    assert!(s.sent.is_empty());
    assert_eq!(*s.settings(0), SvgOffsetSettings::default());
}

#[test]
fn load_takes_a_file_or_the_builtin_and_keeps_the_settings() {
    let mut s = Stub::new(vec![Some(svg_info(None))]);
    s.call("svg/set", json!({"rotation": 30})).unwrap();
    let r = s
        .call("svg/load", json!({"path": "/art/logo.svg"}))
        .unwrap();
    assert_eq!(r["path"], "/art/logo.svg");
    assert_eq!(r["builtin"], false);
    assert_eq!(r["shapes"], 7);
    assert_eq!(r["rotation"], 30.0);
    assert!(s.sent.last().unwrap().1.reload);
    let r = s.call("svg/load", json!({})).unwrap();
    assert_eq!(r["builtin"], true);
    assert!(s.call("svg/load", json!({"path": 3})).is_err());
    let e = s
        .call("svg/load", json!({"path": "/nope.svg"}))
        .unwrap_err();
    assert!(e.contains("no such file"), "{e}");
}

#[test]
fn reload_rereads_the_current_source() {
    let mut s = Stub::new(vec![Some(svg_info(Some("/art/logo.svg")))]);
    s.call("svg/reload", json!({})).unwrap();
    let (_, cmd) = s.sent.last().unwrap();
    assert!(cmd.reload);
    assert_eq!(cmd.path.as_deref(), Some("/art/logo.svg"));
}

#[test]
fn quality_and_background_names_round_trip() {
    for &(value, name) in QUALITIES {
        let edit = SettingsEdit::from_args(&json!({"quality": name})).unwrap();
        assert_eq!(edit.quality, Some(value));
        assert_eq!(name_of(QUALITIES, value), name);
    }
    for &(value, name) in BACKGROUNDS {
        let edit = SettingsEdit::from_args(&json!({"background": value})).unwrap();
        assert_eq!(edit.background, Some(value));
        assert_eq!(name_of(BACKGROUNDS, value), name);
    }
    assert_eq!(name_of(QUALITIES, 3), "full");
}

#[test]
fn offset_halves_win_over_the_pair() {
    let edit = SettingsEdit::from_args(&json!({"offset": [0.1, 0.2], "offset-y": -0.5})).unwrap();
    assert_eq!((edit.offset_x, edit.offset_y), (Some(0.1), Some(-0.5)));
}

// ── Property ────────────────────────────────────────────────────────────────

#[test]
fn an_empty_edit_is_the_identity_and_a_full_edit_ignores_the_base() {
    cases(300, 0x5EED_5001, |rng, _| {
        let f = |rng: &mut crate::control::testkit::Rng, lo: f32, hi: f32| {
            #[allow(clippy::cast_precision_loss)]
            let t = rng.below(1001) as f32 / 1000.0;
            lo + (hi - lo) * t
        };
        let base = SvgOffsetSettings {
            quality: QUALITIES[rng.below(3)].0,
            background: BACKGROUNDS[rng.below(3)].0,
            offset: [f(rng, -1.0, 1.0), f(rng, -1.0, 1.0)],
            rotation: f(rng, -180.0, 180.0),
            key_width: f(rng, 0.0, 1.0),
            key_phase: f(rng, 0.0, 1.0),
            key_soft: f(rng, 0.001, 0.5),
            key_repeat: f(rng, 1.0, 16.0),
        };
        assert_eq!(SettingsEdit::default().apply(&base), base);
        let edit = SettingsEdit {
            quality: Some(QUALITIES[rng.below(3)].0),
            background: Some(BACKGROUNDS[rng.below(3)].0),
            offset_x: Some(f(rng, -1.0, 1.0)),
            offset_y: Some(f(rng, -1.0, 1.0)),
            rotation: Some(f(rng, -180.0, 180.0)),
            key_width: Some(f(rng, 0.0, 1.0)),
            key_phase: Some(f(rng, 0.0, 1.0)),
            key_soft: Some(f(rng, 0.001, 0.5)),
            key_repeat: Some(f(rng, 1.0, 16.0)),
        };
        let once = edit.apply(&base);
        assert_eq!(edit.apply(&SvgOffsetSettings::default()), once);
        assert_eq!(
            edit.apply(&once),
            once,
            "applying twice changes nothing more"
        );
    });
}

#[test]
fn random_set_calls_keep_settings_inside_the_panel_ranges() {
    let keys = [
        ("rotation", -400.0, 400.0),
        ("offset-x", -2.0, 2.0),
        ("offset-y", -2.0, 2.0),
        ("key-width", -0.5, 1.5),
        ("key-phase", -0.5, 1.5),
        ("key-soft", -0.1, 0.7),
        ("key-repeat", 0.0, 20.0),
    ];
    let names = ["full", "half", "quarter", "alpha", "black", "white", "x"];
    cases(200, 0x5EED_5002, |rng, _| {
        let mut s = Stub::new(vec![Some(svg_info(None)), None]);
        for _ in 0..20 {
            let mut args = serde_json::Map::new();
            for &(k, lo, hi) in &keys {
                if rng.coin() {
                    #[allow(clippy::cast_precision_loss)]
                    let t = rng.below(1001) as f64 / 1000.0;
                    args.insert(k.into(), json!(lo + (hi - lo) * t));
                }
            }
            if rng.coin() {
                args.insert("quality".into(), json!(rng.pick(&names).unwrap()));
            }
            if rng.coin() {
                args.insert("background".into(), json!(rng.pick(&names).unwrap()));
            }
            if rng.below(4) == 0 {
                args.insert("layer".into(), json!(rng.below(3)));
            }
            let before = s.settings(0).clone();
            let sent = s.sent.len();
            match s.call("svg/set", Value::Object(args)) {
                Ok(v) => assert_eq!(v["layer"], 0),
                Err(_) => {
                    assert_eq!(*s.settings(0), before, "a refused call changed settings");
                    assert_eq!(s.sent.len(), sent);
                }
            }
            let st = s.settings(0);
            assert!(QUALITIES.iter().any(|(q, _)| *q == st.quality));
            assert!(BACKGROUNDS.iter().any(|(b, _)| *b == st.background));
            assert!((-180.0..=180.0).contains(&st.rotation));
            assert!(st.offset.iter().all(|o| (-1.0..=1.0).contains(o)));
            assert!((0.0..=1.0).contains(&st.key_width));
            assert!((0.0..=1.0).contains(&st.key_phase));
            assert!((0.001..=0.5).contains(&st.key_soft));
            assert!((1.0..=16.0).contains(&st.key_repeat));
        }
    });
}
