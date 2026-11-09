//! Pure parts of the trama ops: graphs and nodes as JSON, values from args,
//! node and effect lookup. No port, no app. Modulation edits: [`super::modedit`].

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::port::{ChainRef, EffectInfo};
use crate::control::args;
use crate::params::{ParamDef, ParamValue};
use crate::trama::effect::{EffectId, EffectKind};
use crate::trama::graph::{NodeGraph, Wire};
use crate::trama::node::{NodeId, NodeInstance, NodeKind};
use crate::trama::ser::ValueDoc;

// ── Names ───────────────────────────────────────────────────────────────────

/// `"master"`, or the layer index.
pub fn chain_json(chain: ChainRef) -> Value {
    match chain {
        ChainRef::Master => json!("master"),
        ChainRef::Layer(i) => json!(i),
    }
}

/// What a node is, as ops spell it.
pub fn kind_name(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::Source { .. } => "source",
        NodeKind::Effect { .. } => "effect",
        NodeKind::Feedback => "feedback",
        NodeKind::ChainInput => "layer-input",
        NodeKind::Anchor => "anchor",
        NodeKind::Output => "output",
    }
}

/// The name a node answers to in `:node "x"`: its effect id, else its kind.
pub fn node_label(kind: &NodeKind) -> String {
    match kind {
        NodeKind::Source { effect } | NodeKind::Effect { effect } => effect.0.clone(),
        other => kind_name(other).to_owned(),
    }
}

/// The title the canvas shows on a node.
pub fn node_title(kind: &NodeKind, effects: &[EffectInfo]) -> String {
    match kind {
        NodeKind::Source { effect } | NodeKind::Effect { effect } => effects
            .iter()
            .find(|e| e.id == effect.0)
            .map_or_else(|| format!("missing: {}", effect.0), |e| e.name.clone()),
        NodeKind::Feedback => "Feedback".into(),
        NodeKind::ChainInput => "Layer input".into(),
        NodeKind::Anchor => "Anchor".into(),
        NodeKind::Output => "Output".into(),
    }
}

// ── Graph as data ───────────────────────────────────────────────────────────

/// A value in the `.fio.json` spelling: number, bool, `[x y]`, `[r g b a]`.
pub fn value_json(v: &ParamValue) -> Value {
    serde_json::to_value(ValueDoc::from(v)).unwrap_or(Value::Null)
}

fn wire_json(w: &Wire) -> Value {
    json!({"from": w.from.0, "to": w.to.0, "input": w.to_input})
}

/// One node: identity, title, pins, bypass, position, whether it reaches
/// Output, its values and modulations by param name.
pub fn node_json(
    graph: &NodeGraph,
    node: &NodeInstance,
    effects: &[EffectInfo],
    pos: Option<[f32; 2]>,
) -> Value {
    let mut params = serde_json::Map::new();
    for (name, v) in &node.params.values {
        params.insert(name.clone(), value_json(v));
    }
    let mut mods = serde_json::Map::new();
    for m in &node.mods {
        mods.insert(
            m.param.clone(),
            serde_json::to_value(m.config).unwrap_or(Value::Null),
        );
    }
    json!({
        "id": node.id.0,
        "kind": kind_name(&node.kind),
        "label": node_label(&node.kind),
        "title": node_title(&node.kind, effects),
        "inputs": node.inputs,
        "bypass": node.bypass,
        "pos": pos,
        "live": graph.live_set().contains(&node.id),
        "params": params,
        "mods": mods,
    })
}

/// A whole chain: nodes, wires, whether it reaches Output (`contributes`).
/// `exists` is false for a layer with no chain yet (shown as a fresh one).
pub fn graph_json(
    chain: ChainRef,
    graph: &NodeGraph,
    exists: bool,
    effects: &[EffectInfo],
    pos: impl Fn(NodeId) -> Option<[f32; 2]>,
) -> Value {
    json!({
        "chain": chain_json(chain),
        "exists": exists,
        "contributes": graph.contributes(),
        "placed": graph.placed_nodes(),
        "output": graph.output_node().0,
        "nodes": graph.nodes().iter().map(|n| node_json(graph, n, effects, pos(n.id))).collect::<Vec<_>>(),
        "wires": graph.wires().iter().map(wire_json).collect::<Vec<_>>(),
    })
}

/// One row of `trama/chains`.
pub fn chain_summary(chain: ChainRef, name: &str, graph: Option<&NodeGraph>) -> Value {
    json!({
        "chain": chain_json(chain),
        "name": name,
        "exists": graph.is_some(),
        "nodes": graph.map_or(0, NodeGraph::placed_nodes),
        "active": graph.is_some_and(NodeGraph::contributes),
    })
}

/// A node's full parameter list: each manifest def with its current value,
/// whole-number flag and modulation; plus what feeds each input pin.
pub fn node_detail(
    graph: &NodeGraph,
    node: &NodeInstance,
    effects: &[EffectInfo],
    pos: Option<[f32; 2]>,
) -> Value {
    let integers: &[String] = match &node.kind {
        NodeKind::Source { effect } | NodeKind::Effect { effect } => effects
            .iter()
            .find(|e| e.id == effect.0)
            .map_or(&[], |e| e.integers.as_slice()),
        _ => &[],
    };
    let params: Vec<Value> = node
        .params
        .defs
        .iter()
        .map(|def| {
            let mut p = serde_json::to_value(def).unwrap_or_else(|_| json!({}));
            p["value"] = node.params.get(def.name()).map_or(Value::Null, value_json);
            p["integer"] = json!(integers.iter().any(|n| n == def.name()));
            p["mod"] = node
                .mods
                .iter()
                .find(|m| m.param == def.name())
                .and_then(|m| serde_json::to_value(m.config).ok())
                .unwrap_or(Value::Null);
            p
        })
        .collect();
    let feeds: Vec<Value> = (0..node.inputs)
        .map(|i| json!({"input": i, "from": graph.input_source(node.id, i).map(|n| n.0)}))
        .collect();
    let mut v = node_json(graph, node, effects, pos);
    v["params"] = Value::Array(params);
    v["feeds"] = Value::Array(feeds);
    v
}

/// `trama/effects`: every effect, failed files, load directories.
pub fn effects_json(
    effects: &[EffectInfo],
    kind: Option<EffectKind>,
    errors: &[(String, String)],
    dirs: &[PathBuf],
) -> Value {
    let list: Vec<Value> = effects
        .iter()
        .filter(|e| kind.is_none_or(|k| e.kind == k))
        .map(|e| {
            json!({
                "id": e.id,
                "name": e.name,
                "kind": effect_kind_name(e.kind),
                "inputs": e.inputs,
                "params": e.params,
                "integers": e.integers,
                "error": e.error,
            })
        })
        .collect();
    json!({
        "effects": list,
        "errors": errors.iter().map(|(f, e)| json!({"file": f, "error": e})).collect::<Vec<_>>(),
        "dirs": dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>(),
    })
}

pub fn effect_kind_name(kind: EffectKind) -> &'static str {
    match kind {
        EffectKind::Source => "source",
        EffectKind::Effect => "effect",
    }
}

// ── Lookup ──────────────────────────────────────────────────────────────────

/// The node `a[key]` names: an id, or a label (`"kaleido"`, `"output"`,
/// a unique substring). A label several nodes share is refused.
pub fn node_ref(graph: &NodeGraph, a: &Value, key: &str) -> Result<NodeId, String> {
    match a.get(key) {
        Some(v) if v.is_u64() => {
            let id = NodeId(v.as_u64().unwrap_or_default());
            graph
                .node(id)
                .map(|_| id)
                .ok_or_else(|| format!("chain has no node {}", id.0))
        }
        Some(Value::String(s)) => {
            let labels: Vec<String> = graph.nodes().iter().map(|n| node_label(&n.kind)).collect();
            let exact: Vec<u64> = graph
                .nodes()
                .iter()
                .zip(&labels)
                .filter(|(_, l)| *l == s)
                .map(|(n, _)| n.id.0)
                .collect();
            if exact.len() > 1 {
                return Err(format!(
                    "several nodes are {s:?}; give the id, one of {exact:?}"
                ));
            }
            let i = args::pick(&json!({"name": s}), &labels, "node")?;
            Ok(graph.nodes()[i].id)
        }
        _ => Err(format!("give :{key} as a node id or a node name")),
    }
}

/// The effect `name` names: its exact id, else a name or id `pick` accepts.
pub fn effect_index(effects: &[EffectInfo], name: &str) -> Result<usize, String> {
    if let Some(i) = effects.iter().position(|e| e.id == name) {
        return Ok(i);
    }
    let names: Vec<&str> = effects.iter().map(|e| e.name.as_str()).collect();
    if let Ok(i) = args::pick(&json!({"name": name}), &names, "effect") {
        return Ok(i);
    }
    let ids: Vec<&str> = effects.iter().map(|e| e.id.as_str()).collect();
    args::pick(&json!({"name": name}), &ids, "effect")
}

/// What `trama/add` places.
#[derive(Debug, Clone, PartialEq)]
pub enum NewNode {
    Effect(usize),
    LayerInput,
    Feedback,
    Anchor,
}

impl NewNode {
    /// `{:effect "id-or-name"}` or `{:kind "layer-input"|"feedback"|"anchor"}`.
    pub fn from_args(a: &Value, effects: &[EffectInfo]) -> Result<Self, String> {
        if let Some(e) = args::opt_str(a, "effect") {
            return effect_index(effects, e).map(Self::Effect);
        }
        match args::opt_str(a, "kind").map(|k| k.to_ascii_lowercase().replace('_', "-")) {
            Some(k) if matches!(k.as_str(), "layer-input" | "input" | "chain-input") => {
                Ok(Self::LayerInput)
            }
            Some(k) if k == "feedback" => Ok(Self::Feedback),
            Some(k) if k == "anchor" => Ok(Self::Anchor),
            Some(k) => Err(format!(
                ":kind must be layer-input, feedback or anchor, got {k:?}"
            )),
            None => Err("give :effect (an effect id or name) or :kind".into()),
        }
    }

    /// The node kind, pin count and param defs to add.
    pub fn spec(&self, effects: &[EffectInfo]) -> (NodeKind, u8, Vec<ParamDef>) {
        match self {
            Self::Effect(i) => {
                let e = &effects[*i];
                let effect = EffectId(e.id.clone());
                let kind = match e.kind {
                    EffectKind::Source => NodeKind::Source { effect },
                    EffectKind::Effect => NodeKind::Effect { effect },
                };
                (kind, e.inputs, e.params.clone())
            }
            Self::LayerInput => (NodeKind::ChainInput, 0, Vec::new()),
            Self::Feedback => (NodeKind::Feedback, 1, Vec::new()),
            Self::Anchor => (NodeKind::Anchor, 1, Vec::new()),
        }
    }
}

/// Add a node as the canvas menu does: at most one layer input per chain.
pub fn add_node(
    graph: &mut NodeGraph,
    kind: NodeKind,
    inputs: u8,
    defs: &[ParamDef],
) -> Result<NodeId, String> {
    if matches!(kind, NodeKind::ChainInput) && graph.chain_input().is_some() {
        return Err("this chain already has its layer input".into());
    }
    Ok(graph.add_node(kind, inputs, defs))
}

/// The wires a disconnect removes: into `to`'s pin `input`, into every pin
/// of `to`, or out of `from`.
pub fn wires_to_drop(
    graph: &NodeGraph,
    from: Option<NodeId>,
    to: Option<NodeId>,
    input: Option<u8>,
) -> Vec<Wire> {
    graph
        .wires()
        .iter()
        .filter(|w| from.is_none_or(|f| w.from == f))
        .filter(|w| to.is_none_or(|t| w.to == t))
        .filter(|w| input.is_none_or(|i| w.to_input == i))
        .copied()
        .collect()
}

pub fn wires_json(wires: &[Wire]) -> Value {
    Value::Array(wires.iter().map(wire_json).collect())
}

// ── Values ──────────────────────────────────────────────────────────────────

fn floats(v: &Value, n: usize) -> Option<Vec<f32>> {
    let items = v.as_array().filter(|a| a.len() == n)?;
    items.iter().map(|x| x.as_f64().map(|f| f as f32)).collect()
}

fn type_hint(def: &ParamDef) -> &'static str {
    match def {
        ParamDef::Float { .. } => "a number",
        ParamDef::Bool { .. } => "true or false",
        ParamDef::Point2D { .. } => "[x y]",
        ParamDef::Color { .. } => "[r g b] or [r g b a]",
    }
}

/// `v` as a value for `def`, refusing a wrong shape or a non-finite number.
pub fn param_value(def: &ParamDef, v: &Value) -> Result<ParamValue, String> {
    let value = match def {
        ParamDef::Float { .. } => v.as_f64().map(|f| ParamValue::Float(f as f32)),
        ParamDef::Bool { .. } => v.as_bool().map(ParamValue::Bool),
        ParamDef::Point2D { .. } => floats(v, 2).map(|p| ParamValue::Point2D([p[0], p[1]])),
        ParamDef::Color { .. } => floats(v, 4)
            .or_else(|| {
                floats(v, 3).map(|mut c| {
                    c.push(1.0);
                    c
                })
            })
            .map(|c| ParamValue::Color([c[0], c[1], c[2], c[3]])),
    };
    let value =
        value.ok_or_else(|| format!("`{}` takes {}, got {v}", def.name(), type_hint(def)))?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(format!("`{}` must be finite", def.name()))
    }
}

/// `"~/x"` against `home`; anything else as given.
pub fn expand_home(raw: &str, home: Option<&Path>) -> PathBuf {
    match (raw.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(raw),
    }
}

/// [`expand_home`], plus the `.fio.json` extension when there is none.
pub fn export_path(raw: &str, home: Option<&Path>) -> PathBuf {
    let mut path = expand_home(raw, home);
    if path.extension().is_none() {
        path.set_extension("fio.json");
    }
    path
}
