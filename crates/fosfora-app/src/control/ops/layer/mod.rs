//! Layer ops: the layer stack, effects, media and post-processing. Stub
//! until filled; follow the shape of `ops::preset`.

use crate::control::{CljrsNs, ControlAddon, OpSpec};

pub struct Addon;

impl<C: 'static> ControlAddon<C> for Addon {
    fn id(&self) -> &'static str {
        "layer"
    }

    fn ops(&self) -> Vec<OpSpec<C>> {
        Vec::new()
    }

    fn cljrs(&self) -> Option<CljrsNs> {
        Some(CljrsNs {
            ns: "fosfora.layer",
            alias: "layer",
            src: include_str!("layer.cljrs"),
        })
    }
}
