//! Property tests: the pure pipeline is total and round-trips; random op
//! sequences keep the stub's invariants.

use std::path::Path;

use serde_json::{Value, json};

use super::Stub;
use crate::control::ops::particle::model::{MediaKind, SimEdit, SimState};
use crate::control::ops::particle::pipeline::{
    GEOMETRY_SHAPES, MORPH_STYLES, expand_home, index_or_name, merge_patch, obstacle_file_kind,
    patched, to_json,
};
use crate::control::ops::particle::promote::{OBSTACLE_CMDS, obstacle_op};
use crate::control::testkit::{Rng, cases};
use crate::gpu::lattice::LatticeParams;

fn random_json(rng: &mut Rng, depth: usize) -> Value {
    match rng.below(if depth == 0 { 4 } else { 6 }) {
        0 => Value::Null,
        1 => json!(rng.coin()),
        2 => json!(rng.range_i(-5, 5)),
        3 => json!(rng.name()),
        4 => Value::Array((0..rng.below(3)).map(|_| random_json(rng, depth - 1)).collect()),
        _ => Value::Object(
            (0..rng.below(4))
                .map(|_| (rng.name(), random_json(rng, depth - 1)))
                .collect(),
        ),
    }
}

#[test]
fn merge_patch_with_nothing_or_with_itself_is_identity_and_is_idempotent() {
    cases(300, 0x9A27_0001, |rng, _| {
        let base = random_json(rng, 3);
        assert_eq!(merge_patch(&base, &json!({})).ok(), base.is_object().then(|| base.clone()).or_else(|| Some(json!({}))));
        if base.is_object() {
            assert_eq!(merge_patch(&base, &base), Ok(base.clone()));
        }
        let patch = random_json(rng, 2);
        if let Ok(once) = merge_patch(&base, &patch) {
            assert_eq!(merge_patch(&once, &patch), Ok(once.clone()), "{base} + {patch}");
        }
    });
}

#[test]
fn patched_lattice_params_round_trip_and_unknown_keys_are_refused() {
    let base = LatticeParams::default();
    let own = to_json(&base);
    let back: LatticeParams = patched(&base, &own).unwrap();
    assert_eq!(to_json(&back), own);
    cases(200, 0x9A27_0002, |rng, _| {
        let key = rng.name();
        let r: Result<LatticeParams, String> = patched(&base, &json!({key.clone(): 1}));
        assert_eq!(r.is_ok(), own.get(&key).is_some(), "{key}");
    });
}

#[test]
fn obstacle_op_is_total_over_names_and_values() {
    let values = [json!(null), json!(true), json!(0.5), json!(3), json!("bounce"), json!("fill"), json!(-1)];
    cases(500, 0x9A27_0003, |rng, _| {
        let name = if rng.coin() { (*rng.pick(OBSTACLE_CMDS).unwrap()).to_owned() } else { rng.name() };
        let name = if rng.coin() { format!("set-{name}") } else { name };
        let value = rng.pick(&values).unwrap().clone();
        let _ = obstacle_op(&json!({"cmd": name, "value": value, "path": "/tmp/x"}));
    });
    for name in OBSTACLE_CMDS {
        let ok = values.iter().any(|v| obstacle_op(&json!({"cmd": name, "value": v, "path": "/x"})).is_ok());
        assert!(ok, "{name} takes no value at all");
    }
}

#[test]
fn style_names_and_indices_round_trip() {
    for (i, name) in MORPH_STYLES.iter().enumerate() {
        let i = u32::try_from(i).unwrap();
        assert_eq!(index_or_name(&json!(name.to_uppercase()), MORPH_STYLES), Some(i));
        assert_eq!(index_or_name(&json!(i), MORPH_STYLES), Some(i));
    }
    cases(200, 0x9A27_0004, |rng, _| {
        let n = rng.below(20);
        assert_eq!(index_or_name(&json!(n), GEOMETRY_SHAPES).is_some(), n < GEOMETRY_SHAPES.len());
    });
}

#[test]
fn paths_and_kinds_are_total() {
    let home = Path::new("/home/v");
    assert_eq!(expand_home("~/a.png", Some(home)), home.join("a.png"));
    assert_eq!(expand_home("~", Some(home)), home.to_path_buf());
    assert_eq!(expand_home("~bob/a", Some(home)), Path::new("~bob/a"));
    cases(300, 0x9A27_0005, |rng, _| {
        let raw = format!("/{}.{}", rng.name(), rng.pick(&["png", "GLB", "mp4", "ply", ""]).unwrap());
        assert_eq!(expand_home(&raw, Some(home)), Path::new(&raw));
        let kind = obstacle_file_kind(Path::new(&raw));
        assert!(MediaKind::ALL.contains(&kind));
        assert_eq!(MediaKind::parse(kind.name()), Some(kind));
    });
}

#[test]
fn sim_edits_are_identity_when_empty_and_idempotent() {
    cases(300, 0x9A27_0006, |rng, _| {
        let s = SimState { emit_rate: 10.0, burst: 1, lifetime: 1.0, speed: 0.1, size: 0.01, drag: 0.5, trail_length: 3, max_particles: 99 };
        assert_eq!(s.with(&SimEdit::default()), s);
        #[allow(clippy::cast_precision_loss)]
        let e = SimEdit {
            emit_rate: rng.coin().then(|| rng.below(1000) as f32),
            burst: rng.coin().then(|| u32::try_from(rng.below(50)).unwrap()),
            trail_length: rng.coin().then(|| u32::try_from(rng.below(33)).unwrap()),
            ..SimEdit::default()
        };
        assert_eq!(s.with(&e).with(&e), s.with(&e));
        assert_eq!(s.with(&e).max_particles, 99);
    });
}

#[test]
fn random_op_sequences_keep_the_particle_state_valid() {
    let ops = [
        "particle/sim", "particle/quality", "particle/source", "particle/load-builtin",
        "particle/obstacle", "particle/morph-geometry", "particle/morph-text",
        "particle/morph-trigger", "particle/morph-clear", "particle/morph-style",
        "particle/morph-blend", "particle/video", "particle/state", "particle/lattice",
    ];
    cases(150, 0x9A27_0007, |rng, _| {
        let mut s = Stub::new();
        for _ in 0..25 {
            let op = *rng.pick(&ops).unwrap();
            let args = json!({
                "layer": rng.below(3), "emit-rate": rng.range_i(-10, 1000), "drag": rng.range_i(-1, 2),
                "trail-length": rng.below(40), "value": rng.name(), "index": rng.below(4),
                "name": rng.name(), "cmd": *rng.pick(OBSTACLE_CMDS).unwrap(), "shape": *rng.pick(GEOMETRY_SHAPES).unwrap(),
                "text": rng.name(), "target": rng.below(5), "slot": rng.below(5), "style": rng.below(6),
                "progress": rng.range_i(-1, 2), "playing": rng.coin(),
            });
            let before = s.layers[1].sim;
            let result = s.call(op, args);
            let sim = s.layers[1].sim.unwrap();
            assert!(sim.emit_rate >= 0.0 && (0.0..=1.0).contains(&sim.drag) && sim.trail_length <= 32, "{op}");
            if op == "particle/sim" && result.is_err() {
                assert_eq!(s.layers[1].sim, before, "a refused sim edit changed the sim");
            }
            let m = s.layers[1].morph.as_ref().unwrap();
            assert!(m.slots.len() <= 4 && m.target_count as usize == m.slots.len(), "{op}: {m:?}");
            assert!(s.layers[0].sim.is_none(), "{op} gave layer 0 particles");
        }
    });
}
