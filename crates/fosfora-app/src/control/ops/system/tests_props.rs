//! Property tests for the system ops: the pure pipeline (totality,
//! round-trips, idempotence) and random op sequences against the stub.

use serde_json::{Value, json};

use super::model::*;
use super::tests_stub::Stub;
use crate::audio::beat::{BPM_MAX, BPM_MIN, TempoConfig, TempoPreset};
use crate::control::testkit::cases;
use crate::gpu::types::OutputResolution;
use crate::midi::types::TriggerAction;
use crate::recording::types::{Container, VideoCodec};
use crate::settings::{AlphaOutputMode, FlashLimit};

// ── Property: pure pipeline ──────────────────────────────────────────────

#[test]
fn every_name_round_trips_through_its_parser() {
    fn round<T: Copy + PartialEq + std::fmt::Debug>(all: &[T], name: fn(T) -> &'static str) {
        for &t in all {
            assert_eq!(parse_named(name(t), all, name, "x"), Ok(t));
            assert_eq!(parse_named(&name(t).to_uppercase(), all, name, "x"), Ok(t));
        }
        let names: std::collections::HashSet<String> = all.iter().map(|&t| slug(name(t))).collect();
        assert_eq!(names.len(), all.len(), "names collide");
    }
    round(TriggerAction::ALL, trigger_name);
    round(&FlashLimit::ALL, flash_limit_name);
    round(AlphaOutputMode::ALL, alpha_mode_name);
    round(VideoCodec::ALL, codec_name);
    round(Container::ALL, container_name);
    round(OutputResolution::ALL, resolution_name);
    round(TempoPreset::ALL, tempo_preset_name);
    round(OutputKind::ALL, OutputKind::name);
    round(PixelFormat::ALL, PixelFormat::name);
    round(LinkMode::ALL, LinkMode::name);
    round(Workspace::ALL, Workspace::name);
}

#[test]
fn parse_named_is_total_and_only_accepts_listed_names() {
    cases(500, 0x5EED_5101, |rng, _| {
        let raw = rng.name();
        match parse_named(&raw, TriggerAction::ALL, trigger_name, "x") {
            Ok(t) => assert_eq!(slug(trigger_name(t)), slug(&raw)),
            Err(e) => assert!(e.contains("next-effect"), "{e}"),
        }
    });
}

#[test]
fn tempo_changes_always_land_in_range_and_are_idempotent() {
    cases(500, 0x5EED_5102, |rng, _| {
        let pick = |rng: &mut crate::control::testkit::Rng, lo: isize, hi: isize| {
            rng.coin().then(|| rng.range_i(lo, hi) as f32)
        };
        let change = TempoChange {
            preset: if rng.coin() {
                rng.pick(TempoPreset::ALL).copied()
            } else {
                None
            },
            center: pick(&mut *rng, -100, 1000),
            sigma: pick(&mut *rng, -5, 10).map(|s| s / 2.0),
            auto: rng.coin().then(|| rng.coin()),
        };
        let start = TempoConfig::default();
        let once = apply_tempo_change(start, &change);
        assert!((BPM_MIN as f32..=BPM_MAX as f32).contains(&once.prior_center_bpm));
        assert!((SIGMA_MIN..=SIGMA_MAX).contains(&once.prior_sigma));
        assert_eq!(apply_tempo_change(once, &change), once);
        assert_eq!(apply_tempo_change(once, &TempoChange::default()), once);
    });
}

#[test]
fn tempo_bpm_accepts_exactly_the_reportable_range() {
    cases(500, 0x5EED_5103, |rng, _| {
        let bpm = rng.range_i(-50, 400) as f64 + if rng.coin() { 0.5 } else { 0.0 };
        assert_eq!(
            tempo_bpm(bpm).is_ok(),
            (BPM_MIN..=BPM_MAX).contains(&bpm),
            "{bpm}"
        );
    });
    assert!(tempo_bpm(f64::NAN).is_err());
}

#[test]
fn output_changes_only_ever_carry_what_the_kind_accepts() {
    let keys = [
        "name",
        "resolution",
        "alpha-from-luma",
        "device",
        "pixel-format",
        "noise",
    ];
    cases(500, 0x5EED_5104, |rng, _| {
        let kind = *rng.pick(OutputKind::ALL).unwrap();
        let mut a = json!({});
        for _ in 0..rng.below(4) {
            let key = *rng.pick(&keys).unwrap();
            a[key] = match rng.below(4) {
                0 => json!(rng.name()),
                1 => json!(rng.coin()),
                2 => json!(*rng.pick(&["1080p", "yuyv", "auto", "4k", "bgrx"]).unwrap()),
                _ => Value::Null,
            };
        }
        if let Ok(changes) = output_changes(kind, &a) {
            assert!(
                changes.iter().all(|c| accepts(kind, c)),
                "{kind:?} {a} -> {changes:?}"
            );
        }
    });
}

#[test]
fn toggle_or_sets_when_given_and_flips_when_not() {
    for now in [false, true] {
        assert_eq!(toggle_or(now, None), !now);
        for want in [false, true] {
            assert_eq!(toggle_or(now, Some(want)), want);
        }
    }
}

// ── Property: random op sequences ────────────────────────────────────────

#[test]
fn random_op_sequences_keep_the_system_consistent() {
    let ops = [
        "system/tempo",
        "system/tempo-config",
        "system/tempo-set",
        "system/tempo-tap",
        "system/link",
        "system/record-start",
        "system/record-stop",
        "system/record-toggle",
        "system/recording-config",
        "system/output",
        "system/outputs",
        "system/settings",
        "system/theme",
        "system/web",
        "system/overlay",
        "system/fullscreen",
        "system/binding-matrix",
        "system/chain-editor",
        "system/workspace",
        "system/escape",
        "system/trigger",
        "system/output-window",
    ];
    cases(200, 0x5EED_5105, |rng, _| {
        let mut s = Stub::with_link();
        for _ in 0..40 {
            let op = *rng.pick(&ops).unwrap();
            let args = match rng.below(5) {
                0 => json!({}),
                1 => json!({"index": rng.below(5), "open": rng.coin(), "visible": rng.coin()}),
                2 => {
                    json!({"name": rng.name(), "kind": *rng.pick(&["ndi", "v4l2", "spout", "x"]).unwrap()})
                }
                3 => {
                    json!({"bpm": rng.range_i(0, 400), "center": rng.range_i(0, 400), "fps": rng.below(300)})
                }
                _ => {
                    json!({"action": rng.name(), "enabled": rng.coin(), "mode": "lead", "quantum": rng.below(20)})
                }
            };
            let result = s.call(op, args);
            // Tempo prior stays inside what the estimator can report.
            assert!((BPM_MIN as f32..=BPM_MAX as f32).contains(&s.tempo.prior_center_bpm));
            // One modal at a time.
            assert!(!(s.view.binding_matrix && s.view.chain_editor), "{op}");
            // An unbuilt sink never turns on.
            assert!(!s.out(OutputKind::Spout).enabled);
            // Recording settings stay valid.
            assert!(FPS_RANGE.contains(&s.rec.fps) && QUALITY_RANGE.contains(&s.rec.quality));
            // Every success is JSON-renderable and errors say something.
            match result {
                Ok(v) => assert!(serde_json::to_string(&v).is_ok()),
                Err(e) => assert!(!e.is_empty(), "{op}"),
            }
        }
    });
}
