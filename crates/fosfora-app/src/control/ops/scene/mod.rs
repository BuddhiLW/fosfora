//! Scene ops: the Scenes panel and the cue timeline. Stub until filled;
//! follow the shape of `ops::preset` (port, Promote, Pipeline, Boundary).

use crate::control::{CljrsNs, ControlAddon, OpSpec};

pub struct Addon;

impl<C: 'static> ControlAddon<C> for Addon {
    fn id(&self) -> &'static str {
        "scene"
    }

    fn ops(&self) -> Vec<OpSpec<C>> {
        Vec::new()
    }

    fn cljrs(&self) -> Option<CljrsNs> {
        Some(CljrsNs {
            ns: "fosfora.scene",
            alias: "scene",
            src: include_str!("scene.cljrs"),
        })
    }
}
