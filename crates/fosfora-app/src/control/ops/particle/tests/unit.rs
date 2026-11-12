//! Unit tests: every op's happy path, bad args, out-of-range and refusals.

use serde_json::json;

use super::Stub;
use crate::control::ControlAddon;
use crate::control::ops::particle::Addon;

#[test]
fn every_op_is_namespaced_and_every_claimed_signal_maps_to_one() {
    let s = Stub::new();
    let catalog = s.registry.catalog();
    let names: Vec<&str> = catalog
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap())
        .collect();
    assert!(
        names.iter().all(|n| n.starts_with("particle/")),
        "{names:?}"
    );
    let signals = <Addon as ControlAddon<Stub>>::ui_signals(&Addon);
    for (signal, op) in signals {
        assert!(names.contains(op), "{signal} -> {op} is not registered");
    }
    assert_eq!(signals.len(), 39);
}

fn loading(name: &str, index: usize) -> crate::gpu::particle::source_loader::SourceLoading {
    crate::gpu::particle::source_loader::SourceLoading {
        index,
        name: name.into(),
        done: 3,
        total: 10,
        secs: 0.5,
    }
}

#[test]
fn cancel_load_picks_by_index_or_name_and_needs_a_pick_when_several_run() {
    let mut s = Stub::new();
    assert!(
        s.call("particle/cancel-load", json!({}))
            .unwrap_err()
            .contains("no particle source")
    );
    s.loads = vec![
        loading("waves.mp4", 0),
        loading("cat.png", 1),
        loading("dog.png", 2),
    ];
    assert_eq!(
        s.call("particle/loading", json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(
        s.call("particle/cancel-load", json!({}))
            .unwrap_err()
            .contains("give :load")
    );
    assert!(
        s.call("particle/cancel-load", json!({"load": "png"}))
            .is_err(),
        "ambiguous"
    );
    assert!(s.call("particle/cancel-load", json!({"load": 7})).is_err());
    assert!(
        s.call("particle/cancel-load", json!({"load": true}))
            .is_err()
    );
    assert_eq!(s.loads.len(), 3, "refusals cancel nothing");
    let r = s
        .call("particle/cancel-load", json!({"load": "cat"}))
        .unwrap();
    assert_eq!(r["name"], "cat.png");
    let r = s.call("particle/cancel-load", json!({"load": 0})).unwrap();
    assert_eq!(r["name"], "waves.mp4");
    let r = s.call("particle/cancel-load", json!({})).unwrap();
    assert_eq!(r["name"], "dog.png");
    assert!(s.loads.is_empty());
}

#[test]
fn sim_reads_changes_a_subset_and_refuses_bad_values_without_changing_anything() {
    let mut s = Stub::new();
    let r = s.call("particle/sim", json!({})).unwrap();
    assert_eq!(r["emit-rate"], 100.0);
    let r = s
        .call("particle/sim", json!({"emit-rate": 500, "trail-length": 8}))
        .unwrap();
    assert_eq!(
        (r["emit-rate"].as_f64(), r["trail-length"].as_u64()),
        (Some(500.0), Some(8))
    );
    assert_eq!(r["drag"].as_f64().map(|d| (d * 100.0).round()), Some(90.0));
    for bad in [
        json!({"emit-rate": -1}),
        json!({"drag": 2.0}),
        json!({"trail-length": 99}),
        json!({"lifetime": 0}),
        json!({"speed": "fast"}),
    ] {
        assert!(s.call("particle/sim", bad.clone()).is_err(), "{bad}");
    }
    assert_eq!(
        s.call("particle/sim", json!({})).unwrap()["emit-rate"],
        500.0
    );
}

#[test]
fn layers_without_particles_or_out_of_range_are_refused() {
    let mut s = Stub::new();
    assert!(
        s.call("particle/sim", json!({"layer": 0}))
            .unwrap_err()
            .contains("no particle")
    );
    assert!(
        s.call("particle/sim", json!({"layer": 7}))
            .unwrap_err()
            .contains("out of range")
    );
    assert!(s.call("particle/state", json!({"layer": 0})).is_err());
    assert!(s.call("particle/morph", json!({"layer": 0})).is_err());
    assert!(s.call("particle/obstacle", json!({"layer": 0})).is_err());
    let st = s.call("particle/state", json!({"layer": 1})).unwrap();
    assert_eq!(
        (st["lattice"].as_bool(), st["splat"].as_bool()),
        (Some(true), Some(true))
    );
}

#[test]
fn quality_reads_sets_by_name_and_refuses_unknown_names() {
    let mut s = Stub::new();
    assert_eq!(
        s.call("particle/quality", json!({})).unwrap()["quality"],
        "high"
    );
    assert_eq!(
        s.call("particle/quality", json!({"value": "ULTRA"}))
            .unwrap()["quality"],
        "ultra"
    );
    assert!(
        s.call("particle/quality", json!({"value": "insane"}))
            .is_err()
    );
    assert!(s.call("particle/quality", json!({"value": 3})).is_err());
}

#[test]
fn volumetric_patches_known_fields_and_refuses_typos() {
    let mut s = Stub::new();
    let r = s
        .call("particle/volumetric", json!({"enabled": true}))
        .unwrap();
    assert_eq!(r["enabled"], true);
    let field = r["params"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    let current = r["params"][&field].clone();
    let r2 = s
        .call(
            "particle/volumetric",
            json!({"params": {field.clone(): current}}),
        )
        .unwrap();
    assert_eq!(r2["params"], r["params"]);
    assert!(
        s.call(
            "particle/volumetric",
            json!({"params": {"no_such_knob": 1}})
        )
        .is_err()
    );
    assert!(s.call("particle/volumetric", json!({"params": 3})).is_err());
}

#[test]
fn lattice_and_helix_patch_their_params_and_refuse_other_layers() {
    let mut s = Stub::new();
    let r = s
        .call(
            "particle/lattice",
            json!({"params": {"grid_res": 64}, "reseed": true}),
        )
        .unwrap();
    assert_eq!(r["params"]["grid_res"], 64);
    assert_eq!(
        s.did.last().map(String::as_str),
        Some("lattice reseed=true")
    );
    assert!(
        s.call("particle/lattice", json!({"params": {"grid_rez": 64}}))
            .is_err()
    );
    let r = s
        .call("particle/helix", json!({"params": {"slice_count": 32}}))
        .unwrap();
    assert_eq!(r["params"]["slice_count"], 32);
    s.layers[1].lattice = None;
    assert!(
        s.call("particle/lattice", json!({}))
            .unwrap_err()
            .contains("not a lattice")
    );
}

#[test]
fn builtins_load_by_index_or_name_and_loads_are_refused_while_loading() {
    let mut s = Stub::new();
    assert_eq!(
        s.call("particle/builtins", json!({})).unwrap()[1]["name"],
        "skull"
    );
    assert_eq!(
        s.call("particle/load-builtin", json!({"name": "phoe"}))
            .unwrap()["loading"],
        "phoenix"
    );
    assert!(
        s.call("particle/load-builtin", json!({"index": 9}))
            .is_err()
    );
    let r = s.call("particle/load-image", json!({})).unwrap();
    assert_eq!(r["loading"], "file dialog");
    s.call("particle/load-video", json!({"path": "/tmp/a.mp4"}))
        .unwrap();
    assert_eq!(
        s.call("particle/source", json!({})).unwrap()["kind"],
        "video"
    );
    s.loading = true;
    assert!(
        s.call("particle/load-model", json!({"path": "/tmp/a.glb"}))
            .is_err()
    );
    assert!(s.call("particle/load-image", json!({"path": 5})).is_err());
}

#[test]
fn model_pose_reads_anywhere_but_changes_only_a_model_source() {
    let mut s = Stub::new();
    assert!(s.call("particle/model-pose", json!({})).is_ok());
    assert!(
        s.call("particle/model-pose", json!({"yaw": 30}))
            .unwrap_err()
            .contains("not a model")
    );
    s.layers[1].source.kind = "model";
    let r = s
        .call("particle/model-pose", json!({"yaw": 30, "light-mix": 0.5}))
        .unwrap();
    assert_eq!(
        (
            r["pose"]["yaw_degrees"].as_f64(),
            r["pose"]["light_mix"].as_f64()
        ),
        (Some(30.0), Some(0.5))
    );
    assert!(s.call("particle/model-pose", json!({"scale": 0})).is_err());
}

#[test]
fn webcam_picks_a_camera_by_name_and_video_transport_needs_a_video() {
    let mut s = Stub::new();
    assert_eq!(
        s.call("particle/webcams", json!({})).unwrap()[1]["name"],
        "USB Cam"
    );
    s.call("particle/webcam", json!({"device": "usb"})).unwrap();
    assert_eq!(
        s.did.last().map(String::as_str),
        Some("webcam Some(\"USB Cam\")")
    );
    assert!(
        s.call("particle/webcam", json!({"device": "nope"}))
            .is_err()
    );
    assert!(
        s.call("particle/video", json!({"playing": false}))
            .unwrap_err()
            .contains("not a video")
    );
    s.call("particle/load-video", json!({"path": "/tmp/v.mp4"}))
        .unwrap();
    let r = s
        .call("particle/video", json!({"playing": false, "speed": 2}))
        .unwrap();
    assert_eq!(
        (r["playing"].as_bool(), r["speed"].as_f64()),
        (Some(false), Some(2.0))
    );
    assert!(s.call("particle/video", json!({"speed": 0})).is_err());
}

#[test]
fn splat_ops_need_a_splat_effect_and_one_download_at_a_time() {
    let mut s = Stub::new();
    assert_eq!(
        s.call("particle/splat-load", json!({"path": "~/s.ply"}))
            .unwrap()["layer"],
        1
    );
    s.call("particle/splat-demo", json!({})).unwrap();
    assert!(s.call("particle/splat-demo", json!({})).is_err());
    assert_eq!(
        s.call("particle/splat", json!({})).unwrap()["downloading"],
        true
    );
    s.layers[1].splat = false;
    assert!(s.call("particle/splat-load", json!({})).is_err());
}

#[test]
fn obstacle_runs_named_commands_and_refuses_bad_ones() {
    let mut s = Stub::new();
    assert_eq!(
        s.call("particle/obstacle", json!({})).unwrap()["enabled"],
        false
    );
    let r = s
        .call(
            "particle/obstacle",
            json!({"cmd": "set-mode", "value": "drape"}),
        )
        .unwrap();
    assert_eq!(r["mode"], "Drape");
    let r = s
        .call(
            "particle/obstacle",
            json!({"cmd": "threshold", "value": 0.25}),
        )
        .unwrap();
    assert_eq!(r["threshold"], 0.25);
    assert!(
        s.call(
            "particle/obstacle",
            json!({"cmd": "threshold", "value": "hi"})
        )
        .is_err()
    );
    assert!(
        s.call("particle/obstacle", json!({"cmd": "mode", "value": "melt"}))
            .is_err()
    );
    assert!(
        s.call("particle/obstacle", json!({"cmd": "explode"}))
            .is_err()
    );
    assert!(
        s.call("particle/obstacle", json!({"cmd": "load-image"}))
            .is_err()
    );
    let r = s
        .call(
            "particle/obstacle",
            json!({"cmd": "load-image", "path": "/tmp/m.png"}),
        )
        .unwrap();
    assert_eq!(r["source"], "image");
    let r = s
        .call("particle/obstacle", json!({"cmd": "clear"}))
        .unwrap();
    assert_eq!(
        (r["enabled"].as_bool(), r["source"].is_null()),
        (Some(false), true)
    );
}

#[test]
fn obstacle_load_guesses_the_kind_and_obstacle_webcam_picks_a_camera() {
    let mut s = Stub::new();
    assert_eq!(
        s.call("particle/obstacle-load", json!({"path": "/x/t.glb"}))
            .unwrap()["source"],
        "model"
    );
    let r = s
        .call(
            "particle/obstacle-load",
            json!({"path": "/x/t.bin", "kind": "image"}),
        )
        .unwrap();
    assert_eq!(r["source"], "image");
    assert!(s.call("particle/obstacle-load", json!({})).is_err());
    assert_eq!(
        s.call("particle/obstacle-webcam", json!({"index": 2}))
            .unwrap()["name"],
        "USB Cam"
    );
    assert_eq!(
        s.call("particle/obstacle-webcam", json!({"name": "integ"}))
            .unwrap()["index"],
        0
    );
    assert!(
        s.call("particle/obstacle-webcam", json!({"index": 1}))
            .is_err()
    );
}

#[test]
fn morph_targets_are_added_triggered_styled_and_cleared() {
    let mut s = Stub::new();
    let r = s
        .call("particle/morph-geometry", json!({"shape": "Star"}))
        .unwrap();
    assert_eq!(r["slots"], json!(["geometry:star"]));
    s.call("particle/morph-text", json!({"text": "HI", "slot": 1}))
        .unwrap();
    assert!(
        s.call("particle/morph-trigger", json!({"target": 3}))
            .unwrap_err()
            .contains("not a loaded")
    );
    assert_eq!(
        s.call("particle/morph-trigger", json!({"target": 1}))
            .unwrap()["dest"],
        1
    );
    assert_eq!(
        s.call("particle/morph-style", json!({"style": "flow"}))
            .unwrap()["style"],
        "flow"
    );
    assert_eq!(
        s.call("particle/morph-style", json!({"style": 4})).unwrap()["style"],
        "direct"
    );
    assert!(s.call("particle/morph-style", json!({"style": 5})).is_err());
    assert_eq!(
        s.call("particle/morph-cycle", json!({"mode": "timed"}))
            .unwrap()["cycle"],
        "timed"
    );
    assert!(
        s.call("particle/morph-blend", json!({"progress": 1.5}))
            .is_err()
    );
    assert!(
        s.call("particle/morph-geometry", json!({"shape": "cube"}))
            .is_err()
    );
    assert!(
        s.call("particle/morph-text", json!({"text": "  "}))
            .is_err()
    );
    assert!(s.call("particle/morph-clear", json!({"slot": 4})).is_err());
    assert_eq!(
        s.call("particle/morph-clear", json!({"slot": 0})).unwrap()["slots"],
        json!(["text:HI"])
    );
    s.call("particle/morph-load", json!({"kind": "model"}))
        .unwrap();
    assert_eq!(s.did.last().map(String::as_str), Some("morph model None"));
    assert!(
        s.call("particle/morph-load", json!({"kind": "sound"}))
            .is_err()
    );
    s.layers[1].morph = None;
    assert!(s.call("particle/morph-snapshot", json!({})).is_err());
}
