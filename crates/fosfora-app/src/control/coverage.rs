//! Coverage gate: every UI action is reachable as an op.
//!
//! The universe is read from the SOURCE, not from the ops: every egui temp id
//! `main.rs` consumes (`remove_temp(egui::Id::new("save_preset"))`) is a UI
//! action. Each must be claimed by some addon's `ui_signals()` or listed in
//! [`EXEMPT`] with the reason it has no op. A new panel action without an op
//! fails here.

use std::collections::{BTreeMap, BTreeSet};

use super::ops::builtin_addons;

const MAIN_RS: &str = include_str!("../main.rs");

/// UI signals with no op, and why.
const EXEMPT: &[(&str, &str)] = &[
    (
        "tour_done",
        "onboarding tour bookkeeping; no show state changes",
    ),
    (
        "dismiss_shader_error",
        "closes an error toast in the window",
    ),
];

/// The ways `main.rs` consumes a UI signal: a typed or untyped `remove_temp`
/// of an egui temp id, or the cue strip's `take("...")` helper.
const NEEDLES: &[&str] = &[
    "remove_temp(egui::Id::new(\"",
    "remove_temp::<bool>(egui::Id::new(\"",
    "take(\"",
    // `let id = egui::Id::new("...")` then `get_temp` + `remove`: the
    // loading lines' Cancel buttons.
    "let id = egui::Id::new(\"",
];

/// Every UI signal `main.rs` consumes.
fn ui_signals() -> BTreeSet<&'static str> {
    NEEDLES
        .iter()
        .flat_map(|needle| {
            MAIN_RS.match_indices(needle).filter_map(move |(at, _)| {
                let rest = &MAIN_RS[at + needle.len()..];
                rest.find('"').map(|end| &rest[..end])
            })
        })
        .collect()
}

/// `signal -> (addon, op)` from every built-in addon.
fn claimed() -> BTreeMap<&'static str, (&'static str, &'static str)> {
    let mut map = BTreeMap::new();
    for addon in builtin_addons() {
        for &(signal, op) in addon.ui_signals() {
            map.insert(signal, (addon.id(), op));
        }
    }
    map
}

#[test]
fn the_signal_scan_sees_the_panels() {
    let signals = ui_signals();
    assert!(
        signals.len() > 100,
        "only {} signals found; did main.rs move?",
        signals.len()
    );
    assert!(signals.contains("save_preset"));
    for s in [
        "postprocess_to_previous",
        "postprocess_to_defaults",
        "scene_insert_cue",
        "scene_move_cue",
        "scene_set_cue_label",
        "scene_clear_cue_hold",
        "cancel_media_load",
        "cancel_source_load",
    ] {
        assert!(signals.contains(s), "the scan misses {s}");
    }
}

#[test]
fn every_claim_names_a_live_signal_and_a_registered_op() {
    let signals = ui_signals();
    let ops: BTreeSet<&str> = builtin_addons()
        .iter()
        .flat_map(|a| a.ops().into_iter().map(|o| o.name))
        .collect();
    for (signal, (addon, op)) in claimed() {
        assert!(
            signals.contains(signal),
            "{addon} claims {signal:?}, which main.rs no longer raises"
        );
        assert!(
            ops.contains(op),
            "{addon} maps {signal:?} to {op:?}, which no addon registers"
        );
    }
    for (signal, _) in EXEMPT {
        assert!(
            signals.contains(signal),
            "exemption for {signal:?}, which main.rs no longer raises"
        );
        assert!(
            !claimed().contains_key(signal),
            "{signal:?} is both claimed and exempt"
        );
    }
}

#[test]
fn every_ui_signal_has_an_op_or_a_reason() {
    let claimed = claimed();
    let exempt: BTreeSet<&str> = EXEMPT.iter().map(|(s, _)| *s).collect();
    let missing: Vec<&str> = ui_signals()
        .into_iter()
        .filter(|s| !claimed.contains_key(s) && !exempt.contains(s))
        .collect();
    assert!(
        missing.is_empty(),
        "{} UI actions have no op: {missing:?}",
        missing.len()
    );
}
