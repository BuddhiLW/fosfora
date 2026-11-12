//! Unit + property tests for the preset ops, against an in-memory port.
//! The integration leg (a live app over nREPL) is `tests/repl_ops.rs`.

use serde_json::{Value, json};

use super::*;
use crate::control::testkit::cases;
use crate::control::{ControlAddon, OpHost, Registry, run};
use crate::scene::types::TransitionType;

/// What the stub saw happen, in order.
#[derive(Debug, Clone, PartialEq)]
enum Did {
    Switch(usize),
    Cut(usize),
    Cleared,
}

#[derive(Default)]
struct Stub {
    names: Vec<String>,
    builtin: usize,
    current: Option<usize>,
    dirty: bool,
    did: Vec<Did>,
    transition: (TransitionType, f32, bool),
    registry: Registry<Stub>,
}

impl Stub {
    fn new(names: &[&str], builtin: usize) -> Self {
        let mut s = Self {
            names: names.iter().map(|n| (*n).to_owned()).collect(),
            builtin,
            transition: (TransitionType::Cut, 1.0, false),
            ..Self::default()
        };
        s.registry.register(&Addon as &dyn ControlAddon<Stub>);
        s
    }

    fn call(&mut self, op: &str, args: Value) -> Result<Value, String> {
        run(self, op, &args)
    }

    /// Insert `name` keeping user presets sorted, like a rescan does.
    fn insert_user(&mut self, name: &str) -> usize {
        let pos = self.names[self.builtin..]
            .iter()
            .position(|n| n.as_str() > name)
            .map_or(self.names.len(), |p| p + self.builtin);
        self.names.insert(pos, name.to_owned());
        pos
    }
}

impl OpHost for Stub {
    fn registry(&self) -> &Registry<Self> {
        &self.registry
    }
}

impl PresetPort for Stub {
    fn preset_names(&self) -> Vec<String> {
        self.names.clone()
    }
    fn builtin_count(&self) -> usize {
        self.builtin
    }
    fn current_preset(&self) -> Option<usize> {
        self.current
    }
    fn target_preset(&self) -> Option<usize> {
        self.current
    }
    fn preset_dirty(&self) -> bool {
        self.dirty
    }
    fn save_preset(&mut self, name: &str) -> Result<usize, String> {
        if name.trim().is_empty() {
            return Err("Preset name cannot be empty".into());
        }
        if let Some(i) = self.names.iter().position(|n| n == name) {
            if i < self.builtin {
                return Err("Cannot overwrite built-in".into());
            }
            self.current = Some(i);
            return Ok(i);
        }
        let i = self.insert_user(name);
        self.current = Some(i);
        Ok(i)
    }
    fn load_preset(&mut self, index: usize) {
        self.current = Some(index);
        self.did.push(Did::Cut(index));
    }
    fn switch_preset(&mut self, index: usize) {
        self.current = Some(index);
        self.did.push(Did::Switch(index));
    }
    fn delete_preset(&mut self, index: usize) -> Result<(), String> {
        self.names.remove(index);
        self.current = None;
        Ok(())
    }
    fn copy_preset(&mut self, _index: usize, name: &str) -> Result<usize, String> {
        Ok(self.insert_user(name))
    }
    fn deselect_preset(&mut self) {
        self.current = None;
        self.dirty = false;
    }
    fn new_preset(&mut self) {
        self.deselect_preset();
        self.did.push(Did::Cleared);
    }
    fn rescan_presets(&mut self) {}
    fn preset_transition(&self) -> (TransitionType, f32, bool) {
        self.transition
    }
    fn set_preset_transition(
        &mut self,
        kind: Option<TransitionType>,
        secs: Option<f32>,
        keep: Option<bool>,
    ) {
        let t = &mut self.transition;
        t.0 = kind.unwrap_or(t.0);
        t.1 = secs.map_or(t.1, |s| s.clamp(0.1, 30.0));
        t.2 = keep.unwrap_or(t.2);
    }
}

#[test]
fn transition_reads_without_args_and_changes_only_what_is_given() {
    let mut s = Stub::new(&[], 0);
    let r = s.call("preset/transition", json!({})).unwrap();
    assert_eq!(r, json!({"kind": "cut", "secs": 1.0, "keep-moving": false}));
    let r = s
        .call(
            "preset/transition",
            json!({"kind": "dissolve", "secs": 2.5}),
        )
        .unwrap();
    assert_eq!(
        r,
        json!({"kind": "dissolve", "secs": 2.5, "keep-moving": false})
    );
    assert!(
        s.call("preset/transition", json!({"kind": "wipe"}))
            .is_err()
    );
    assert!(s.call("preset/transition", json!({"secs": -1})).is_err());
    assert_eq!(
        s.transition.0,
        TransitionType::Dissolve,
        "a refused call changes nothing"
    );
}

// ── Unit ────────────────────────────────────────────────────────────────────

#[test]
fn every_op_is_registered_under_the_preset_namespace() {
    let s = Stub::new(&[], 0);
    let catalog = s.registry.catalog();
    let names: Vec<&str> = catalog
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap())
        .collect();
    assert!(names.iter().all(|n| n.starts_with("preset/")), "{names:?}");
    for op in [
        "preset/save",
        "preset/load",
        "preset/delete",
        "preset/copy",
        "preset/next",
    ] {
        assert!(names.contains(&op), "{op} missing from {names:?}");
    }
}

#[test]
fn load_by_name_switches_and_cut_loads_without_transition() {
    let mut s = Stub::new(&["Aurora", "Night Drive", "Tunnel"], 1);
    let r = s.call("preset/load", json!({"name": "night"})).unwrap();
    assert_eq!(r, json!({"index": 1, "name": "Night Drive"}));
    s.call("preset/load", json!({"index": 2, "cut": true}))
        .unwrap();
    assert_eq!(s.did, vec![Did::Switch(1), Did::Cut(2)]);
}

#[test]
fn ambiguous_or_missing_names_are_refused_with_the_candidates() {
    let mut s = Stub::new(&["Tunnel", "Tessera Tunnel"], 0);
    let e = s.call("preset/load", json!({"name": "unn"})).unwrap_err();
    assert!(e.contains("Tessera Tunnel"), "{e}");
    assert!(s.call("preset/load", json!({"name": "zzz"})).is_err());
    assert!(s.call("preset/load", json!({})).is_err());
    assert!(s.did.is_empty());
}

#[test]
fn save_reports_the_saved_index_and_refuses_an_empty_name() {
    let mut s = Stub::new(&["Aurora"], 1);
    let r = s.call("preset/save", json!({"name": "Mine"})).unwrap();
    assert_eq!(r, json!({"index": 1, "name": "Mine"}));
    assert!(s.call("preset/save", json!({"name": "  "})).is_err());
    assert!(s.call("preset/save", json!({})).is_err());
}

#[test]
fn builtins_cannot_be_deleted_user_presets_can() {
    let mut s = Stub::new(&["Aurora", "Mine"], 1);
    assert!(s.call("preset/delete", json!({"index": 0})).is_err());
    assert_eq!(s.names.len(), 2);
    s.call("preset/delete", json!({"name": "Mine"})).unwrap();
    assert_eq!(s.names, vec!["Aurora".to_owned()]);
}

#[test]
fn copy_names_like_the_copy_button_and_loads_the_copy() {
    let mut s = Stub::new(&["Aurora", "Aurora Copy"], 1);
    let r = s.call("preset/copy", json!({"index": 0})).unwrap();
    assert_eq!(r["name"], "Aurora Copy 2");
    assert_eq!(
        s.did.last(),
        Some(&Did::Cut(r["index"].as_u64().unwrap() as usize))
    );
    let r = s
        .call("preset/copy", json!({"index": 0, "as": "Dawn"}))
        .unwrap();
    assert_eq!(r["name"], "Dawn");
}

#[test]
fn next_and_prev_wrap_and_start_from_the_ends_when_nothing_is_loaded() {
    assert_eq!(step(None, 3, 1), Some(0));
    assert_eq!(step(None, 3, -1), Some(2));
    assert_eq!(step(Some(2), 3, 1), Some(0));
    assert_eq!(step(Some(0), 3, -1), Some(2));
    assert_eq!(step(Some(0), 0, 1), None);
    let mut s = Stub::new(&[], 0);
    assert!(s.call("preset/next", json!({})).is_err());
}

#[test]
fn new_clears_and_current_reports_nil_after() {
    let mut s = Stub::new(&["Aurora"], 1);
    s.call("preset/load", json!({"index": 0})).unwrap();
    assert_eq!(
        s.call("preset/current", json!({})).unwrap()["name"],
        "Aurora"
    );
    s.call("preset/new", json!({})).unwrap();
    assert_eq!(s.call("preset/current", json!({})).unwrap(), Value::Null);
    assert_eq!(s.did.last(), Some(&Did::Cleared));
}

// ── Property ────────────────────────────────────────────────────────────────

#[test]
fn step_always_lands_inside_the_list_and_inverts() {
    cases(500, 0x5EED_0001, |rng, _| {
        let len = rng.below(20);
        let from = rng.coin().then(|| rng.below(len.max(1)));
        let delta = rng.range_i(-50, 50);
        match step(from, len, delta) {
            None => assert_eq!(len, 0),
            Some(i) => {
                assert!(i < len);
                if let Some(f) = from {
                    assert_eq!(step(Some(i), len, -delta), Some(f));
                }
            }
        }
    });
}

#[test]
fn copy_name_is_always_fresh_and_derived_from_the_base() {
    cases(300, 0x5EED_0002, |rng, _| {
        let base = rng.name();
        let existing: Vec<String> = (0..rng.below(8))
            .map(|i| match i {
                0 => format!("{base} Copy"),
                n => format!("{base} Copy {}", n + 1),
            })
            .chain((0..rng.below(4)).map(|_| rng.name()))
            .collect();
        let name = copy_name(&base, &existing);
        assert!(!existing.contains(&name), "{name:?} in {existing:?}");
        assert!(name.starts_with(&format!("{base} Copy")));
    });
}

#[test]
fn random_op_sequences_keep_the_store_consistent() {
    let ops = [
        "preset/list",
        "preset/load",
        "preset/next",
        "preset/prev",
        "preset/save",
        "preset/delete",
        "preset/copy",
        "preset/current",
        "preset/new",
        "preset/deselect",
    ];
    cases(200, 0x5EED_0003, |rng, _| {
        let mut s = Stub::new(&["Aurora", "Night"], 2);
        for _ in 0..30 {
            let op = *rng.pick(&ops).unwrap();
            let len = s.names.len();
            let args = match rng.below(3) {
                0 => json!({"index": rng.below(len + 2)}),
                1 => json!({"name": rng.name(), "cut": rng.coin()}),
                _ => json!({}),
            };
            let before = s.names.len();
            let result = s.call(op, args);
            // Built-ins survive anything.
            assert_eq!(&s.names[..2], ["Aurora", "Night"]);
            // A current preset always points inside the list.
            assert!(s.current.is_none_or(|c| c < s.names.len()), "{op}");
            // Only save/copy grow the list, only delete shrinks it, by one.
            match (op, &result) {
                ("preset/save" | "preset/copy", Ok(_)) => assert!(s.names.len() <= before + 1),
                ("preset/delete", Ok(_)) => assert_eq!(s.names.len(), before - 1),
                _ => assert_eq!(s.names.len(), before, "{op} changed the list"),
            }
            // Results that name a preset name one that exists.
            if let Ok(v) = &result {
                if let Some(i) = v.get("index").and_then(Value::as_u64) {
                    if op != "preset/delete" {
                        assert!((i as usize) < s.names.len(), "{op} -> {v}");
                    }
                }
            }
        }
    });
}
