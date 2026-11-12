//! Boundary: the Binding Matrix op bodies, each one [`BindingsPort`] call
//! that acts, answering with the new state.

// Every op body has the fixed `OpFn` signature, infallible ones included.
#![allow(clippy::unnecessary_wraps)]

use serde_json::{Value, json};

use super::pipeline::{
    binding_json, edited, lfo_json, lfo_key, matches, parse_scope, pick_binding, scope_name,
};
use super::port::{BindingEdit, BindingsPort};
use crate::bindings::lfo;
use crate::bindings::templates::builtin_templates;
use crate::bindings::types::{Binding, BindingScope};
use crate::control::{OpResult, args};

/// The binding with `id` as JSON.
fn describe<P: BindingsPort>(port: &P, id: &str) -> OpResult {
    let all = port.bindings();
    let i = all
        .iter()
        .position(|b| b.id == id)
        .ok_or_else(|| format!("no binding {id:?}"))?;
    Ok(binding_json(i, &all[i], port.binding_io(id)))
}

/// The binding the args pick.
fn target<P: BindingsPort>(port: &P, a: &Value) -> Result<Binding, String> {
    let all = port.bindings();
    let i = pick_binding(&all, a)?;
    Ok(all[i].clone())
}

pub fn list<P: BindingsPort>(port: &mut P, a: &Value) -> OpResult {
    let scope = parse_scope(a, "scope")?;
    let filter = args::opt_str(a, "filter");
    Ok(Value::Array(
        port.bindings()
            .iter()
            .enumerate()
            .filter(|(_, b)| scope.as_ref().is_none_or(|s| *s == b.scope))
            .filter(|(_, b)| matches(filter, &[&b.name, &b.source, &b.target.to_string()]))
            .map(|(i, b)| binding_json(i, b, port.binding_io(&b.id)))
            .collect(),
    ))
}

pub fn get<P: BindingsPort>(port: &mut P, a: &Value) -> OpResult {
    let b = target(port, a)?;
    describe(port, &b.id)
}

pub fn sources<P: BindingsPort>(port: &mut P, a: &Value) -> OpResult {
    let filter = args::opt_str(a, "filter");
    Ok(Value::Array(
        port.sources()
            .into_iter()
            .filter(|s| matches(filter, &[&s.key, &s.group]))
            .map(|s| json!({"group": s.group, "key": s.key, "value": s.value}))
            .collect(),
    ))
}

pub fn targets<P: BindingsPort>(port: &mut P, a: &Value) -> OpResult {
    let filter = args::opt_str(a, "filter");
    Ok(Value::Array(
        port.targets()
            .into_iter()
            .filter(|t| matches(filter, &[&t.target, &t.label, &t.group]))
            .map(|t| json!({"target": t.target, "label": t.label, "group": t.group}))
            .collect(),
    ))
}

pub fn add<P: BindingsPort>(port: &mut P, a: &Value) -> OpResult {
    let scope = parse_scope(a, "scope")?.unwrap_or(BindingScope::Preset);
    let edit = edited(BindingEdit::default(), a)?;
    let id = port.add_binding(scope, edit);
    describe(port, &id)
}

pub fn update<P: BindingsPort>(port: &mut P, a: &Value) -> OpResult {
    if a.get("scope").is_some() {
        return Err("a binding's scope cannot change; add a new one in the other scope".into());
    }
    let b = target(port, a)?;
    let edit = edited(BindingEdit::of(&b), a)?;
    port.edit_binding(&b.id, edit)?;
    describe(port, &b.id)
}

pub fn enable<P: BindingsPort>(port: &mut P, a: &Value) -> OpResult {
    let b = target(port, a)?;
    let mut edit = BindingEdit::of(&b);
    edit.enabled = args::opt_bool(a, "on").unwrap_or(!b.enabled);
    port.edit_binding(&b.id, edit)?;
    describe(port, &b.id)
}

pub fn remove<P: BindingsPort>(port: &mut P, a: &Value) -> OpResult {
    let all = port.bindings();
    let i = pick_binding(&all, a)?;
    let gone = binding_json(i, &all[i], (None, None));
    port.remove_binding(&all[i].id);
    Ok(gone)
}

pub fn duplicate<P: BindingsPort>(port: &mut P, a: &Value) -> OpResult {
    let b = target(port, a)?;
    let id = port
        .duplicate_binding(&b.id)
        .ok_or_else(|| format!("no binding {:?}", b.id))?;
    describe(port, &id)
}

pub fn learn<P: BindingsPort>(port: &mut P, a: &Value) -> OpResult {
    if args::opt_bool(a, "cancel").unwrap_or(false) {
        port.set_learning(None);
        return Ok(json!({"learning": Value::Null}));
    }
    let b = target(port, a)?;
    port.set_learning(Some(&b.id));
    let mut v = describe(port, &b.id)?;
    v["learning"] = json!(port.learning().as_deref() == Some(b.id.as_str()));
    Ok(v)
}

pub fn templates<P: BindingsPort>(_: &mut P, _: &Value) -> OpResult {
    Ok(Value::Array(
        builtin_templates()
            .iter()
            .enumerate()
            .map(|(i, t)| {
                json!({"index": i, "name": t.name, "description": t.description, "entries": t.entries.len()})
            })
            .collect(),
    ))
}

pub fn apply_template<P: BindingsPort>(port: &mut P, a: &Value) -> OpResult {
    let names: Vec<&str> = builtin_templates().iter().map(|t| t.name).collect();
    let template = args::pick(a, &names, "template")?;
    let layer = args::opt_usize(a, "layer").unwrap_or_else(|| port.active_layer());
    let ids = port.apply_template(template, layer)?;
    let added: Vec<Value> = ids
        .iter()
        .filter_map(|id| describe(port, id).ok())
        .collect();
    Ok(json!({"template": names[template], "layer": layer, "added": added}))
}

pub fn save<P: BindingsPort>(port: &mut P, a: &Value) -> OpResult {
    let scope = parse_scope(a, "scope")?.ok_or(":scope must be preset or global")?;
    let saved = port.save_bindings(scope.clone())?;
    let count = port.bindings().iter().filter(|b| b.scope == scope).count();
    Ok(json!({"scope": scope_name(&scope), "saved": saved, "count": count}))
}

pub fn lfos<P: BindingsPort>(_: &mut P, a: &Value) -> OpResult {
    if a.get("shape").is_none() && a.get("division").is_none() {
        return Ok(Value::Array(
            lfo::base_keys().filter_map(lfo_json).collect(),
        ));
    }
    let shape = args::str(a, "shape")?;
    let division = args::str(a, "division")?;
    let degrees = match args::opt_usize(a, "degrees") {
        None => 0,
        Some(d) => u16::try_from(d).map_err(|_| ":degrees must be 0..359")?,
    };
    let key = lfo_key(shape, division, degrees)?;
    lfo_json(&key).ok_or_else(|| format!("no LFO {key:?}"))
}
