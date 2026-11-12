//! Unit tests for `layer/add :at`, `layer/shader-source`, `layer/loading`
//! and `layer/cancel-media`, against the stub in `tests.rs`.

use serde_json::json;

use super::{MAX, Stub, media_layer};
use crate::control::ops::layer::MediaLoadView;
use crate::control::testkit::cases;

fn load(file: &str) -> MediaLoadView {
    MediaLoadView {
        file: file.into(),
        done: 12,
        total: 450,
        secs: 1.5,
    }
}

#[test]
fn add_at_inserts_where_asked_and_selects_it() {
    let mut s = Stub::new();
    let r = s.call("layer/add", json!({"at": 0})).unwrap();
    assert_eq!(r["index"], 0);
    assert_eq!(r["active"], true);
    assert_eq!(s.layers.len(), 3);
    assert_eq!(
        s.layers[2].view.effect.as_deref(),
        Some("Fosfora"),
        "the rest shift down"
    );
    let r = s.call("layer/add", json!({"at": 3})).unwrap();
    assert_eq!(r["index"], 3, "at = len appends");
    assert_eq!(
        s.call("layer/add", json!({"at": null})).unwrap()["index"],
        4
    );
}

#[test]
fn add_at_refuses_a_position_off_the_stack_or_not_an_index() {
    let mut s = Stub::new();
    let before = s.snapshot();
    assert!(
        s.call("layer/add", json!({"at": 3}))
            .unwrap_err()
            .contains("out of range")
    );
    assert!(s.call("layer/add", json!({"at": -1})).is_err());
    assert!(s.call("layer/add", json!({"at": "top"})).is_err());
    assert_eq!(s.snapshot(), before, "refused adds change nothing");
    while s.layers.len() < MAX {
        s.call("layer/add", json!({"at": 0})).unwrap();
    }
    assert!(s.call("layer/add", json!({"at": 0})).is_err());
}

#[test]
fn property_add_at_lands_at_the_index_and_keeps_the_others_in_order() {
    cases(64, 0x1a7e_a77a, |rng, _| {
        let mut s = Stub::new();
        for _ in 0..rng.below(4) {
            s.call("layer/add", json!({})).unwrap();
        }
        let before: Vec<_> = s.snapshot().into_iter().map(|v| v.effect).collect();
        let at = rng.below(before.len() + 1);
        let r = s.call("layer/add", json!({"at": at})).unwrap();
        assert_eq!(r["index"], at);
        assert_eq!(s.active, at);
        let mut after: Vec<_> = s.snapshot().into_iter().map(|v| v.effect).collect();
        after.remove(at);
        assert_eq!(after, before);
    });
}

#[test]
fn shader_source_reads_an_effect_layer_and_refuses_a_media_one() {
    let mut s = Stub::new();
    let r = s
        .call("layer/shader-source", json!({"name": "fosfora"}))
        .unwrap();
    assert_eq!(r["layer"], 1);
    assert_eq!(r["effect"], "Fosfora");
    assert!(r["source"].as_str().unwrap().contains("fs_main"));
    assert_eq!(r["passes"].as_array().unwrap().len(), 1);
    assert_eq!(
        s.call("layer/shader-source", json!({})).unwrap()["layer"],
        0
    );
    s.layers.push(media_layer("Layer 3", "a.mp4", false));
    assert!(
        s.call("layer/shader-source", json!({"index": 2}))
            .unwrap_err()
            .contains("not an effect layer")
    );
    assert!(s.call("layer/shader-source", json!({"index": 9})).is_err());
}

#[test]
fn cancel_media_picks_by_index_or_file_and_needs_a_pick_when_several_load() {
    let mut s = Stub::new();
    assert!(
        s.call("layer/cancel-media", json!({}))
            .unwrap_err()
            .contains("no media")
    );
    s.loads = vec![load("a.mp4"), load("b.mp4"), load("intro.mov")];
    let listed = s.call("layer/loading", json!({})).unwrap();
    assert_eq!(
        listed[2],
        json!({"index": 2, "file": "intro.mov", "done": 12, "total": 450, "secs": 1.5})
    );
    assert!(
        s.call("layer/cancel-media", json!({}))
            .unwrap_err()
            .contains("give :load")
    );
    assert!(
        s.call("layer/cancel-media", json!({"load": "mp4"}))
            .is_err(),
        "ambiguous"
    );
    assert!(s.call("layer/cancel-media", json!({"load": 3})).is_err());
    assert_eq!(s.loads.len(), 3, "refusals cancel nothing");
    assert_eq!(
        s.call("layer/cancel-media", json!({"load": "intro"}))
            .unwrap()["file"],
        "intro.mov"
    );
    assert_eq!(
        s.call("layer/cancel-media", json!({"load": 1})).unwrap()["file"],
        "b.mp4"
    );
    assert_eq!(
        s.call("layer/cancel-media", json!({})).unwrap()["file"],
        "a.mp4"
    );
    assert!(s.loads.is_empty());
}
