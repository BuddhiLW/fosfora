//! System ops: tempo and Link, recording, outputs, settings and the window.
//! Stub until filled; follow the shape of `ops::preset`.

use crate::control::{CljrsNs, ControlAddon, OpSpec};

pub struct Addon;

impl<C: 'static> ControlAddon<C> for Addon {
    fn id(&self) -> &'static str {
        "system"
    }

    fn ops(&self) -> Vec<OpSpec<C>> {
        Vec::new()
    }

    fn cljrs(&self) -> Option<CljrsNs> {
        Some(CljrsNs {
            ns: "fosfora.system",
            alias: "sys",
            src: include_str!("system.cljrs"),
        })
    }
}
