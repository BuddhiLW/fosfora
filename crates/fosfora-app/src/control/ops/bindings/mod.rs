//! Binding ops: the Binding Matrix, audio, MIDI and OSC mappings. Stub until
//! filled; follow the shape of `ops::preset`.

use crate::control::{CljrsNs, ControlAddon, OpSpec};

pub struct Addon;

impl<C: 'static> ControlAddon<C> for Addon {
    fn id(&self) -> &'static str {
        "bindings"
    }

    fn ops(&self) -> Vec<OpSpec<C>> {
        Vec::new()
    }

    fn cljrs(&self) -> Option<CljrsNs> {
        Some(CljrsNs {
            ns: "fosfora.bindings",
            alias: "bind",
            src: include_str!("bindings.cljrs"),
        })
    }
}
