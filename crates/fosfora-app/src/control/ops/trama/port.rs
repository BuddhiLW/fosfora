//! What the trama ops need from the app (ISP), and the one adapter that
//! reaches into [`App`] for it (DIP). Two roles: the chains themselves
//! ([`ChainPort`]) and the chain editor window ([`EditorPort`]).

use std::path::PathBuf;

use crate::app::App;
use crate::params::ParamDef;
use crate::trama::effect::EffectKind;
use crate::trama::graph::NodeGraph;
use crate::trama::node::{ChainId, NodeId};
use crate::trama::ser::ChainDoc;
use crate::trama::{CanvasTarget, TramaSystem};

/// One chain, as an op names it: the master, or layer `n` by stack position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChainRef {
    Master,
    Layer(usize),
}

/// An installed trama effect, without its GPU pipeline.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectInfo {
    pub id: String,
    pub name: String,
    pub kind: EffectKind,
    pub inputs: u8,
    pub params: Vec<ParamDef>,
    /// Float params that take whole numbers only.
    pub integers: Vec<String>,
    /// The file no longer compiles; the last good version still runs.
    pub error: Option<String>,
}

/// The chain editor window: open or not, which tab, which layer is selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EditorState {
    pub open: bool,
    pub on_master: bool,
    pub selected_layer: usize,
}

/// The chains: every layer's and the master's graph, their canvas
/// positions, and the installed effects nodes are made from.
pub trait ChainPort {
    /// Each layer's display name, in stack order.
    fn layer_names(&self) -> Vec<String>;
    /// The graph of `chain`; `None` for a layer that has no chain yet.
    fn chain_graph(&self, chain: ChainRef) -> Option<&NodeGraph>;
    /// Where `node` sits on the canvas, if known.
    fn node_pos(&self, chain: ChainRef, node: NodeId) -> Option<[f32; 2]>;
    /// Run `edit` on `chain`'s graph, giving a layer a chain first if it has
    /// none. On `Ok` the edit counts as an unsaved change; a structural edit
    /// also rebuilds the canvas view so it shows the new graph.
    fn edit_chain<R>(
        &mut self,
        chain: ChainRef,
        edit: impl FnOnce(&mut NodeGraph) -> Result<R, String>,
    ) -> Result<R, String>;
    /// Move nodes on the canvas. Every node must exist in `chain`.
    fn place_nodes(&mut self, chain: ChainRef, placed: &[(NodeId, [f32; 2])])
    -> Result<(), String>;
    /// Replace `chain` with `doc` (Import); what had to be repaired, in words.
    fn replace_chain(&mut self, chain: ChainRef, doc: &ChainDoc) -> Result<Vec<String>, String>;
    /// Every installed effect, sorted by id.
    fn trama_effects(&self) -> Vec<EffectInfo>;
    /// `(file, error)` for each effect file that failed to load.
    fn trama_errors(&self) -> Vec<(String, String)>;
    /// The directories effects load from, in load order.
    fn trama_dirs(&self) -> Vec<PathBuf>;
}

/// The chain editor window.
pub trait EditorPort {
    fn editor(&self) -> EditorState;
    /// Open the editor on `chain`; a layer is selected first. Refused for a
    /// layer that does not exist.
    fn open_editor(&mut self, chain: ChainRef) -> Result<(), String>;
    fn close_editor(&mut self);
}

/// Both trama roles; what the trama ops are generic over.
pub trait TramaPort: ChainPort + EditorPort {}
impl<T: ChainPort + EditorPort> TramaPort for T {}

impl App {
    /// The slot of `chain`, without creating one.
    fn trama_chain_id(&self, chain: ChainRef) -> Option<ChainId> {
        match chain {
            ChainRef::Master => Some(ChainId::Master),
            ChainRef::Layer(i) => self.layer_stack.layers.get(i)?.chain.as_ref().map(|c| c.id),
        }
    }

    /// The slot of `chain`, giving a layer a chain if it has none.
    fn trama_ensure_chain_id(&mut self, chain: ChainRef) -> Result<ChainId, String> {
        match chain {
            ChainRef::Master => Ok(ChainId::Master),
            ChainRef::Layer(i) if i >= self.layer_stack.layers.len() => Err(format!(
                "layer index {i} out of range (have {})",
                self.layer_stack.layers.len()
            )),
            ChainRef::Layer(i) => self
                .layer_stack
                .ensure_chain(i)
                .ok_or_else(|| format!("layer {i} has no chain and no chain slot is free")),
        }
    }
}

/// Rebuild the canvas view of `id` from the positions it knows, `placed`
/// winning; a node with no position gets the automatic layout.
fn relayout(trama: &mut TramaSystem, id: ChainId, nodes: &[NodeId], placed: &[(NodeId, [f32; 2])]) {
    let layout = nodes
        .iter()
        .filter_map(|&n| {
            placed
                .iter()
                .find(|(p, _)| *p == n)
                .map(|(_, pos)| *pos)
                .or_else(|| trama.canvas.position(id, n))
                .map(|pos| (n, pos))
        })
        .collect();
    trama.canvas.replace_chain(id, layout);
}

impl ChainPort for App {
    fn layer_names(&self) -> Vec<String> {
        self.layer_stack
            .layers
            .iter()
            .map(|l| l.custom_name.clone().unwrap_or_else(|| l.name.clone()))
            .collect()
    }

    fn chain_graph(&self, chain: ChainRef) -> Option<&NodeGraph> {
        match chain {
            ChainRef::Master => Some(&self.trama.master),
            ChainRef::Layer(i) => self
                .layer_stack
                .layers
                .get(i)?
                .chain
                .as_deref()
                .map(|c| &c.graph),
        }
    }

    fn node_pos(&self, chain: ChainRef, node: NodeId) -> Option<[f32; 2]> {
        self.trama
            .canvas
            .position(self.trama_chain_id(chain)?, node)
    }

    fn edit_chain<R>(
        &mut self,
        chain: ChainRef,
        edit: impl FnOnce(&mut NodeGraph) -> Result<R, String>,
    ) -> Result<R, String> {
        let id = self.trama_ensure_chain_id(chain)?;
        let (out, moved, nodes) = {
            let graph = match chain {
                ChainRef::Master => &mut self.trama.master,
                ChainRef::Layer(i) => {
                    &mut self.layer_stack.layers[i]
                        .chain
                        .as_deref_mut()
                        .expect("trama_ensure_chain_id gave it one")
                        .graph
                }
            };
            let before = graph.version();
            let out = edit(graph);
            let nodes: Vec<NodeId> = graph.nodes().iter().map(|n| n.id).collect();
            (out, graph.version() != before, nodes)
        };
        if moved {
            relayout(&mut self.trama, id, &nodes, &[]);
        }
        if out.is_ok() {
            self.trama.mark_edited();
        }
        out
    }

    fn place_nodes(
        &mut self,
        chain: ChainRef,
        placed: &[(NodeId, [f32; 2])],
    ) -> Result<(), String> {
        let id = self
            .trama_chain_id(chain)
            .ok_or("that layer has no chain yet")?;
        let graph = self
            .chain_graph(chain)
            .ok_or("that layer has no chain yet")?;
        if let Some((missing, _)) = placed.iter().find(|(n, _)| graph.node(*n).is_none()) {
            return Err(format!("chain has no node {}", missing.0));
        }
        let nodes: Vec<NodeId> = graph.nodes().iter().map(|n| n.id).collect();
        relayout(&mut self.trama, id, &nodes, placed);
        self.trama.mark_edited();
        Ok(())
    }

    fn replace_chain(&mut self, chain: ChainRef, doc: &ChainDoc) -> Result<Vec<String>, String> {
        let id = self.trama_ensure_chain_id(chain)?;
        let notes =
            crate::trama::persist::load_into(&mut self.layer_stack, &mut self.trama, id, doc)?;
        self.trama.mark_edited();
        Ok(notes)
    }

    fn trama_effects(&self) -> Vec<EffectInfo> {
        self.trama
            .registry
            .effects
            .iter()
            .map(|e| EffectInfo {
                id: e.id.0.clone(),
                name: e.name.clone(),
                kind: e.kind,
                inputs: e.inputs,
                params: e.params.clone(),
                integers: e.integers.clone(),
                error: e.error.clone(),
            })
            .collect()
    }

    fn trama_errors(&self) -> Vec<(String, String)> {
        self.trama.registry.errors.clone()
    }

    fn trama_dirs(&self) -> Vec<PathBuf> {
        crate::trama::dirs::all_dirs()
    }
}

impl EditorPort for App {
    fn editor(&self) -> EditorState {
        EditorState {
            open: self.trama.canvas_open,
            on_master: self.trama.canvas_target == CanvasTarget::Master,
            selected_layer: self.layer_stack.active_layer,
        }
    }

    fn open_editor(&mut self, chain: ChainRef) -> Result<(), String> {
        match chain {
            ChainRef::Master => {
                self.open_trama_on_master();
                Ok(())
            }
            ChainRef::Layer(i) if self.open_trama_on_layer(i) => Ok(()),
            ChainRef::Layer(i) => Err(format!(
                "layer index {i} out of range (have {})",
                self.layer_stack.layers.len()
            )),
        }
    }

    fn close_editor(&mut self) {
        self.trama.canvas_open = false;
    }
}
