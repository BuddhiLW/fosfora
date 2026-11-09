//! Boundary: MIDI and OSC mapping op bodies. One generic body per action,
//! instantiated per [`Proto`] because an op is a plain `fn`.

// Every op body has the fixed `OpFn` signature, infallible ones included.
#![allow(clippy::unnecessary_wraps)]

use serde_json::{Value, json};

use super::pipeline::{map_target, mapping_json, target_key};
use super::port::{MappingPort, Proto};
use crate::control::{OpResult, args};

fn status<P: MappingPort>(port: &mut P, proto: Proto) -> Value {
    let mut v = json!({
        "enabled": port.mapping_enabled(proto),
        "learning": port.mapping_learning(proto).as_ref().map(target_key),
        "mappings": port.mappings(proto).iter().map(mapping_json).collect::<Vec<_>>(),
    });
    if proto == Proto::Midi {
        v["port"] = json!(port.midi_port());
        v["ports"] = json!(port.midi_ports());
    }
    v
}

fn enable<P: MappingPort>(port: &mut P, a: &Value, proto: Proto) -> OpResult {
    let on = args::bool(a, "on")?;
    port.set_mapping_enabled(proto, on);
    Ok(json!({"enabled": port.mapping_enabled(proto)}))
}

fn learn<P: MappingPort>(port: &mut P, a: &Value, proto: Proto) -> OpResult {
    let target = if args::opt_bool(a, "cancel").unwrap_or(false) {
        None
    } else {
        Some(map_target(a)?)
    };
    port.set_mapping_learn(proto, target);
    Ok(json!({"learning": port.mapping_learning(proto).as_ref().map(target_key)}))
}

fn clear<P: MappingPort>(port: &mut P, a: &Value, proto: Proto) -> OpResult {
    let target = map_target(a)?;
    if !port.clear_mapping(proto, &target) {
        return Err(format!("no mapping for {}", target_key(&target)));
    }
    Ok(json!({"cleared": target_key(&target)}))
}

pub fn midi<P: MappingPort>(port: &mut P, _: &Value) -> OpResult {
    Ok(status(port, Proto::Midi))
}

pub fn osc<P: MappingPort>(port: &mut P, _: &Value) -> OpResult {
    Ok(status(port, Proto::Osc))
}

pub fn midi_connect<P: MappingPort>(port: &mut P, a: &Value) -> OpResult {
    if args::opt_bool(a, "disconnect").unwrap_or(false) {
        port.midi_connect(None)?;
    } else {
        let ports = port.midi_ports();
        let i = args::pick(a, &ports, "MIDI port")
            .map_err(|e| format!("{e} (or :disconnect true); ports: {ports:?}"))?;
        port.midi_connect(Some(&ports[i]))?;
    }
    Ok(json!({"port": port.midi_port()}))
}

pub fn midi_enable<P: MappingPort>(port: &mut P, a: &Value) -> OpResult {
    enable(port, a, Proto::Midi)
}

pub fn osc_enable<P: MappingPort>(port: &mut P, a: &Value) -> OpResult {
    enable(port, a, Proto::Osc)
}

pub fn midi_learn<P: MappingPort>(port: &mut P, a: &Value) -> OpResult {
    learn(port, a, Proto::Midi)
}

pub fn osc_learn<P: MappingPort>(port: &mut P, a: &Value) -> OpResult {
    learn(port, a, Proto::Osc)
}

pub fn midi_clear<P: MappingPort>(port: &mut P, a: &Value) -> OpResult {
    clear(port, a, Proto::Midi)
}

pub fn osc_clear<P: MappingPort>(port: &mut P, a: &Value) -> OpResult {
    clear(port, a, Proto::Osc)
}
