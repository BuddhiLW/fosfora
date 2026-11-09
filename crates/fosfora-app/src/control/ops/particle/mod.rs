//! Particle ops: the particle, lattice, helix, obstacle, morph and splat
//! panels. Stub until filled; follow the shape of `ops::preset`.

use crate::control::{CljrsNs, ControlAddon, OpSpec};

pub struct Addon;

impl<C: 'static> ControlAddon<C> for Addon {
    fn id(&self) -> &'static str {
        "particle"
    }

    fn ops(&self) -> Vec<OpSpec<C>> {
        Vec::new()
    }

    fn cljrs(&self) -> Option<CljrsNs> {
        Some(CljrsNs {
            ns: "fosfora.particle",
            alias: "particle",
            src: include_str!("particle.cljrs"),
        })
    }
}
