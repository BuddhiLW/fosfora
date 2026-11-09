//! Trama ops: effect chains per layer and on the master, node graphs and
//! their links. Stub until filled; follow the shape of `ops::preset`.

use crate::control::{CljrsNs, ControlAddon, OpSpec};

pub struct Addon;

impl<C: 'static> ControlAddon<C> for Addon {
    fn id(&self) -> &'static str {
        "trama"
    }

    fn ops(&self) -> Vec<OpSpec<C>> {
        Vec::new()
    }

    fn cljrs(&self) -> Option<CljrsNs> {
        Some(CljrsNs {
            ns: "fosfora.trama",
            alias: "trama",
            src: include_str!("trama.cljrs"),
        })
    }
}
