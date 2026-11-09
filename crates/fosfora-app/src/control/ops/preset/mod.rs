//! Preset ops: everything the Presets panel does.
//!
//! Reference shape for a domain addon:
//! Promote (`Target`, `LoadMode` from args) -> Pipeline (pure: `step`,
//! `copy_name`, `listing`) -> Boundary (one [`PresetPort`] call per op).

pub mod port;
#[cfg(test)]
mod tests;

use serde_json::{Value, json};

pub use self::port::PresetPort;
use crate::control::{CljrsNs, ControlAddon, OpResult, OpSpec, args};

pub struct Addon;

impl<C: PresetPort + 'static> ControlAddon<C> for Addon {
    fn id(&self) -> &'static str {
        "preset"
    }

    fn ops(&self) -> Vec<OpSpec<C>> {
        vec![
            OpSpec::new("preset/list", "{}", "Every preset: index, name, builtin?, current?.", list::<C>),
            OpSpec::new("preset/current", "{}", "The loaded preset {index name dirty}, or nil.", current::<C>),
            OpSpec::new("preset/save", "{:name str}", "Save the live stack as a user preset.", save::<C>),
            OpSpec::new(
                "preset/load",
                "{:index int | :name str, :cut bool?}",
                "Load a preset: default transition, or a cut with :cut true.",
                load::<C>,
            ),
            OpSpec::new("preset/next", "{:cut bool?}", "Load the next preset (wraps).", next::<C>),
            OpSpec::new("preset/prev", "{:cut bool?}", "Load the previous preset (wraps).", prev::<C>),
            OpSpec::new("preset/delete", "{:index int | :name str}", "Delete a user preset.", delete::<C>),
            OpSpec::new(
                "preset/copy",
                "{:index int | :name str, :as str?}",
                "Copy a preset to a new user preset (default name \"<name> Copy\") and load it.",
                copy::<C>,
            ),
            OpSpec::new("preset/deselect", "{}", "Forget the current preset; the stack stays.", deselect::<C>),
            OpSpec::new("preset/new", "{}", "Clear every layer and forget the current preset.", new::<C>),
            OpSpec::new("preset/rescan", "{}", "Re-read the presets folder.", rescan::<C>),
            OpSpec::new(
                "preset/transition",
                "{:kind cut|dissolve|morph?, :secs num?, :keep-moving bool?}",
                "Read, or change any part of, the default switch transition.",
                transition::<C>,
            ),
        ]
    }

    fn ui_signals(&self) -> &'static [(&'static str, &'static str)] {
        &[
            ("pending_preset", "preset/load"),
            ("save_preset", "preset/save"),
            ("delete_preset", "preset/delete"),
            ("copy_preset_index", "preset/copy"),
            ("deselect_preset", "preset/deselect"),
            ("new_preset", "preset/new"),
            ("set_preset_transition", "preset/transition"),
        ]
    }

    fn cljrs(&self) -> Option<CljrsNs> {
        Some(CljrsNs {
            ns: "fosfora.preset",
            alias: "preset",
            src: include_str!("preset.cljrs"),
        })
    }
}

// ── Promote ─────────────────────────────────────────────────────────────────

/// How a load applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadMode {
    /// The default transition from Settings, as a click does.
    Switch,
    /// No transition.
    Cut,
}

impl LoadMode {
    pub fn from_args(a: &Value) -> Self {
        if args::opt_bool(a, "cut").unwrap_or(false) {
            Self::Cut
        } else {
            Self::Switch
        }
    }
}

/// The preset `{:index n}` or `{:name "x"}` names.
fn target<P: PresetPort>(port: &P, a: &Value) -> Result<usize, String> {
    args::pick(a, &port.preset_names(), "preset")
}

// ── Pipeline (pure) ─────────────────────────────────────────────────────────

/// The preset `delta` steps from `from` in a list of `len`, wrapping; `None`
/// for an empty list. With nothing loaded, forward starts at the first preset
/// and backward at the last.
pub fn step(from: Option<usize>, len: usize, delta: isize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let len_i = isize::try_from(len).ok()?;
    let start = match from {
        Some(i) => isize::try_from(i.min(len - 1)).ok()?,
        None if delta >= 0 => -1,
        None => len_i,
    };
    usize::try_from((start + delta).rem_euclid(len_i)).ok()
}

/// The name the Copy button gives: `"<base> Copy"`, then `"<base> Copy 2"`...
pub fn copy_name<S: AsRef<str>>(base: &str, existing: &[S]) -> String {
    let taken = |n: &str| existing.iter().any(|e| e.as_ref() == n);
    let first = format!("{base} Copy");
    if !taken(&first) {
        return first;
    }
    (2..)
        .map(|n| format!("{first} {n}"))
        .find(|c| !taken(c))
        .unwrap_or(first)
}

/// `[{index name builtin current}]`.
pub fn listing(names: &[String], builtin_count: usize, current: Option<usize>) -> Value {
    Value::Array(
        names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                json!({"index": i, "name": n, "builtin": i < builtin_count, "current": current == Some(i)})
            })
            .collect(),
    )
}

fn named(names: &[String], index: usize) -> Value {
    json!({"index": index, "name": names.get(index)})
}

// ── Boundary ────────────────────────────────────────────────────────────────

fn list<P: PresetPort>(port: &mut P, _: &Value) -> OpResult {
    Ok(listing(&port.preset_names(), port.builtin_count(), port.current_preset()))
}

fn current<P: PresetPort>(port: &mut P, _: &Value) -> OpResult {
    Ok(match port.current_preset() {
        Some(i) => {
            let mut v = named(&port.preset_names(), i);
            v["dirty"] = json!(port.preset_dirty());
            v
        }
        None => Value::Null,
    })
}

fn save<P: PresetPort>(port: &mut P, a: &Value) -> OpResult {
    let name = args::str(a, "name")?;
    let index = port.save_preset(name)?;
    Ok(named(&port.preset_names(), index))
}

fn apply_load<P: PresetPort>(port: &mut P, index: usize, mode: LoadMode) -> OpResult {
    match mode {
        LoadMode::Switch => port.switch_preset(index),
        LoadMode::Cut => port.load_preset(index),
    }
    Ok(named(&port.preset_names(), index))
}

fn load<P: PresetPort>(port: &mut P, a: &Value) -> OpResult {
    let index = target(port, a)?;
    apply_load(port, index, LoadMode::from_args(a))
}

fn stepped<P: PresetPort>(port: &mut P, a: &Value, delta: isize) -> OpResult {
    let index = step(port.target_preset(), port.preset_names().len(), delta)
        .ok_or("there are no presets")?;
    apply_load(port, index, LoadMode::from_args(a))
}

fn next<P: PresetPort>(port: &mut P, a: &Value) -> OpResult {
    stepped(port, a, 1)
}

fn prev<P: PresetPort>(port: &mut P, a: &Value) -> OpResult {
    stepped(port, a, -1)
}

fn delete<P: PresetPort>(port: &mut P, a: &Value) -> OpResult {
    let index = target(port, a)?;
    if index < port.builtin_count() {
        return Err("built-in presets cannot be deleted".into());
    }
    let gone = named(&port.preset_names(), index);
    port.delete_preset(index)?;
    Ok(gone)
}

fn copy<P: PresetPort>(port: &mut P, a: &Value) -> OpResult {
    let source = target(port, a)?;
    let names = port.preset_names();
    let name = match args::opt_str(a, "as") {
        Some(n) => n.to_owned(),
        None => copy_name(&names[source], &names),
    };
    let index = port.copy_preset(source, &name)?;
    port.load_preset(index);
    Ok(named(&port.preset_names(), index))
}

fn deselect<P: PresetPort>(port: &mut P, _: &Value) -> OpResult {
    port.deselect_preset();
    Ok(Value::Null)
}

fn new<P: PresetPort>(port: &mut P, _: &Value) -> OpResult {
    port.new_preset();
    Ok(Value::Null)
}

fn rescan<P: PresetPort>(port: &mut P, _: &Value) -> OpResult {
    port.rescan_presets();
    Ok(json!(port.preset_names().len()))
}

fn transition<P: PresetPort>(port: &mut P, a: &Value) -> OpResult {
    let kind = args::opt_transition(a, "kind")?;
    let secs = args::opt_f32(a, "secs");
    if secs.is_some_and(|s| !s.is_finite() || s < 0.0) {
        return Err(":secs must be a non-negative number".into());
    }
    let keep = args::opt_bool(a, "keep-moving");
    if kind.is_some() || secs.is_some() || keep.is_some() {
        port.set_preset_transition(kind, secs, keep);
    }
    let (kind, secs, keep) = port.preset_transition();
    Ok(json!({"kind": args::transition_name(kind), "secs": secs, "keep-moving": keep}))
}
