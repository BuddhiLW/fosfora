//! Unit tests for the system ops: happy paths, bad args, out-of-range and
//! refused calls, against the stub in `tests_stub.rs`. Property tests:
//! `tests_props.rs`. Integration: `tests/repl_ops.rs`.

use serde_json::{Value, json};

use super::Addon;
use super::model::*;
use super::tests_stub::Stub;
use crate::audio::beat::{BPM_MAX, TempoCommand};
use crate::control::ControlAddon;
use crate::midi::types::TriggerAction;
use crate::recording::types::{Container, RecordingConfig, VideoCodec};
use crate::settings::FlashLimit;

// ── Unit: registration ───────────────────────────────────────────────────

#[test]
fn every_op_is_registered_under_the_system_namespace() {
    let s = Stub::new();
    let catalog = s.registry.catalog();
    let names: Vec<&str> = catalog
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap())
        .collect();
    assert!(names.iter().all(|n| n.starts_with("system/")), "{names:?}");
    assert!(names.len() >= 30, "{names:?}");
}

#[test]
fn every_claimed_signal_maps_to_a_registered_op() {
    let s = Stub::new();
    let addon: &dyn ControlAddon<Stub> = &Addon;
    for (signal, op) in addon.ui_signals() {
        assert!(
            s.registry.get(op).is_some(),
            "{signal} -> {op} is not registered"
        );
    }
}

// ── Unit: tempo and Link ─────────────────────────────────────────────────

#[test]
fn tempo_config_applies_a_preset_then_fields_and_clamps() {
    let mut s = Stub::new();
    let r = s
        .call("system/tempo-config", json!({"preset": "House"}))
        .unwrap();
    assert_eq!(r["preset"], "house");
    assert_eq!(r["center"], 127.0);
    let r = s
        .call("system/tempo-config", json!({"center": 999, "sigma": 0}))
        .unwrap();
    assert_eq!(r["center"], f64::from(BPM_MAX as f32));
    assert_eq!(r["sigma"], f64::from(SIGMA_MIN));
    assert_eq!(r["preset"], Value::Null, "hand-tuned has no preset");
    assert!(
        s.call("system/tempo-config", json!({"preset": "polka"}))
            .is_err()
    );
    let before = s.tempo;
    assert_eq!(
        s.call("system/tempo-config", json!({})).unwrap()["center"],
        f64::from(before.prior_center_bpm)
    );
    assert_eq!(s.tempo, before, "no args reads");
}

#[test]
fn tempo_set_locks_in_range_and_refuses_outside() {
    let mut s = Stub::new();
    assert_eq!(
        s.call("system/tempo-set", json!({"bpm": 174})).unwrap(),
        json!({"bpm": 174.0})
    );
    assert_eq!(s.commands, vec![TempoCommand::Tap(174.0)]);
    assert!(s.call("system/tempo-set", json!({"bpm": 20})).is_err());
    assert!(s.call("system/tempo-set", json!({"bpm": 900})).is_err());
    assert!(s.call("system/tempo-set", json!({})).is_err());
    assert_eq!(s.commands.len(), 1, "refused calls queue nothing");
}

#[test]
fn half_double_and_tap_reach_the_detector() {
    let mut s = Stub::new();
    s.call("system/tempo-half", json!({})).unwrap();
    s.call("system/tempo-double", json!({})).unwrap();
    s.call("system/tempo-tap", json!({})).unwrap();
    assert_eq!(
        s.commands,
        vec![TempoCommand::ShiftOctave(-1), TempoCommand::ShiftOctave(1)]
    );
    assert_eq!(s.taps, 1);
}

#[test]
fn link_reads_changes_and_says_when_not_built() {
    let mut s = Stub::new();
    let e = s.call("system/link", json!({})).unwrap_err();
    assert!(e.contains("link"), "{e}");
    let mut s = Stub::with_link();
    let r = s
        .call(
            "system/link",
            json!({"enabled": true, "mode": "lead", "quantum": 8}),
        )
        .unwrap();
    assert_eq!(r["enabled"], true);
    assert_eq!(r["mode"], "lead");
    assert_eq!(r["quantum"], 8.0);
    assert!(s.call("system/link", json!({"mode": "boss"})).is_err());
    assert!(s.call("system/link", json!({"quantum": -1})).is_err());
    assert!(s.call("system/link", json!({"quantum": "four"})).is_err());
    assert_eq!(
        s.link.unwrap().mode,
        LinkMode::Lead,
        "refused calls change nothing"
    );
}

// ── Unit: recording ──────────────────────────────────────────────────────

#[test]
fn record_start_stop_toggle_and_refusals() {
    let mut s = Stub::new();
    let r = s.call("system/record-start", json!({})).unwrap();
    assert_eq!(r["path"], "/tmp/out.mp4");
    assert!(
        s.call("system/record-start", json!({})).is_err(),
        "already recording"
    );
    assert_eq!(
        s.call("system/record-toggle", json!({})).unwrap()["recording"],
        false
    );
    assert_eq!(
        s.call("system/record-toggle", json!({})).unwrap()["recording"],
        true
    );
    assert_eq!(
        s.call("system/record-stop", json!({})).unwrap()["recording"],
        false
    );
    s.ffmpeg = false;
    assert!(
        s.call("system/record-start", json!({}))
            .unwrap_err()
            .contains("ffmpeg")
    );
    assert!(!s.recording);
}

#[test]
fn recording_config_changes_only_what_is_given() {
    let mut s = Stub::new();
    let r = s
        .call(
            "system/recording-config",
            json!({"codec": "HEVC", "fps": 30, "container": "mkv"}),
        )
        .unwrap();
    assert_eq!(r["codec"], "hevc");
    assert_eq!(r["fps"], 30);
    assert_eq!(r["container"], "mkv");
    assert_eq!(s.rec.quality, RecordingConfig::default().quality);
    for bad in [
        json!({"fps": 0}),
        json!({"fps": 1000}),
        json!({"quality": 99}),
        json!({"codec": "divx"}),
    ] {
        assert!(
            s.call("system/recording-config", bad.clone()).is_err(),
            "{bad}"
        );
    }
    assert_eq!(s.rec.codec, VideoCodec::Hevc);
    assert_eq!(s.rec.container, Container::Mkv);
    let r = s.call("system/recording", json!({})).unwrap();
    assert_eq!(r["config"]["codec"], "hevc");
}

// ── Unit: outputs ────────────────────────────────────────────────────────

#[test]
fn output_configures_and_switches_a_built_sink() {
    let mut s = Stub::new();
    let r = s
        .call(
            "system/output",
            json!({"kind": "NDI", "enabled": true, "name": "Stage", "resolution": "1080p", "alpha-from-luma": true}),
        )
        .unwrap();
    assert_eq!(r["enabled"], true);
    assert_eq!(r["name"], "Stage");
    assert_eq!(r["resolution"], "1080p");
    assert_eq!(r["alpha-from-luma"], true);
    let r = s
        .call(
            "system/output",
            json!({"kind": "v4l2", "device": "auto", "pixel-format": "bgrx"}),
        )
        .unwrap();
    assert_eq!(r["device"], "auto");
    assert_eq!(r["pixel-format"], "bgrx");
    assert_eq!(
        s.call("system/outputs", json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        4
    );
}

#[test]
fn output_refuses_unbuilt_sinks_unknown_kinds_and_foreign_settings() {
    let mut s = Stub::new();
    let e = s
        .call("system/output", json!({"kind": "spout", "enabled": true}))
        .unwrap_err();
    assert!(e.contains("not built"), "{e}");
    assert_eq!(
        s.call("system/output", json!({"kind": "spout"})).unwrap()["available"],
        false
    );
    assert!(s.call("system/output", json!({"kind": "hdmi"})).is_err());
    assert!(s.call("system/output", json!({})).is_err());
    assert!(
        s.call("system/output", json!({"kind": "v4l2", "name": "x"}))
            .is_err()
    );
    assert!(
        s.call(
            "system/output",
            json!({"kind": "ndi", "pixel-format": "yuyv"})
        )
        .is_err()
    );
    assert!(
        s.call("system/output", json!({"kind": "ndi", "name": "  "}))
            .is_err()
    );
    assert!(
        s.call("system/output", json!({"kind": "ndi", "resolution": "16k"}))
            .is_err()
    );
    assert_eq!(s.out(OutputKind::Ndi).name.as_deref(), Some("Fosfora"));
}

#[test]
fn v4l2_refresh_and_output_window() {
    let mut s = Stub::new();
    let r = s.call("system/v4l2-refresh", json!({})).unwrap();
    assert_eq!(r["devices"].as_array().unwrap().len(), 2);
    let r = s.call("system/output-window", json!({})).unwrap();
    assert_eq!(r["displays"].as_array().unwrap().len(), 2);
    let r = s
        .call("system/output-window", json!({"name": "proj"}))
        .unwrap();
    assert_eq!(r["index"], 1);
    assert_eq!(s.requested_display, Some(1));
    assert!(s.call("system/output-window", json!({"index": 7})).is_err());
    s.window_on = Some("Projector".into());
    let r = s
        .call("system/output-window", json!({"close": true}))
        .unwrap();
    assert_eq!(r["open-on"], Value::Null);
}

// ── Unit: settings, theme, web ───────────────────────────────────────────

#[test]
fn settings_read_change_and_refuse_unknown_names() {
    let mut s = Stub::new();
    let r = s
        .call("system/settings", json!({"flash-limit": "strict", "output-alpha": "passthrough", "auto-reconnect": false}))
        .unwrap();
    assert_eq!(
        r,
        json!({"flash-limit": "strict", "output-alpha": "passthrough", "auto-reconnect": false})
    );
    assert!(
        s.call("system/settings", json!({"flash-limit": "sometimes"}))
            .is_err()
    );
    assert_eq!(s.settings.flash_limit, FlashLimit::Strict);
}

#[test]
fn theme_lists_and_picks() {
    let mut s = Stub::new();
    let list = s.call("system/theme", json!({})).unwrap();
    assert_eq!(
        list[1],
        json!({"index": 1, "name": "Gray", "current": true})
    );
    assert_eq!(
        s.call("system/theme", json!({"name": "black"})).unwrap()["index"],
        2
    );
    assert_eq!(s.theme, 2);
    assert!(s.call("system/theme", json!({"index": 9})).is_err());
}

#[test]
fn web_changes_and_never_reports_the_key() {
    let mut s = Stub::new();
    let r = s
        .call("system/web", json!({"enabled": true, "lan": true}))
        .unwrap();
    assert_eq!(r["enabled"], true);
    assert_eq!(r["lan"], true);
    let r = s.call("system/web-replace-key", json!({})).unwrap();
    assert_eq!(s.key_replaced, 1);
    assert!(r.get("key").is_none() && r.get("access_key").is_none());
}

// ── Unit: view, shortcuts, quit, triggers ────────────────────────────────

#[test]
fn view_ops_toggle_without_args_and_set_with_them() {
    let mut s = Stub::new();
    assert_eq!(
        s.call("system/overlay", json!({})).unwrap()["overlay"],
        false
    );
    assert_eq!(
        s.call("system/overlay", json!({"visible": true})).unwrap()["overlay"],
        true
    );
    assert_eq!(
        s.call("system/fullscreen", json!({})).unwrap()["fullscreen"],
        true
    );
    s.call("system/binding-matrix", json!({"open": true}))
        .unwrap();
    let r = s.call("system/chain-editor", json!({})).unwrap();
    assert_eq!(
        (r["chain-editor"].as_bool(), r["binding-matrix"].as_bool()),
        (Some(true), Some(false))
    );
    assert_eq!(
        s.call("system/workspace", json!({"name": "Perform"}))
            .unwrap(),
        "perform"
    );
    assert!(
        s.call("system/workspace", json!({"name": "garden"}))
            .is_err()
    );
    assert_eq!(s.call("system/workspace", json!({})).unwrap(), "perform");
}

#[test]
fn tours_escape_and_quit() {
    let mut s = Stub::new();
    assert_eq!(
        s.call("system/tours", json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        2
    );
    s.call("system/tour", json!({"name": "layers"})).unwrap();
    assert_eq!(s.tour, Some(1));
    assert!(s.call("system/tour", json!({"index": 9})).is_err());
    s.view.overlay = false;
    assert_eq!(
        s.call("system/escape", json!({})).unwrap()["did"],
        "showed-interface"
    );
    assert_eq!(s.call("system/quit", json!({})).unwrap()["asked"], true);
    assert_eq!(s.quit, Some(false));
    s.call("system/quit", json!({"confirm": true})).unwrap();
    assert_eq!(s.quit, Some(true));
}

#[test]
fn every_trigger_fires_by_name_and_unknown_ones_are_refused() {
    let mut s = Stub::new();
    let names = s.call("system/triggers", json!({})).unwrap();
    assert_eq!(names.as_array().unwrap().len(), TriggerAction::ALL.len());
    for name in names.as_array().unwrap() {
        s.call("system/trigger", json!({"action": name})).unwrap();
    }
    assert_eq!(s.triggers, TriggerAction::ALL);
    s.call("system/trigger", json!({"action": "next_preset"}))
        .unwrap();
    assert_eq!(s.triggers.last(), Some(&TriggerAction::NextPreset));
    assert!(
        s.call("system/trigger", json!({"action": "explode"}))
            .is_err()
    );
    assert!(s.call("system/trigger", json!({})).is_err());
}

// ── Unit: gaps (web port, ui scale, theme files, tours, quit cancel) ─────

#[test]
fn web_port_changes_in_range_and_refuses_others() {
    let mut s = Stub::new();
    assert_eq!(
        s.call("system/web", json!({"port": 9191})).unwrap()["port"],
        9191
    );
    for bad in [
        json!({"port": 80}),
        json!({"port": 70000}),
        json!({"port": "x"}),
        json!({"port": -1}),
    ] {
        assert!(s.call("system/web", bad.clone()).is_err(), "{bad}");
    }
    assert_eq!(s.web.port, 9191, "refused calls change nothing");
    assert!(
        s.call("system/web", json!({"port": 80, "enabled": true}))
            .is_err(),
        "a bad port refuses the whole call"
    );
    assert!(!s.web.enabled);
}

#[test]
fn ui_scale_reads_sets_in_range_and_refuses_others() {
    let mut s = Stub::new();
    assert_eq!(
        s.call("system/ui-scale", json!({})).unwrap(),
        json!({"scale": 1.0})
    );
    assert_eq!(
        s.call("system/ui-scale", json!({"scale": 1.5})).unwrap(),
        json!({"scale": 1.5})
    );
    for bad in [
        json!({"scale": 0.5}),
        json!({"scale": 3}),
        json!({"scale": "big"}),
    ] {
        assert!(s.call("system/ui-scale", bad.clone()).is_err(), "{bad}");
    }
    assert_eq!(s.ui_scale, 1.5);
}

#[test]
fn theme_files_reload_save_a_copy_and_show_the_folder() {
    let mut s = Stub::new();
    let list = s.call("system/theme-reload", json!({})).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 3);
    assert_eq!(s.theme_reloads, 1);
    let r = s.call("system/theme-save-copy", json!({})).unwrap();
    assert_eq!(r, json!({"name": "Gray copy", "index": 3, "count": 4}));
    s.theme_save_fails = true;
    assert!(
        s.call("system/theme-save-copy", json!({}))
            .unwrap_err()
            .contains("Could not save")
    );
    assert_eq!(s.themes.len(), 4);
    let r = s.call("system/theme-folder", json!({})).unwrap();
    assert_eq!(r, json!({"dir": "/cfg/themes", "opened": false}));
    assert_eq!(s.folder_opened, 0, "reading the folder opens nothing");
    s.call("system/theme-folder", json!({"open": true}))
        .unwrap();
    assert_eq!(s.folder_opened, 1);
}

#[test]
fn tour_reports_starts_and_ends_and_tours_say_which_were_taken() {
    let mut s = Stub::new();
    assert_eq!(
        s.call("system/tour", json!({})).unwrap(),
        json!({"running": false})
    );
    let r = s.call("system/tour", json!({"index": 1})).unwrap();
    assert_eq!(r["running"], true);
    assert_eq!(s.call("system/tour", json!({})).unwrap()["name"], "Layers");
    let list = s.call("system/tours", json!({})).unwrap();
    assert_eq!(list[1]["current"], true);
    assert_eq!(list[1]["taken"], false);
    assert_eq!(list[0]["taken"], true);
    assert_eq!(
        s.call("system/tour", json!({"end": true})).unwrap(),
        json!({"running": false})
    );
    assert_eq!(s.tours_taken, [true, true]);
    assert!(s.call("system/tour", json!({"name": "dance"})).is_err());
}

#[test]
fn quit_cancel_closes_the_dialog() {
    let mut s = Stub::new();
    s.call("system/quit", json!({})).unwrap();
    assert_eq!(s.quit, Some(false));
    assert_eq!(
        s.call("system/quit", json!({"cancel": true})).unwrap()["asked"],
        false
    );
    assert_eq!(s.quit, None);
    let addon: &dyn ControlAddon<Stub> = &Addon;
    assert!(
        addon
            .ui_signals()
            .contains(&("quit_dialog_shown", "system/quit"))
    );
}
