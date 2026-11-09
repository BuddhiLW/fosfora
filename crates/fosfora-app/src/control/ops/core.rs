//! `ops`, `addons`: what the registry holds. Generic over any [`OpHost`].

use serde_json::{Value, json};

use crate::control::{ControlAddon, OpHost, OpResult, OpSpec};

pub struct Addon;

impl<C: OpHost + 'static> ControlAddon<C> for Addon {
    fn id(&self) -> &'static str {
        "core"
    }

    fn ops(&self) -> Vec<OpSpec<C>> {
        vec![
            OpSpec::new("ops", "{}", "Every registered op: name, addon, args, doc.", ops::<C>),
            OpSpec::new("addons", "{}", "Ids of the registered addons.", addons::<C>),
        ]
    }
}

fn ops<C: OpHost>(host: &mut C, _: &Value) -> OpResult {
    Ok(host.registry().catalog())
}

fn addons<C: OpHost>(host: &mut C, _: &Value) -> OpResult {
    Ok(json!(host.registry().addon_ids()))
}
