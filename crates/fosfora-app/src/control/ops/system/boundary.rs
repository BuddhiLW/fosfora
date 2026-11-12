//! Op bodies (Boundary): promote the args, make one port call that acts,
//! answer with the state it left. Generic over the port each needs.

// Every op body has the `OpFn` signature, infallible ones included.
#![allow(clippy::unnecessary_wraps)]

use serde_json::{Value, json};

use super::model::{
    self, LinkChange, OutputKind, RecordingChange, SettingsChange, TempoChange, Workspace,
};
use super::port::{OutputPort, RecordingPort, SettingsPort, ShellPort, TempoPort};
use crate::audio::beat::TempoCommand;
use crate::control::{OpResult, args};
use crate::midi::types::TriggerAction;

// ── Tempo and Link ───────────────────────────────────────────────────────

fn tempo_now<P: TempoPort>(port: &P) -> Value {
    let (cfg, bpm) = port.tempo_state();
    model::tempo_json(&cfg, bpm)
}

pub fn tempo<P: TempoPort>(port: &mut P, _: &Value) -> OpResult {
    Ok(tempo_now(port))
}

pub fn tempo_config<P: TempoPort>(port: &mut P, a: &Value) -> OpResult {
    let change = TempoChange::from_args(a)?;
    if !change.is_empty() {
        let (cfg, _) = port.tempo_state();
        port.set_tempo_config(model::apply_tempo_change(cfg, &change));
    }
    Ok(tempo_now(port))
}

pub fn tempo_tap<P: TempoPort>(port: &mut P, _: &Value) -> OpResult {
    port.tap_tempo();
    Ok(tempo_now(port))
}

pub fn tempo_set<P: TempoPort>(port: &mut P, a: &Value) -> OpResult {
    let bpm = a
        .get("bpm")
        .and_then(Value::as_f64)
        .ok_or("missing number arg :bpm")?;
    let bpm = model::tempo_bpm(bpm)?;
    port.tempo_command(TempoCommand::Tap(bpm));
    Ok(json!({"bpm": bpm}))
}

fn shift<P: TempoPort>(port: &mut P, octaves: i32) -> OpResult {
    port.tempo_command(TempoCommand::ShiftOctave(octaves));
    Ok(tempo_now(port))
}

pub fn tempo_half<P: TempoPort>(port: &mut P, _: &Value) -> OpResult {
    shift(port, -1)
}

pub fn tempo_double<P: TempoPort>(port: &mut P, _: &Value) -> OpResult {
    shift(port, 1)
}

pub fn link<P: TempoPort>(port: &mut P, a: &Value) -> OpResult {
    let change = LinkChange::from_args(a)?;
    if !change.is_empty() {
        port.set_link(&change)?;
    }
    Ok(port.link_state()?.to_json())
}

// ── Recording ────────────────────────────────────────────────────────────

fn recording_now<P: RecordingPort>(port: &P) -> Value {
    let mut v = port.recording_status().to_json();
    v["config"] = model::recording_config_json(&port.recording_config());
    v
}

pub fn recording<P: RecordingPort>(port: &mut P, _: &Value) -> OpResult {
    Ok(recording_now(port))
}

pub fn record_start<P: RecordingPort>(port: &mut P, _: &Value) -> OpResult {
    if port.recording_status().recording {
        return Err("already recording; system/record-stop first".into());
    }
    let path = port.start_recording()?;
    Ok(json!({"recording": true, "path": path.display().to_string()}))
}

pub fn record_stop<P: RecordingPort>(port: &mut P, _: &Value) -> OpResult {
    port.stop_recording();
    Ok(recording_now(port))
}

pub fn record_toggle<P: RecordingPort>(port: &mut P, a: &Value) -> OpResult {
    if port.recording_status().recording {
        record_stop(port, a)
    } else {
        record_start(port, a)
    }
}

pub fn recording_config<P: RecordingPort>(port: &mut P, a: &Value) -> OpResult {
    let change = RecordingChange::from_args(a)?;
    if !change.is_empty() {
        port.configure_recording(&change);
    }
    Ok(model::recording_config_json(&port.recording_config()))
}

// ── Outputs ──────────────────────────────────────────────────────────────

pub fn outputs<P: OutputPort>(port: &mut P, _: &Value) -> OpResult {
    Ok(Value::Array(
        OutputKind::ALL
            .iter()
            .map(|&k| port.output_status(k).to_json())
            .collect(),
    ))
}

pub fn output<P: OutputPort>(port: &mut P, a: &Value) -> OpResult {
    let raw = args::str(a, "kind")?;
    let kind = model::parse_named(raw, OutputKind::ALL, OutputKind::name, ":kind")?;
    let changes = model::output_changes(kind, a)?;
    let enable = args::opt_bool(a, "enabled");
    let available = port.output_status(kind).available;
    if !available && (enable.is_some() || !changes.is_empty()) {
        return Err(model::not_built(kind));
    }
    if !changes.is_empty() {
        port.configure_output(kind, &changes)?;
    }
    if let Some(on) = enable {
        port.set_output_enabled(kind, on)?;
    }
    Ok(port.output_status(kind).to_json())
}

pub fn v4l2_refresh<P: OutputPort>(port: &mut P, _: &Value) -> OpResult {
    port.refresh_v4l2_devices()?;
    Ok(port.output_status(OutputKind::V4l2).to_json())
}

fn window_now<P: OutputPort>(port: &P) -> Value {
    let (displays, open_on) = port.output_window();
    json!({"displays": displays, "open-on": open_on})
}

pub fn output_window<P: OutputPort>(port: &mut P, a: &Value) -> OpResult {
    if args::opt_bool(a, "close").unwrap_or(false) {
        port.close_output_window();
        return Ok(window_now(port));
    }
    if a.get("index").is_none() && a.get("name").is_none() {
        return Ok(window_now(port));
    }
    let (displays, _) = port.output_window();
    let index = args::pick(a, &displays, "display")?;
    port.open_output_window(index);
    Ok(json!({"requested": displays[index], "index": index}))
}

// ── Settings ─────────────────────────────────────────────────────────────

pub fn settings<P: SettingsPort>(port: &mut P, a: &Value) -> OpResult {
    let change = SettingsChange::from_args(a)?;
    if let Some(l) = change.flash_limit {
        port.set_flash_limit(l);
    }
    if let Some(m) = change.output_alpha {
        port.set_output_alpha(m);
    }
    if let Some(on) = change.auto_reconnect {
        port.set_auto_reconnect(on);
    }
    Ok(port.settings_view().to_json())
}

pub fn theme<P: SettingsPort>(port: &mut P, a: &Value) -> OpResult {
    let (names, current) = port.themes();
    if a.get("index").is_none() && a.get("name").is_none() {
        return Ok(model::listing(&names, current));
    }
    let index = args::pick(a, &names, "theme")?;
    port.set_theme(index);
    Ok(json!({"index": index, "name": names[index]}))
}

pub fn web<P: SettingsPort>(port: &mut P, a: &Value) -> OpResult {
    let enabled = args::opt_bool(a, "enabled");
    let lan = args::opt_bool(a, "lan");
    let new_port = args::opt_port(a, "port")?;
    if let Some(p) = new_port.filter(|&p| p != port.web_status().port) {
        port.set_web_port(p);
    }
    if enabled.is_some() || lan.is_some() {
        port.set_web(enabled, lan);
    }
    Ok(port.web_status().to_json())
}

pub fn ui_scale<P: SettingsPort>(port: &mut P, a: &Value) -> OpResult {
    match model::ui_scale_arg(a)? {
        Some(s) => {
            port.set_ui_scale(s);
            Ok(json!({"scale": s}))
        }
        None => Ok(json!({"scale": port.ui_scale()})),
    }
}

pub fn theme_reload<P: SettingsPort>(port: &mut P, _: &Value) -> OpResult {
    port.reload_themes();
    let (names, current) = port.themes();
    Ok(model::listing(&names, current))
}

pub fn theme_save_copy<P: SettingsPort>(port: &mut P, _: &Value) -> OpResult {
    let name = port.save_theme_copy()?;
    let (names, current) = port.themes();
    Ok(json!({"name": name, "index": current, "count": names.len()}))
}

pub fn theme_folder<P: SettingsPort>(port: &mut P, a: &Value) -> OpResult {
    let dir = port.themes_folder().display().to_string();
    let open = args::opt_bool(a, "open").unwrap_or(false);
    if open {
        port.open_themes_folder()?;
    }
    Ok(json!({"dir": dir, "opened": open}))
}

pub fn web_replace_key<P: SettingsPort>(port: &mut P, _: &Value) -> OpResult {
    port.replace_web_key();
    Ok(port.web_status().to_json())
}

// ── Window, view, shortcuts ──────────────────────────────────────────────

pub fn view<P: ShellPort>(port: &mut P, _: &Value) -> OpResult {
    Ok(port.view().to_json())
}

pub fn overlay<P: ShellPort>(port: &mut P, a: &Value) -> OpResult {
    let on = model::toggle_or(port.view().overlay, args::opt_bool(a, "visible"));
    port.set_overlay_visible(on);
    Ok(port.view().to_json())
}

pub fn fullscreen<P: ShellPort>(port: &mut P, a: &Value) -> OpResult {
    let on = model::toggle_or(port.view().fullscreen, args::opt_bool(a, "on"));
    port.set_fullscreen(on);
    Ok(port.view().to_json())
}

pub fn binding_matrix<P: ShellPort>(port: &mut P, a: &Value) -> OpResult {
    let open = model::toggle_or(port.view().binding_matrix, args::opt_bool(a, "open"));
    port.set_binding_matrix_open(open);
    Ok(port.view().to_json())
}

pub fn chain_editor<P: ShellPort>(port: &mut P, a: &Value) -> OpResult {
    let open = model::toggle_or(port.view().chain_editor, args::opt_bool(a, "open"));
    port.set_chain_editor_open(open);
    Ok(port.view().to_json())
}

pub fn workspace<P: ShellPort>(port: &mut P, a: &Value) -> OpResult {
    if let Some(ws) = model::opt_named(a, "name", Workspace::ALL, Workspace::name)? {
        port.set_workspace(ws);
    }
    Ok(json!(port.view().workspace.name()))
}

fn tours_now<P: ShellPort>(port: &P) -> Value {
    let current = port.current_tour().map(|(i, _)| i);
    model::tour_listing(&port.tour_names(), current, &port.tours_taken())
}

pub fn tours<P: ShellPort>(port: &mut P, _: &Value) -> OpResult {
    Ok(tours_now(port))
}

fn tour_now<P: ShellPort>(port: &P) -> Value {
    let names = port.tour_names();
    match port.current_tour() {
        Some((i, step)) => json!({"index": i, "name": names.get(i), "step": step, "running": true}),
        None => json!({"running": false}),
    }
}

pub fn tour<P: ShellPort>(port: &mut P, a: &Value) -> OpResult {
    if args::opt_bool(a, "end").unwrap_or(false) {
        port.end_tour();
        return Ok(tour_now(port));
    }
    if a.get("index").is_none() && a.get("name").is_none() {
        return Ok(tour_now(port));
    }
    let names = port.tour_names();
    let index = args::pick(a, &names, "tour")?;
    port.start_tour(index);
    Ok(json!({"index": index, "name": names[index], "step": 0, "running": true}))
}

pub fn escape<P: ShellPort>(port: &mut P, _: &Value) -> OpResult {
    let did = port.escape();
    let mut v = port.view().to_json();
    v["did"] = json!(did);
    Ok(v)
}

pub fn quit<P: ShellPort>(port: &mut P, a: &Value) -> OpResult {
    if args::opt_bool(a, "cancel").unwrap_or(false) {
        port.cancel_quit();
        return Ok(json!({"quitting": false, "asked": false}));
    }
    let confirm = args::opt_bool(a, "confirm").unwrap_or(false);
    port.request_quit(confirm);
    Ok(json!({"quitting": confirm, "asked": !confirm}))
}

// ── Triggers ─────────────────────────────────────────────────────────────

pub fn triggers<P: ShellPort>(_: &mut P, _: &Value) -> OpResult {
    Ok(json!(
        TriggerAction::ALL
            .iter()
            .map(|&t| model::trigger_name(t))
            .collect::<Vec<_>>()
    ))
}

pub fn trigger<P: ShellPort>(port: &mut P, a: &Value) -> OpResult {
    let raw = args::str(a, "action")?;
    let action = model::parse_named(raw, TriggerAction::ALL, model::trigger_name, ":action")?;
    port.run_trigger(action);
    Ok(json!(model::trigger_name(action)))
}
