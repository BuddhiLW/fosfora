//! Promote: `:chain`, nodes, positions, pins and params from op args.

use serde_json::{Value, json};

use super::pipeline::node_ref;
use super::port::{ChainPort, ChainRef, EditorState, TramaPort};
use crate::control::args;
use crate::params::ParamDef;
use crate::trama::graph::NodeGraph;
use crate::trama::node::NodeId;

/// The chain the editor edits: the master tab, else the selected layer's
/// (the master when there are no layers).
pub fn default_chain(editor: &EditorState, layers: usize) -> ChainRef {
    if editor.on_master || editor.selected_layer >= layers {
        ChainRef::Master
    } else {
        ChainRef::Layer(editor.selected_layer)
    }
}

/// `:chain` as "master", a layer index or a layer name; absent, `fallback`.
pub fn chain_ref(a: &Value, layers: &[String], fallback: ChainRef) -> Result<ChainRef, String> {
    match a.get("chain") {
        None | Some(Value::Null) => Ok(fallback),
        Some(Value::String(s)) if s.eq_ignore_ascii_case("master") => Ok(ChainRef::Master),
        Some(n @ Value::Number(_)) => {
            args::pick(&json!({"index": n}), layers, "layer").map(ChainRef::Layer)
        }
        Some(Value::String(s)) => {
            args::pick(&json!({"name": s}), layers, "layer").map(ChainRef::Layer)
        }
        Some(other) => Err(format!(
            ":chain must be \"master\", a layer index or a layer name, got {other}"
        )),
    }
}

pub(super) fn target<P: TramaPort>(port: &P, a: &Value) -> Result<ChainRef, String> {
    let layers = port.layer_names();
    chain_ref(a, &layers, default_chain(&port.editor(), layers.len()))
}

pub(super) fn pos_arg(a: &Value) -> Result<Option<[f32; 2]>, String> {
    match a.get("pos") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let xy: Option<Vec<f32>> = v
                .as_array()
                .filter(|p| p.len() == 2)
                .and_then(|p| p.iter().map(|x| x.as_f64().map(|f| f as f32)).collect());
            match xy {
                Some(p) if p.iter().all(|c| c.is_finite()) => Ok(Some([p[0], p[1]])),
                _ => Err(format!(":pos must be [x y], got {v}")),
            }
        }
    }
}

pub(super) fn input_arg(a: &Value) -> Result<Option<u8>, String> {
    match a.get("input") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|i| u8::try_from(i).ok())
            .map(Some)
            .ok_or_else(|| format!(":input must be a pin number, got {v}")),
    }
}

/// Read `chain`'s graph; a fresh one for a layer that has none yet.
pub(super) fn with_graph<P: ChainPort, R>(
    port: &P,
    chain: ChainRef,
    f: impl FnOnce(&NodeGraph) -> R,
) -> R {
    match port.chain_graph(chain) {
        Some(g) => f(g),
        None => f(&NodeGraph::new_with_output()),
    }
}

/// `(chain, node)` from `:chain` and `a[key]`.
pub(super) fn node_target<P: TramaPort>(
    port: &P,
    a: &Value,
    key: &str,
) -> Result<(ChainRef, NodeId), String> {
    let chain = target(port, a)?;
    let node = with_graph(port, chain, |g| node_ref(g, a, key))?;
    Ok((chain, node))
}

/// The manifest def of `node`'s param `:param`.
pub(super) fn param_def<P: ChainPort>(
    port: &P,
    chain: ChainRef,
    node: NodeId,
    a: &Value,
) -> Result<ParamDef, String> {
    let name = args::str(a, "param")?;
    with_graph(port, chain, |g| {
        let n = g.node(node).ok_or("unknown node")?;
        n.params
            .defs
            .iter()
            .find(|d| d.name() == name)
            .cloned()
            .ok_or_else(|| {
                let have: Vec<&str> = n.params.defs.iter().map(ParamDef::name).collect();
                format!("node {} has no param {name:?} (has {have:?})", node.0)
            })
    })
}

pub(super) fn home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}
