//! Unit + property tests for the trama ops, against an in-memory port that
//! holds real `NodeGraph`s. The integration leg is `tests/repl_ops.rs`.

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::{Value, json};

use super::modedit::modulation_edit;
use super::pipeline::{export_path, param_value, value_json, wires_to_drop};
use super::*;
use crate::control::testkit::cases;
use crate::control::{ControlAddon, OpHost, Registry, run};
use crate::params::ParamValue;
use crate::trama::modulation::Modulation;
use crate::trama::ser::EffectShape;

fn float(name: &str, default: f32, min: f32, max: f32) -> ParamDef {
    ParamDef::Float {
        name: name.into(),
        default,
        min,
        max,
    }
}

fn library() -> Vec<EffectInfo> {
    vec![
        EffectInfo {
            id: "kaleido".into(),
            name: "Kaleidoscope".into(),
            kind: EffectKind::Effect,
            inputs: 1,
            params: vec![
                float("segments", 6.0, 1.0, 32.0),
                float("spin", 0.0, -1.0, 1.0),
                ParamDef::Color {
                    name: "tint".into(),
                    default: [1.0; 4],
                },
                ParamDef::Bool {
                    name: "mirror".into(),
                    default: false,
                },
                ParamDef::Point2D {
                    name: "center".into(),
                    default: [0.5; 2],
                    min: [0.0; 2],
                    max: [1.0; 2],
                },
            ],
            integers: vec!["segments".into()],
            error: None,
        },
        EffectInfo {
            id: "mix".into(),
            name: "Mix".into(),
            kind: EffectKind::Effect,
            inputs: 2,
            params: vec![float("amount", 0.5, 0.0, 1.0)],
            integers: vec![],
            error: None,
        },
        EffectInfo {
            id: "noise_field".into(),
            name: "Noise Field".into(),
            kind: EffectKind::Source,
            inputs: 0,
            params: vec![float("scale", 3.0, 0.0, 10.0)],
            integers: vec![],
            error: None,
        },
    ]
}

struct Stub {
    layers: Vec<(String, Option<NodeGraph>)>,
    master: NodeGraph,
    pos: HashMap<(ChainRef, NodeId), [f32; 2]>,
    effects: Vec<EffectInfo>,
    editor: EditorState,
    edits: usize,
    registry: Registry<Stub>,
}

impl Stub {
    fn new(layers: &[&str]) -> Self {
        let mut s = Self {
            layers: layers.iter().map(|n| ((*n).to_owned(), None)).collect(),
            master: NodeGraph::new_with_output(),
            pos: HashMap::new(),
            effects: library(),
            editor: EditorState::default(),
            edits: 0,
            registry: Registry::default(),
        };
        s.registry.register(&Addon as &dyn ControlAddon<Stub>);
        s
    }

    fn call(&mut self, op: &str, args: Value) -> Result<Value, String> {
        run(self, op, &args)
    }

    fn graph_of(&self, chain: ChainRef) -> &NodeGraph {
        self.chain_graph(chain).expect("chain exists")
    }

    fn id(v: &Value) -> u64 {
        v["id"].as_u64().expect("an id")
    }
}

impl OpHost for Stub {
    fn registry(&self) -> &Registry<Self> {
        &self.registry
    }
}

impl ChainPort for Stub {
    fn layer_names(&self) -> Vec<String> {
        self.layers.iter().map(|(n, _)| n.clone()).collect()
    }
    fn chain_graph(&self, chain: ChainRef) -> Option<&NodeGraph> {
        match chain {
            ChainRef::Master => Some(&self.master),
            ChainRef::Layer(i) => self.layers.get(i)?.1.as_ref(),
        }
    }
    fn node_pos(&self, chain: ChainRef, node: NodeId) -> Option<[f32; 2]> {
        self.pos.get(&(chain, node)).copied()
    }
    fn edit_chain<R>(
        &mut self,
        chain: ChainRef,
        edit: impl FnOnce(&mut NodeGraph) -> Result<R, String>,
    ) -> Result<R, String> {
        let graph = match chain {
            ChainRef::Master => &mut self.master,
            ChainRef::Layer(i) => self
                .layers
                .get_mut(i)
                .ok_or("no such layer")?
                .1
                .get_or_insert_with(NodeGraph::new_with_output),
        };
        let out = edit(graph)?;
        self.edits += 1;
        Ok(out)
    }
    fn place_nodes(
        &mut self,
        chain: ChainRef,
        placed: &[(NodeId, [f32; 2])],
    ) -> Result<(), String> {
        let graph = self
            .chain_graph(chain)
            .ok_or("that layer has no chain yet")?;
        if placed.iter().any(|(n, _)| graph.node(*n).is_none()) {
            return Err("unknown node".into());
        }
        for &(n, p) in placed {
            self.pos.insert((chain, n), p);
        }
        Ok(())
    }
    fn replace_chain(&mut self, chain: ChainRef, doc: &ChainDoc) -> Result<Vec<String>, String> {
        let effects = self.effects.clone();
        let restored = doc
            .restore(|id| {
                effects.iter().find(|e| e.id == id.0).map(|e| EffectShape {
                    inputs: e.inputs,
                    params: &e.params,
                })
            })
            .map_err(|e| e.to_string())?;
        self.pos.retain(|(c, _), _| *c != chain);
        for (n, p) in restored.positions {
            self.pos.insert((chain, n), p);
        }
        match chain {
            ChainRef::Master => self.master = restored.graph,
            ChainRef::Layer(i) => {
                self.layers.get_mut(i).ok_or("no such layer")?.1 = Some(restored.graph);
            }
        }
        Ok(restored.notes)
    }
    fn trama_effects(&self) -> Vec<EffectInfo> {
        self.effects.clone()
    }
    fn trama_errors(&self) -> Vec<(String, String)> {
        vec![("broken.wgsl".into(), "WGSL validation: nope".into())]
    }
    fn trama_dirs(&self) -> Vec<PathBuf> {
        vec![PathBuf::from("/assets/trama/effects")]
    }
}

impl EditorPort for Stub {
    fn editor(&self) -> EditorState {
        self.editor
    }
    fn open_editor(&mut self, chain: ChainRef) -> Result<(), String> {
        match chain {
            ChainRef::Master => self.editor.on_master = true,
            ChainRef::Layer(i) if i < self.layers.len() => {
                self.editor.on_master = false;
                self.editor.selected_layer = i;
            }
            ChainRef::Layer(_) => return Err("no such layer".into()),
        }
        self.editor.open = true;
        Ok(())
    }
    fn close_editor(&mut self) {
        self.editor.open = false;
    }
}

/// layer input -> kaleido -> Output on `chain`; the node ids.
fn patch(s: &mut Stub, chain: Value) -> (u64, u64) {
    let input = Stub::id(
        &s.call("trama/add", json!({"chain": chain, "kind": "layer-input"}))
            .unwrap(),
    );
    let k = Stub::id(
        &s.call("trama/add", json!({"chain": chain, "effect": "kaleido"}))
            .unwrap(),
    );
    s.call(
        "trama/connect",
        json!({"chain": chain, "from": input, "to": k}),
    )
    .unwrap();
    s.call(
        "trama/connect",
        json!({"chain": chain, "from": k, "to": "output"}),
    )
    .unwrap();
    (input, k)
}

// ── Unit ────────────────────────────────────────────────────────────────────

#[test]
fn every_op_is_registered_under_trama_and_the_signals_name_real_ops() {
    let s = Stub::new(&[]);
    let catalog = s.registry.catalog();
    let names: Vec<&str> = catalog
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap())
        .collect();
    assert!(names.iter().all(|n| n.starts_with("trama/")), "{names:?}");
    assert_eq!(names.len(), 20, "{names:?}");
    for (signal, op) in ControlAddon::<Stub>::ui_signals(&Addon) {
        assert!(names.contains(op), "{signal} -> {op} is not registered");
    }
}

#[test]
fn chains_lists_the_master_and_every_layer_and_reads_create_nothing() {
    let mut s = Stub::new(&["Tunnel", "Dots"]);
    let r = s.call("trama/chains", json!({})).unwrap();
    assert_eq!(r.as_array().unwrap().len(), 3);
    assert_eq!(r[0]["chain"], "master");
    assert_eq!(r[2]["name"], "Dots");
    assert_eq!(r[1]["exists"], false);
    let g = s.call("trama/graph", json!({"chain": 1})).unwrap();
    assert_eq!(g["exists"], false);
    assert_eq!(
        g["nodes"].as_array().unwrap().len(),
        1,
        "a fresh chain is its Output"
    );
    assert!(s.layers[1].1.is_none(), "reading did not create a chain");
    assert_eq!(s.edits, 0);
}

#[test]
fn chain_arg_takes_master_an_index_or_a_name_and_defaults_to_the_editor() {
    let layers = vec!["Tunnel".to_owned(), "Dots".to_owned()];
    let m = ChainRef::Master;
    assert_eq!(chain_ref(&json!({}), &layers, m), Ok(m));
    assert_eq!(
        chain_ref(&json!({"chain": "MASTER"}), &layers, ChainRef::Layer(0)),
        Ok(m)
    );
    assert_eq!(
        chain_ref(&json!({"chain": 1}), &layers, m),
        Ok(ChainRef::Layer(1))
    );
    assert_eq!(
        chain_ref(&json!({"chain": "tun"}), &layers, m),
        Ok(ChainRef::Layer(0))
    );
    assert!(chain_ref(&json!({"chain": 5}), &layers, m).is_err());
    assert!(chain_ref(&json!({"chain": true}), &layers, m).is_err());
    let on_layer = EditorState {
        open: true,
        on_master: false,
        selected_layer: 1,
    };
    assert_eq!(default_chain(&on_layer, 2), ChainRef::Layer(1));
    assert_eq!(
        default_chain(&on_layer, 0),
        ChainRef::Master,
        "no layers: the master"
    );
}

#[test]
fn add_places_effects_by_id_or_name_and_utility_nodes_by_kind() {
    let mut s = Stub::new(&["Tunnel"]);
    let k = s
        .call(
            "trama/add",
            json!({"chain": 0, "effect": "kaleido", "pos": [280, 160]}),
        )
        .unwrap();
    assert_eq!(k["title"], "Kaleidoscope");
    assert_eq!(k["pos"], json!([280.0, 160.0]));
    assert_eq!(k["chain"], 0);
    assert_eq!(k["params"]["segments"], 6.0);
    let n = s
        .call("trama/add", json!({"chain": 0, "effect": "Noise"}))
        .unwrap();
    assert_eq!(n["kind"], "source");
    for kind in ["feedback", "anchor", "layer-input"] {
        assert_eq!(
            s.call("trama/add", json!({"chain": 0, "kind": kind}))
                .unwrap()["kind"],
            kind
        );
    }
    let e = s
        .call("trama/add", json!({"chain": 0, "kind": "layer_input"}))
        .unwrap_err();
    assert!(e.contains("already has its layer input"), "{e}");
    assert!(
        s.call("trama/add", json!({"chain": 0, "effect": "nope"}))
            .is_err()
    );
    assert!(
        s.call("trama/add", json!({"chain": 0, "kind": "output"}))
            .is_err()
    );
    assert!(s.call("trama/add", json!({"chain": 0})).is_err());
    assert!(
        s.call(
            "trama/add",
            json!({"chain": 0, "effect": "mix", "pos": [1]})
        )
        .is_err()
    );
    assert_eq!(
        s.graph_of(ChainRef::Layer(0)).placed_nodes(),
        4,
        "an anchor is not counted"
    );
    assert!(s.graph_of(ChainRef::Layer(0)).validate().is_ok());
}

#[test]
fn connect_wires_by_id_or_name_and_refuses_cycles_and_bad_pins() {
    let mut s = Stub::new(&[]);
    let (input, k) = patch(&mut s, json!("master"));
    let g = s.call("trama/graph", json!({"chain": "master"})).unwrap();
    assert_eq!(g["contributes"], true);
    assert_eq!(g["wires"].as_array().unwrap().len(), 2);
    assert!(
        g["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| n["live"] == true)
    );
    assert!(
        s.call(
            "trama/connect",
            json!({"chain": "master", "from": k, "to": input})
        )
        .is_err(),
        "input has no pins"
    );
    assert!(
        s.call(
            "trama/connect",
            json!({"chain": "master", "from": k, "to": k})
        )
        .is_err()
    );
    assert!(
        s.call(
            "trama/connect",
            json!({"chain": "master", "from": "output", "to": k})
        )
        .is_err()
    );
    assert!(
        s.call(
            "trama/connect",
            json!({"chain": "master", "from": input, "to": k, "input": 3})
        )
        .is_err()
    );
    let m = Stub::id(
        &s.call("trama/add", json!({"chain": "master", "effect": "mix"}))
            .unwrap(),
    );
    s.call(
        "trama/connect",
        json!({"chain": "master", "from": m, "to": k}),
    )
    .unwrap();
    let e = s
        .call(
            "trama/connect",
            json!({"chain": "master", "from": k, "to": m, "input": 1}),
        )
        .unwrap_err();
    assert!(e.contains("cycle"), "{e}");
    assert!(s.master.validate().is_ok());
}

#[test]
fn a_name_several_nodes_share_is_refused_with_their_ids() {
    let mut s = Stub::new(&[]);
    s.call("trama/add", json!({"kind": "feedback"})).unwrap();
    s.call("trama/add", json!({"kind": "feedback"})).unwrap();
    let e = s
        .call("trama/remove", json!({"node": "feedback"}))
        .unwrap_err();
    assert!(e.contains("give the id"), "{e}");
    assert!(s.call("trama/remove", json!({"node": 99})).is_err());
    assert!(s.call("trama/remove", json!({})).is_err());
}

#[test]
fn disconnect_drops_a_pin_a_node_or_a_source_and_reports_what_went() {
    let mut s = Stub::new(&[]);
    let (input, k) = patch(&mut s, json!("master"));
    let r = s
        .call("trama/disconnect", json!({"to": "output", "input": 0}))
        .unwrap();
    assert_eq!(r["removed"], json!([{"from": k, "to": 0, "input": 0}]));
    let r = s.call("trama/disconnect", json!({"from": input})).unwrap();
    assert_eq!(r["removed"].as_array().unwrap().len(), 1);
    assert!(s.master.wires().is_empty());
    let r = s.call("trama/disconnect", json!({"to": k})).unwrap();
    assert_eq!(r["removed"], json!([]), "nothing left to drop");
    assert!(s.call("trama/disconnect", json!({})).is_err());
}

#[test]
fn remove_takes_the_node_and_its_wires_but_never_output() {
    let mut s = Stub::new(&[]);
    let (_, k) = patch(&mut s, json!("master"));
    let gone = s.call("trama/remove", json!({"node": "kaleido"})).unwrap();
    assert_eq!(Stub::id(&gone), k);
    assert_eq!(s.master.wires().len(), 0);
    assert!(s.call("trama/remove", json!({"node": "output"})).is_err());
    assert!(s.master.node(NodeId(0)).is_some());
}

#[test]
fn bypass_toggles_or_sets_and_refuses_output() {
    let mut s = Stub::new(&[]);
    let (_, k) = patch(&mut s, json!("master"));
    assert_eq!(
        s.call("trama/bypass", json!({"node": k})).unwrap()["bypass"],
        true
    );
    assert_eq!(
        s.call("trama/bypass", json!({"node": k})).unwrap()["bypass"],
        false
    );
    assert_eq!(
        s.call("trama/bypass", json!({"node": k, "on": true}))
            .unwrap()["bypass"],
        true
    );
    assert!(s.call("trama/bypass", json!({"node": "output"})).is_err());
}

#[test]
fn set_takes_every_value_shape_resets_and_refuses_wrong_types() {
    let mut s = Stub::new(&[]);
    let (_, k) = patch(&mut s, json!("master"));
    let set = |s: &mut Stub, param: &str, value: Value| {
        s.call(
            "trama/set",
            json!({"node": k, "param": param, "value": value}),
        )
    };
    assert_eq!(set(&mut s, "segments", json!(8)).unwrap()["value"], 8.0);
    assert_eq!(set(&mut s, "mirror", json!(true)).unwrap()["value"], true);
    assert_eq!(
        set(&mut s, "center", json!([0.25, 0.75])).unwrap()["value"],
        json!([0.25, 0.75])
    );
    assert_eq!(
        set(&mut s, "tint", json!([1, 0, 0])).unwrap()["value"],
        json!([1.0, 0.0, 0.0, 1.0])
    );
    assert!(set(&mut s, "segments", json!("eight")).is_err());
    assert!(set(&mut s, "center", json!([1, 2, 3])).is_err());
    assert!(set(&mut s, "nope", json!(1)).is_err());
    assert!(
        s.call("trama/set", json!({"node": k, "param": "segments"}))
            .is_err()
    );
    let r = s
        .call(
            "trama/set",
            json!({"node": k, "param": "segments", "reset": true}),
        )
        .unwrap();
    assert_eq!(r["value"], 6.0);
    let node = s.call("trama/node", json!({"node": k})).unwrap();
    let seg = node["params"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "segments")
        .unwrap();
    assert_eq!(seg["integer"], true);
    assert_eq!(seg["min"], 1.0);
    assert_eq!(node["feeds"][0]["from"], json!(1));
}

#[test]
fn mod_adds_changes_and_removes_a_modulation_one_field_at_a_time() {
    let mut s = Stub::new(&[]);
    let (_, k) = patch(&mut s, json!("master"));
    let m = |s: &mut Stub, extra: Value| {
        let mut a = json!({"node": k, "param": "spin"});
        a.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        s.call("trama/mod", a)
    };
    assert!(
        m(&mut s, json!({"amount": 0.3})).is_err(),
        "no modulation yet and no source"
    );
    let r = m(&mut s, json!({"source": "bass"})).unwrap();
    assert_eq!(r["mod"]["source"], json!({"audio": "bass"}));
    assert_eq!(r["mod"]["amount"], 0.5);
    let r = m(&mut s, json!({"amount": -0.25, "mode": "replace"})).unwrap();
    assert_eq!(r["mod"]["source"], json!({"audio": "bass"}), "source kept");
    assert_eq!(r["mod"]["mode"], "replace");
    assert!(
        m(&mut s, json!({"rate": 2.0})).is_err(),
        "rate is an oscillator's"
    );
    let r = m(
        &mut s,
        json!({"source": "sample-hold", "rate": "beat", "phase": 0.5}),
    )
    .unwrap();
    let osc = &r["mod"]["source"]["oscillator"];
    assert_eq!(osc["shape"], "sample_hold");
    assert_eq!(osc["rate"], json!({"beat_sync": "beat"}));
    assert_eq!(r["mod"]["amount"], -0.25, "depth survives a source change");
    assert!(m(&mut s, json!({"amount": 2.0})).is_err());
    assert!(m(&mut s, json!({"source": "wobble"})).is_err());
    assert!(
        s.call(
            "trama/mod",
            json!({"node": k, "param": "mirror", "source": "bass"})
        )
        .is_err()
    );
    assert_eq!(m(&mut s, json!({"off": true})).unwrap()["mod"], Value::Null);
    assert!(s.master.node(NodeId(k)).unwrap().mods.is_empty());
}

#[test]
fn doc_import_and_clear_round_trip_a_chain() {
    let mut s = Stub::new(&["Tunnel"]);
    let (input, k) = patch(&mut s, json!(0));
    for (n, x) in [(json!(input), 0), (json!(k), 10), (json!("output"), 20)] {
        s.call("trama/move", json!({"chain": 0, "node": n, "pos": [x, 20]}))
            .unwrap();
    }
    s.call(
        "trama/set",
        json!({"chain": 0, "node": k, "param": "segments", "value": 9}),
    )
    .unwrap();
    let before = s.call("trama/graph", json!({"chain": 0})).unwrap();
    let doc = s.call("trama/doc", json!({"chain": 0})).unwrap();
    let cleared = s.call("trama/clear", json!({"chain": 0})).unwrap();
    assert_eq!(cleared["placed"], 0);
    assert_eq!(cleared["contributes"], false);
    let r = s
        .call("trama/import", json!({"chain": 0, "doc": doc}))
        .unwrap();
    assert_eq!(r["notes"], json!([]));
    assert_eq!(r["graph"], before);
    assert!(
        s.call("trama/import", json!({"chain": 0, "doc": {"nope": 1}}))
            .is_err()
    );
    assert!(s.call("trama/import", json!({"chain": 0})).is_err());
    assert_eq!(
        s.call("trama/graph", json!({"chain": 0})).unwrap(),
        before,
        "a refused import changes nothing"
    );
}

#[test]
fn export_writes_a_file_import_reads_back() {
    let mut s = Stub::new(&[]);
    patch(&mut s, json!("master"));
    let dir = std::env::temp_dir().join(format!("trama-ops-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let r = s
        .call(
            "trama/export",
            json!({"path": dir.join("chain").display().to_string()}),
        )
        .unwrap();
    let path = r["path"].as_str().unwrap().to_owned();
    assert!(path.ends_with("chain.fio.json"), "{path}");
    let mut t = Stub::new(&["Tunnel"]);
    let r = t
        .call("trama/import", json!({"chain": 0, "path": path}))
        .unwrap();
    assert_eq!(r["graph"]["contributes"], true);
    assert!(
        t.call(
            "trama/import",
            json!({"path": dir.join("missing.json").display().to_string()})
        )
        .is_err()
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn seed_gives_an_empty_chain_the_starter_patch_once() {
    let mut s = Stub::new(&["Tunnel"]);
    let g = s.call("trama/seed", json!({"chain": 0})).unwrap();
    assert_eq!(g["contributes"], true);
    assert_eq!(g["placed"], 2);
    assert!(
        g["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| !n["pos"].is_null())
    );
    assert!(
        s.call("trama/seed", json!({"chain": 0})).is_err(),
        "not empty any more"
    );
    assert!(
        s.call(
            "trama/seed",
            json!({"chain": "master", "effect": "noise_field"})
        )
        .is_err(),
        "a source has no input"
    );
}

#[test]
fn editor_opens_on_a_layer_or_the_master_and_ops_default_to_its_chain() {
    let mut s = Stub::new(&["Tunnel", "Dots"]);
    let r = s.call("trama/open", json!({"chain": "dots"})).unwrap();
    assert_eq!(
        r,
        json!({"open": true, "tab": "layer", "layer": 1, "chain": 1})
    );
    s.call("trama/add", json!({"kind": "feedback"})).unwrap();
    assert!(
        s.layers[1].1.is_some(),
        "no :chain edits the editor's chain"
    );
    assert_eq!(
        s.call("trama/open", json!({"chain": "master"})).unwrap()["chain"],
        "master"
    );
    assert!(s.call("trama/open", json!({"chain": 7})).is_err());
    assert_eq!(s.call("trama/close", json!({})).unwrap()["open"], false);
    assert_eq!(s.call("trama/editor", json!({})).unwrap()["tab"], "master");
}

#[test]
fn effects_lists_the_library_with_failures_and_dirs() {
    let mut s = Stub::new(&[]);
    let r = s.call("trama/effects", json!({})).unwrap();
    assert_eq!(r["effects"].as_array().unwrap().len(), 3);
    assert_eq!(r["errors"][0]["file"], "broken.wgsl");
    assert_eq!(r["dirs"], json!(["/assets/trama/effects"]));
    let r = s.call("trama/effects", json!({"kind": "source"})).unwrap();
    assert_eq!(r["effects"], json!([r["effects"][0].clone()]));
    assert_eq!(r["effects"][0]["id"], "noise_field");
    assert!(s.call("trama/effects", json!({"kind": "shader"})).is_err());
}

#[test]
fn a_refused_edit_changes_nothing() {
    let mut s = Stub::new(&[]);
    let (_, k) = patch(&mut s, json!("master"));
    let before = s.call("trama/doc", json!({})).unwrap();
    let edits = s.edits;
    for (op, a) in [
        ("trama/connect", json!({"from": k, "to": k})),
        ("trama/remove", json!({"node": "output"})),
        (
            "trama/set",
            json!({"node": k, "param": "segments", "value": "x"}),
        ),
        (
            "trama/mod",
            json!({"node": k, "param": "spin", "amount": 5}),
        ),
        ("trama/move", json!({"node": 42, "pos": [0, 0]})),
    ] {
        assert!(s.call(op, a).is_err(), "{op}");
    }
    assert_eq!(s.call("trama/doc", json!({})).unwrap(), before);
    assert_eq!(s.edits, edits);
}

// ── Property ────────────────────────────────────────────────────────────────

fn random_value(rng: &mut crate::control::testkit::Rng) -> Value {
    match rng.below(7) {
        0 => json!(rng.range_i(-5, 40)),
        1 => json!(rng.coin()),
        2 => json!([rng.range_i(0, 3), rng.range_i(0, 3)]),
        3 => json!([0.1, 0.2, 0.3, 0.4]),
        4 => json!([1, 0, 1]),
        5 => json!(rng.name()),
        _ => Value::Null,
    }
}

#[test]
fn random_op_sequences_keep_every_chain_a_valid_graph() {
    let ops = [
        "trama/add",
        "trama/remove",
        "trama/connect",
        "trama/disconnect",
        "trama/bypass",
        "trama/set",
        "trama/mod",
        "trama/move",
        "trama/clear",
        "trama/seed",
        "trama/graph",
        "trama/open",
    ];
    let names = [
        "kaleido",
        "mix",
        "noise_field",
        "output",
        "feedback",
        "anchor",
        "layer-input",
    ];
    let params = [
        "segments", "spin", "tint", "mirror", "center", "amount", "scale",
    ];
    cases(150, 0x7A4A_0001, |rng, _| {
        let mut s = Stub::new(&["A", "B"]);
        for _ in 0..40 {
            let op = *rng.pick(&ops).unwrap();
            let chain = match rng.below(3) {
                0 => json!("master"),
                n => json!(n - 1),
            };
            let node = |rng: &mut crate::control::testkit::Rng| {
                if rng.coin() {
                    json!(rng.below(8))
                } else {
                    json!(*rng.pick(&names).unwrap())
                }
            };
            let a = json!({
                "chain": chain,
                "node": node(rng), "from": node(rng), "to": node(rng),
                "input": rng.below(3),
                "effect": *rng.pick(&names[..3]).unwrap(),
                "kind": *rng.pick(&names[4..]).unwrap(),
                "param": *rng.pick(&params).unwrap(),
                "value": random_value(rng),
                "source": *rng.pick(&["bass", "sine", "drift", "nope"]).unwrap(),
                "amount": rng.range_i(-2, 2),
                "pos": [rng.range_i(0, 500), rng.range_i(0, 500)],
            });
            let result = s.call(op, a);
            for c in [ChainRef::Master, ChainRef::Layer(0), ChainRef::Layer(1)] {
                if let Some(g) = s.chain_graph(c) {
                    assert_eq!(g.validate(), Ok(()), "{op} broke {c:?}");
                    assert!(g.node(g.output_node()).is_some());
                }
            }
            if let Ok(v) = &result {
                if let Some(id) = v.get("id").and_then(Value::as_u64) {
                    if op == "trama/add" {
                        let c = match v["chain"].as_u64() {
                            Some(i) => ChainRef::Layer(i as usize),
                            None => ChainRef::Master,
                        };
                        assert!(s.graph_of(c).node(NodeId(id)).is_some(), "{op} -> {v}");
                    }
                }
            }
        }
    });
}

#[test]
fn param_value_is_total_and_round_trips_what_it_accepts() {
    let defs = library().remove(0).params;
    cases(400, 0x7A4A_0002, |rng, _| {
        let def = rng.pick(&defs).unwrap();
        let raw = random_value(rng);
        if let Ok(v) = param_value(def, &raw) {
            assert!(v.is_finite());
            assert_eq!(
                std::mem::discriminant(&v),
                std::mem::discriminant(&def.default_value())
            );
            let again = param_value(def, &value_json(&v)).expect("its own spelling is accepted");
            assert_eq!(value_json(&again), value_json(&v));
        }
    });
}

#[test]
fn modulation_edits_are_total_idempotent_and_round_trip() {
    let sources = [
        "bass",
        "rms",
        "band7",
        "sine",
        "saw",
        "drift",
        "sample_hold",
        "beat-phase",
        "x",
    ];
    cases(400, 0x7A4A_0003, |rng, _| {
        let mut a = serde_json::Map::new();
        if rng.coin() {
            a.insert("source".into(), json!(*rng.pick(&sources).unwrap()));
        }
        if rng.coin() {
            a.insert("amount".into(), json!(rng.range_i(-3, 3) as f64 / 2.0));
        }
        if rng.coin() {
            a.insert(
                "mode".into(),
                json!(*rng.pick(&["add", "multiply", "replace", "mul"]).unwrap()),
            );
        }
        if rng.coin() {
            a.insert(
                "rate".into(),
                if rng.coin() {
                    json!(rng.below(4))
                } else {
                    json!("bar")
                },
            );
        }
        if rng.below(8) == 0 {
            a.insert("off".into(), json!(true));
        }
        let a = Value::Object(a);
        let start = modulation_edit(None, &json!({"source": "sine"})).unwrap();
        let current = if rng.coin() { start } else { None };
        if let Ok(once) = modulation_edit(current, &a) {
            assert_eq!(
                modulation_edit(once, &a),
                Ok(once),
                "the same edit twice is the edit once"
            );
            if let Some(m) = once {
                let back: Modulation =
                    serde_json::from_value(serde_json::to_value(m).unwrap()).unwrap();
                assert_eq!(back, m);
            }
        }
    });
}

#[test]
fn dropping_wires_is_idempotent_and_export_paths_always_have_an_extension() {
    cases(200, 0x7A4A_0004, |rng, _| {
        let mut g = NodeGraph::new_with_output();
        let ids: Vec<NodeId> = (0..=rng.below(5))
            .map(|_| g.add_node(NodeKind::Anchor, 1, &[]))
            .collect();
        for _ in 0..8 {
            let (a, b) = (*rng.pick(&ids).unwrap(), *rng.pick(&ids).unwrap());
            let _ = g.connect(a, b, 0);
        }
        let from = rng.coin().then(|| *rng.pick(&ids).unwrap());
        let to = (from.is_none() || rng.coin()).then(|| *rng.pick(&ids).unwrap());
        for w in wires_to_drop(&g, from, to, None) {
            g.disconnect(w.to, w.to_input);
        }
        assert!(wires_to_drop(&g, from, to, None).is_empty());
        assert!(g.validate().is_ok());
        let raw = rng.name();
        let p = export_path(&format!("~/{raw}"), Some(std::path::Path::new("/home/v")));
        assert!(p.extension().is_some(), "{p:?}");
        assert!(p.starts_with("/home/v"));
    });
}

#[test]
fn unknown_param_values_render_as_their_file_spelling() {
    assert_eq!(value_json(&ParamValue::Float(0.5)), json!(0.5));
    assert_eq!(
        value_json(&ParamValue::Color([1.0, 0.0, 0.0, 1.0])),
        json!([1.0, 0.0, 0.0, 1.0])
    );
}
