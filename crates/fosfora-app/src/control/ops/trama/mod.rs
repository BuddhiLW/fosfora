//! Trama ops: the effect chain on each layer and on the master, edited as
//! the chain editor edits it (nodes, wires, bypass, params, modulation,
//! positions, Import/Export), plus the editor window and the effect library.
//!
//! Promote (`ChainRef` from `:chain`, nodes, values) -> Pipeline (pure, in
//! [`pipeline`]) -> Boundary (one [`TramaPort`] call that acts per op).
//! Every op takes `:chain` ("master", a layer index or a layer name);
//! without it, the chain the editor is pointed at.
#![allow(
    clippy::unnecessary_wraps,
    reason = "every op body has the OpFn signature, fallible or not"
)]

mod actions;
pub mod modedit;
pub mod pipeline;
pub mod port;
mod promote;
#[cfg(test)]
mod tests;

use serde_json::{Value, json};

use self::modedit::modulation_edit;
use self::pipeline::{
    NewNode, add_node, chain_json, chain_summary, effects_json, expand_home, export_path,
    graph_json, node_detail, node_json, node_ref, param_value, wires_json, wires_to_drop,
};
pub use self::port::{ChainPort, ChainRef, EditorPort, EditorState, EffectInfo, TramaPort};
pub use self::promote::{chain_ref, default_chain};
use self::promote::{home, input_arg, node_target, param_def, pos_arg, target, with_graph};
use crate::control::{CljrsNs, ControlAddon, OpResult, OpSpec, args};
use crate::params::ParamDef;
use crate::trama::effect::{EffectId, EffectKind};
use crate::trama::graph::NodeGraph;
use crate::trama::node::{NodeId, NodeKind};
use crate::trama::ser::ChainDoc;

pub struct Addon;

impl<C: TramaPort + 'static> ControlAddon<C> for Addon {
    fn id(&self) -> &'static str {
        "trama"
    }

    fn ops(&self) -> Vec<OpSpec<C>> {
        vec![
            OpSpec::new(
                "trama/chains",
                "{}",
                "Every chain: master and one per layer, with node count and active?.",
                chains::<C>,
            ),
            OpSpec::new(
                "trama/graph",
                "{:chain?}",
                "A chain as data: nodes (params, mods, pos, live?), wires, contributes?.",
                graph::<C>,
            ),
            OpSpec::new(
                "trama/doc",
                "{:chain?}",
                "A chain as its exact .fio.json document (trama/import takes it back).",
                doc::<C>,
            ),
            OpSpec::new(
                "trama/node",
                "{:chain?, :node id|str}",
                "One node: every param's def, value and mod, and what feeds each input.",
                node::<C>,
            ),
            OpSpec::new(
                "trama/effects",
                "{:kind source|effect?}",
                "Installed trama effects, failed files, and the directories they load from.",
                effects::<C>,
            ),
            OpSpec::new(
                "trama/editor",
                "{}",
                "The chain editor: open?, tab, selected layer, the chain it edits.",
                editor::<C>,
            ),
            OpSpec::new(
                "trama/open",
                "{:chain?}",
                "Open the chain editor on a layer's chain (selecting the layer) or the master.",
                open::<C>,
            ),
            OpSpec::new("trama/close", "{}", "Close the chain editor.", close::<C>),
            OpSpec::new(
                "trama/add",
                "{:chain?, :effect str | :kind layer-input|feedback|anchor, :pos [x y]?}",
                "Place a node (an installed effect, or a utility node); returns it.",
                add::<C>,
            ),
            OpSpec::new(
                "trama/remove",
                "{:chain?, :node id|str}",
                "Delete a node and its wires (not Output).",
                remove::<C>,
            ),
            OpSpec::new(
                "trama/connect",
                "{:chain?, :from node, :to node, :input int?}",
                "Wire from's output into to's input pin, replacing that pin's wire.",
                connect::<C>,
            ),
            OpSpec::new(
                "trama/disconnect",
                "{:chain?, :to node, :input int? | :from node}",
                "Remove the wires into a pin, into a node, or out of a node.",
                disconnect::<C>,
            ),
            OpSpec::new(
                "trama/move",
                "{:chain?, :node id|str, :pos [x y]}",
                "Move a node on the canvas.",
                move_node::<C>,
            ),
            OpSpec::new(
                "trama/bypass",
                "{:chain?, :node id|str, :on bool?}",
                "Bypass a node (toggles without :on).",
                bypass::<C>,
            ),
            OpSpec::new(
                "trama/set",
                "{:chain?, :node id|str, :param str, :value v | :reset true}",
                "Set a node param (number, bool, [x y], [r g b a]) or reset it to its default.",
                set::<C>,
            ),
            OpSpec::new(
                "trama/mod",
                "{:chain?, :node, :param, :off true | :source str|map, :amount, :mode, :smoothing, :rate hz|div, :phase}",
                "Add, change or remove a number param's modulation; returns it.",
                modulate::<C>,
            ),
            OpSpec::new(
                "trama/clear",
                "{:chain?}",
                "Empty a chain back to its Output node.",
                clear::<C>,
            ),
            OpSpec::new(
                "trama/seed",
                "{:chain?, :effect str?}",
                "Give an empty chain the starter patch: layer input -> effect -> Output.",
                seed::<C>,
            ),
            OpSpec::new(
                "trama/export",
                "{:chain?, :path str}",
                "Save a chain as a .fio.json file.",
                export::<C>,
            ),
            OpSpec::new(
                "trama/import",
                "{:chain?, :path str | :doc map}",
                "Replace a chain with a .fio.json file or document; returns repairs and the graph.",
                import::<C>,
            ),
        ]
    }

    fn ui_signals(&self) -> &'static [(&'static str, &'static str)] {
        &[
            ("open_trama_on_layer", "trama/open"),
            ("open_trama_on_master", "trama/open"),
        ]
    }

    fn cljrs(&self) -> Option<CljrsNs> {
        Some(CljrsNs {
            ns: "fosfora.trama",
            alias: "trama",
            src: include_str!("trama.cljrs"),
        })
    }
}

// ── Boundary ────────────────────────────────────────────────────────────────

/// The chain as `trama/graph` returns it.
fn view<P: ChainPort>(port: &P, chain: ChainRef) -> Value {
    let effects = port.trama_effects();
    let exists = port.chain_graph(chain).is_some();
    with_graph(port, chain, |g| {
        graph_json(chain, g, exists, &effects, |n| port.node_pos(chain, n))
    })
}

/// One node as `trama/add` and `trama/remove` return it.
fn node_view<P: ChainPort>(port: &P, chain: ChainRef, node: NodeId) -> Value {
    let effects = port.trama_effects();
    with_graph(port, chain, |g| match g.node(node) {
        Some(n) => {
            let mut v = node_json(g, n, &effects, port.node_pos(chain, node));
            v["chain"] = chain_json(chain);
            v
        }
        None => Value::Null,
    })
}

fn capture<P: ChainPort>(port: &P, chain: ChainRef) -> ChainDoc {
    with_graph(port, chain, |g| {
        ChainDoc::capture(g, |n| port.node_pos(chain, n))
    })
}

fn chains<P: TramaPort>(port: &mut P, _: &Value) -> OpResult {
    let mut out = vec![chain_summary(
        ChainRef::Master,
        "master",
        port.chain_graph(ChainRef::Master),
    )];
    for (i, name) in port.layer_names().iter().enumerate() {
        out.push(chain_summary(
            ChainRef::Layer(i),
            name,
            port.chain_graph(ChainRef::Layer(i)),
        ));
    }
    Ok(Value::Array(out))
}

fn graph<P: TramaPort>(port: &mut P, a: &Value) -> OpResult {
    let chain = target(port, a)?;
    Ok(view(port, chain))
}

fn doc<P: TramaPort>(port: &mut P, a: &Value) -> OpResult {
    let chain = target(port, a)?;
    serde_json::to_value(capture(port, chain)).map_err(|e| e.to_string())
}

fn node<P: TramaPort>(port: &mut P, a: &Value) -> OpResult {
    let (chain, id) = node_target(port, a, "node")?;
    let effects = port.trama_effects();
    let pos = port.node_pos(chain, id);
    let mut v = with_graph(port, chain, |g| {
        g.node(id)
            .map_or(Value::Null, |n| node_detail(g, n, &effects, pos))
    });
    v["chain"] = chain_json(chain);
    Ok(v)
}

fn effects<P: TramaPort>(port: &mut P, a: &Value) -> OpResult {
    let kind = match args::opt_str(a, "kind") {
        None => None,
        Some(k) if k.eq_ignore_ascii_case("source") => Some(EffectKind::Source),
        Some(k) if k.eq_ignore_ascii_case("effect") => Some(EffectKind::Effect),
        Some(k) => return Err(format!(":kind must be source or effect, got {k:?}")),
    };
    Ok(effects_json(
        &port.trama_effects(),
        kind,
        &port.trama_errors(),
        &port.trama_dirs(),
    ))
}

fn editor_json<P: TramaPort>(port: &P) -> Value {
    let e = port.editor();
    let layers = port.layer_names().len();
    json!({
        "open": e.open,
        "tab": if e.on_master { "master" } else { "layer" },
        "layer": e.selected_layer,
        "chain": chain_json(default_chain(&e, layers)),
    })
}

fn editor<P: TramaPort>(port: &mut P, _: &Value) -> OpResult {
    Ok(editor_json(port))
}

fn open<P: TramaPort>(port: &mut P, a: &Value) -> OpResult {
    let chain = target(port, a)?;
    port.open_editor(chain)?;
    Ok(editor_json(port))
}

fn close<P: TramaPort>(port: &mut P, _: &Value) -> OpResult {
    port.close_editor();
    Ok(editor_json(port))
}

fn add<P: TramaPort>(port: &mut P, a: &Value) -> OpResult {
    let chain = target(port, a)?;
    let effects = port.trama_effects();
    let (kind, inputs, defs) = NewNode::from_args(a, &effects)?.spec(&effects);
    let pos = pos_arg(a)?;
    let id = port.edit_chain(chain, |g| add_node(g, kind, inputs, &defs))?;
    if let Some(p) = pos {
        port.place_nodes(chain, &[(id, p)])?;
    }
    Ok(node_view(port, chain, id))
}

fn remove<P: TramaPort>(port: &mut P, a: &Value) -> OpResult {
    let (chain, id) = node_target(port, a, "node")?;
    let gone = node_view(port, chain, id);
    port.edit_chain(chain, |g| g.remove_node(id).map_err(|e| e.to_string()))?;
    Ok(gone)
}

fn connect<P: TramaPort>(port: &mut P, a: &Value) -> OpResult {
    let (chain, from) = node_target(port, a, "from")?;
    let to = with_graph(port, chain, |g| node_ref(g, a, "to"))?;
    let input = input_arg(a)?.unwrap_or(0);
    let contributes = port.edit_chain(chain, |g| {
        g.connect(from, to, input).map_err(|e| e.to_string())?;
        Ok(g.contributes())
    })?;
    Ok(
        json!({"chain": chain_json(chain), "from": from.0, "to": to.0, "input": input, "contributes": contributes}),
    )
}

fn disconnect<P: TramaPort>(port: &mut P, a: &Value) -> OpResult {
    let chain = target(port, a)?;
    let input = input_arg(a)?;
    let (from, to) = with_graph(port, chain, |g| {
        let from = a.get("from").map(|_| node_ref(g, a, "from")).transpose()?;
        let to = a.get("to").map(|_| node_ref(g, a, "to")).transpose()?;
        Ok::<_, String>((from, to))
    })?;
    if from.is_none() && to.is_none() {
        return Err("give :to (and optionally :input) or :from".into());
    }
    let dropped = port.edit_chain(chain, |g| {
        let dropped = wires_to_drop(g, from, to, input);
        for w in &dropped {
            g.disconnect(w.to, w.to_input);
        }
        Ok(dropped)
    })?;
    Ok(json!({"chain": chain_json(chain), "removed": wires_json(&dropped)}))
}

fn move_node<P: TramaPort>(port: &mut P, a: &Value) -> OpResult {
    let (chain, id) = node_target(port, a, "node")?;
    let pos = pos_arg(a)?.ok_or("missing :pos [x y]")?;
    port.place_nodes(chain, &[(id, pos)])?;
    Ok(json!({"chain": chain_json(chain), "node": id.0, "pos": pos}))
}

fn bypass<P: TramaPort>(port: &mut P, a: &Value) -> OpResult {
    let (chain, id) = node_target(port, a, "node")?;
    let on = args::opt_bool(a, "on");
    let now = port.edit_chain(chain, |g| {
        let node = g.node(id).ok_or("unknown node")?;
        if matches!(node.kind, NodeKind::Output) {
            return Err("the Output node cannot be bypassed".into());
        }
        let next = on.unwrap_or(!node.bypass);
        g.set_bypass(id, next).map_err(|e| e.to_string())?;
        Ok(next)
    })?;
    Ok(json!({"chain": chain_json(chain), "node": id.0, "bypass": now}))
}

fn set<P: TramaPort>(port: &mut P, a: &Value) -> OpResult {
    let (chain, id) = node_target(port, a, "node")?;
    let def = param_def(port, chain, id, a)?;
    let value = if args::opt_bool(a, "reset") == Some(true) {
        def.default_value()
    } else {
        param_value(&def, a.get("value").ok_or("give :value, or :reset true")?)?
    };
    let shown = pipeline::value_json(&value);
    port.edit_chain(chain, |g| {
        let n = g.params_mut(id).ok_or("unknown node")?;
        n.params.set(def.name(), value);
        Ok(())
    })?;
    Ok(json!({"chain": chain_json(chain), "node": id.0, "param": def.name(), "value": shown}))
}

fn modulate<P: TramaPort>(port: &mut P, a: &Value) -> OpResult {
    let (chain, id) = node_target(port, a, "node")?;
    let def = param_def(port, chain, id, a)?;
    if !matches!(def, ParamDef::Float { .. }) {
        return Err(format!(
            "only number params can be modulated; `{}` is not one",
            def.name()
        ));
    }
    let name = def.name();
    let current = with_graph(port, chain, |g| {
        g.node(id)
            .and_then(|n| n.mods.iter().find(|m| m.param == name).map(|m| m.config))
    });
    let next = modulation_edit(current, a)?;
    port.edit_chain(chain, |g| {
        g.set_modulation(id, name, next).map_err(|e| e.to_string())
    })?;
    let m = next.map_or(Value::Null, |m| {
        serde_json::to_value(m).unwrap_or(Value::Null)
    });
    Ok(json!({"chain": chain_json(chain), "node": id.0, "param": name, "mod": m}))
}

fn clear<P: TramaPort>(port: &mut P, a: &Value) -> OpResult {
    let chain = target(port, a)?;
    if port.chain_graph(chain).is_some() {
        let empty = ChainDoc::capture(&NodeGraph::new_with_output(), |_| None);
        port.replace_chain(chain, &empty)?;
    }
    Ok(view(port, chain))
}

fn seed<P: TramaPort>(port: &mut P, a: &Value) -> OpResult {
    use crate::trama::starter;
    let chain = target(port, a)?;
    let effects = port.trama_effects();
    let usable = |e: &EffectInfo| e.kind == EffectKind::Effect && e.inputs >= 1;
    let pick = match args::opt_str(a, "effect") {
        Some(name) => {
            let i = pipeline::effect_index(&effects, name)?;
            Some(i).filter(|&i| usable(&effects[i]))
        }
        None => effects
            .iter()
            .position(|e| e.id == starter::STARTER_EFFECT && usable(e))
            .or_else(|| effects.iter().position(|e| usable(e) && e.inputs == 1)),
    };
    let e = &effects[pick.ok_or("no installed effect with an input to seed with")?];
    let (effect, params) = (EffectId(e.id.clone()), e.params.clone());
    let st = port.edit_chain(chain, |g| {
        starter::seed(g, &effect, &params).ok_or_else(|| "the chain is not empty".to_owned())
    })?;
    port.place_nodes(chain, &st.layout)?;
    Ok(view(port, chain))
}

fn export<P: TramaPort>(port: &mut P, a: &Value) -> OpResult {
    let chain = target(port, a)?;
    let path = export_path(args::str(a, "path")?, home().as_deref());
    let doc = capture(port, chain);
    crate::paths::write_atomic(&path, &doc.to_json())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(
        json!({"chain": chain_json(chain), "path": path.display().to_string(), "nodes": doc.graph.nodes.len()}),
    )
}

fn import<P: TramaPort>(port: &mut P, a: &Value) -> OpResult {
    let chain = target(port, a)?;
    let doc = match (args::opt_str(a, "path"), a.get("doc")) {
        (Some(p), _) => {
            let path = expand_home(p, home().as_deref());
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            ChainDoc::from_json(&text).map_err(|e| e.to_string())?
        }
        (None, Some(d)) => {
            serde_json::from_value(d.clone()).map_err(|e| format!("not a trama chain: {e}"))?
        }
        (None, None) => return Err("give :path or :doc".into()),
    };
    let notes = port.replace_chain(chain, &doc)?;
    Ok(json!({"notes": notes, "graph": view(port, chain)}))
}
