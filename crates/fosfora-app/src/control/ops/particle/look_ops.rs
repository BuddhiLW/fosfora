//! Boundary: the emission, quality, volumetric, lattice and helix ops. Each
//! makes at most one port call that acts, then answers with the new state.

use serde_json::{Value, json};

use super::pipeline::{patched, sim_json, to_json};
use super::port::ParticleSimPort;
use super::promote;
use crate::control::{OpResult, args};

fn no_particles(layer: usize) -> String {
    format!("layer {layer} has no particle system")
}

/// `particle/sim`: read, or change any subset of, the emission params.
pub fn sim<P: ParticleSimPort>(port: &mut P, a: &Value) -> OpResult {
    let layer = promote::layer(port, a)?;
    let edit = promote::sim_edit(a)?;
    if port.sim_state(layer).is_none() {
        return Err(no_particles(layer));
    }
    if !edit.is_empty() {
        port.apply_sim(layer, &edit)?;
    }
    let state = port.sim_state(layer).ok_or_else(|| no_particles(layer))?;
    Ok(sim_json(layer, &state))
}

/// `particle/quality`: read, or set, the particle quality.
pub fn quality<P: ParticleSimPort>(port: &mut P, a: &Value) -> OpResult {
    if let Some(raw) = args::opt_str(a, "value") {
        port.set_particle_quality(promote::quality(raw)?);
    } else if a.get("value").is_some_and(|v| !v.is_null()) {
        return Err(":value must be a quality name".into());
    }
    Ok(json!({"quality": promote::quality_name(port.particle_quality())}))
}

/// `particle/volumetric`: read, or change, the volumetric look.
pub fn volumetric<P: ParticleSimPort>(port: &mut P, a: &Value) -> OpResult {
    let enabled = promote::opt_flag(a, "enabled")?;
    let (was_on, was) = port.volumetric();
    let params = match a.get("params") {
        None | Some(Value::Null) => was,
        Some(p) => patched(&was, p)?,
    };
    let on = enabled.unwrap_or(was_on);
    if (on, params) != (was_on, was) {
        port.set_volumetric(on, params);
    }
    let (on, params) = port.volumetric();
    Ok(json!({"enabled": on, "params": to_json(&params)}))
}

/// `particle/lattice`: read, or patch, the lattice params; `:reseed` too.
pub fn lattice<P: ParticleSimPort>(port: &mut P, a: &Value) -> OpResult {
    let layer = promote::layer(port, a)?;
    let reseed = promote::opt_flag(a, "reseed")?.unwrap_or(false);
    let base = port
        .lattice(layer)
        .ok_or_else(|| format!("layer {layer}'s effect is not a lattice"))?;
    let params = match a.get("params") {
        None | Some(Value::Null) => base,
        Some(p) => patched(&base, p)?,
    };
    if a.get("params").is_some_and(|p| !p.is_null()) || reseed {
        port.set_lattice(layer, params, reseed)?;
    }
    let now = port.lattice(layer).unwrap_or(params);
    Ok(json!({"layer": layer, "params": to_json(&now)}))
}

/// `particle/helix`: read, or patch, the helix params.
pub fn helix<P: ParticleSimPort>(port: &mut P, a: &Value) -> OpResult {
    let layer = promote::layer(port, a)?;
    let base = port
        .helix(layer)
        .ok_or_else(|| format!("layer {layer}'s effect is not a helix"))?;
    if let Some(p) = a.get("params").filter(|p| !p.is_null()) {
        port.set_helix(layer, patched(&base, p)?)?;
    }
    let now = port.helix(layer).unwrap_or(base);
    Ok(json!({"layer": layer, "params": to_json(&now)}))
}
