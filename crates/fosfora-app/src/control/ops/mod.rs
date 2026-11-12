//! Built-in control addons, one per area of the UI.
//!
//! Shape of a domain module `<d>` (see `preset` for the reference one):
//!
//! - `port.rs`: `trait <D>Port`, the role-sized slice of the app this domain
//!   needs (ISP), with `impl <D>Port for App` as the only place that touches
//!   `App` internals (DIP).
//! - `mod.rs`: domain values parsed from args (Promote), pure planning
//!   (Pipeline), op bodies generic over `P: <D>Port` whose one effect is a
//!   port call (Boundary), and `impl<C: <D>Port> ControlAddon<C> for Addon`.
//! - `<d>.cljrs`: the `fosfora.<d>` REPL namespace wrapping the ops.
//! - tests: unit + seeded property tests against a stub port; the REPL
//!   integration test lives in `tests/repl_ops.rs`.
//!
//! To add an area: write the module, list it in [`builtin_addons`].

use super::ControlAddon;
use crate::app::App;

pub mod bindings;
pub mod core;
pub mod layer;
pub mod particle;
pub mod preset;
pub mod scene;
pub mod svg;
pub mod system;
pub mod trama;

/// Every built-in addon, in registration order.
pub fn builtin_addons() -> Vec<Box<dyn ControlAddon<App>>> {
    vec![
        Box::new(core::Addon),
        Box::new(preset::Addon),
        Box::new(scene::Addon),
        Box::new(layer::Addon),
        Box::new(particle::Addon),
        Box::new(svg::Addon),
        Box::new(trama::Addon),
        Box::new(bindings::Addon),
        Box::new(system::Addon),
    ]
}

/// The REPL namespaces of the built-in addons, in registration order.
pub fn builtin_cljrs() -> Vec<super::CljrsNs> {
    builtin_addons().iter().filter_map(|a| a.cljrs()).collect()
}
