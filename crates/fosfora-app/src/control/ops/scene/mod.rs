//! Scene ops: the Scenes panel and the cue timeline.
//!
//! [`promote`] (`AdvanceKind`, `CueEdit`, `Transport` from args) ->
//! [`pipeline`] (pure: `plan_advance`, `apply_cue_edit`, the JSON views) ->
//! Boundary here (one [`ScenePort`] / [`CuePort`] call per op). [`actions`]
//! holds the `App` methods the panel signals and the ops share.

pub mod actions;
pub mod pipeline;
pub mod port;
pub mod promote;
#[cfg(test)]
mod tests;

use serde_json::{Value, json};

pub use self::pipeline::*;
pub use self::port::{CuePort, ScenePort, TimelineView};
pub use self::promote::*;
use crate::control::{CljrsNs, ControlAddon, OpResult, OpSpec, args};

pub struct Addon;

impl<C: ScenePort + CuePort + 'static> ControlAddon<C> for Addon {
    fn id(&self) -> &'static str {
        "scene"
    }

    fn ops(&self) -> Vec<OpSpec<C>> {
        vec![
            OpSpec::new("scene/list", "{}", "Every scene: index, name, cues, current?.", list::<C>),
            OpSpec::new(
                "scene/current",
                "{}",
                "The open scene, playback state, loop, advance mode and cue list.",
                current::<C>,
            ),
            OpSpec::new(
                "scene/save",
                "{:name str}",
                "Save the timeline as a scene; a new name starts a blank cue list.",
                save::<C>,
            ),
            OpSpec::new("scene/open", "{:index int | :name str}", "Open a scene for editing without playing it.", open::<C>),
            OpSpec::new("scene/load", "{:index int | :name str}", "Open a scene and play it from its first cue.", load::<C>),
            OpSpec::new("scene/delete", "{:index int | :name str}", "Delete a scene file.", delete::<C>),
            OpSpec::new("scene/rename", "{:index int | :name str, :to str}", "Rename a scene.", rename::<C>),
            OpSpec::new("scene/duplicate", "{:index int | :name str}", "Copy a scene as \"<name> copy\".", duplicate::<C>),
            OpSpec::new(
                "scene/add-cue",
                "{:preset int|str?, :at int?}",
                "Add a cue for a preset (default: the loaded one), at :at or at the end.",
                add_cue::<C>,
            ),
            OpSpec::new("scene/remove-cue", "{:cue int}", "Remove a cue.", remove_cue::<C>),
            OpSpec::new("scene/move-cue", "{:from int, :to int}", "Move a cue; playback follows it.", move_cue::<C>),
            OpSpec::new(
                "scene/set-cue",
                "{:cue int, :transition cut|dissolve|morph?, :secs 0.1..30?, :hold 0.5..600|false?, :label str?}",
                "Read a cue, or change its transition, duration, hold (false clears) or label.",
                set_cue::<C>,
            ),
            OpSpec::new("scene/play", "{:cue int?}", "Play the timeline from a cue (default the first).", play::<C>),
            OpSpec::new("scene/stop", "{}", "Stop the timeline.", stop::<C>),
            OpSpec::new("scene/toggle", "{}", "Stop if playing, else play from the first cue.", toggle::<C>),
            OpSpec::new("scene/next", "{}", "Go to the next cue (wraps when looping).", next::<C>),
            OpSpec::new("scene/prev", "{}", "Go to the previous cue (wraps when looping).", prev::<C>),
            OpSpec::new("scene/jump", "{:cue int}", "Go to a cue, starting playback if stopped.", jump::<C>),
            OpSpec::new(
                "scene/timeline",
                "{:loop bool?, :advance manual|timer|beats?, :beats 1..64?}",
                "Read, or change, the loop flag and advance mode.",
                timeline::<C>,
            ),
        ]
    }

    fn ui_signals(&self) -> &'static [(&'static str, &'static str)] {
        &[
            ("save_scene", "scene/save"),
            ("open_scene", "scene/open"),
            ("load_scene", "scene/load"),
            ("delete_scene", "scene/delete"),
            ("scene_rename", "scene/rename"),
            ("scene_duplicate", "scene/duplicate"),
            ("scene_add_cue", "scene/add-cue"),
            ("scene_remove_cue", "scene/remove-cue"),
            ("scene_set_cue_transition", "scene/set-cue"),
            ("scene_set_cue_transition_secs", "scene/set-cue"),
            ("scene_set_cue_hold_secs", "scene/set-cue"),
            ("scene_toggle_play", "scene/toggle"),
            ("scene_go_next", "scene/next"),
            ("scene_go_prev", "scene/prev"),
            ("scene_jump_to_cue", "scene/jump"),
            ("scene_set_loop", "scene/timeline"),
            ("scene_set_advance_mode", "scene/timeline"),
            ("scene_set_beats_per_cue", "scene/timeline"),
        ]
    }

    fn cljrs(&self) -> Option<CljrsNs> {
        Some(CljrsNs {
            ns: "fosfora.scene",
            alias: "scene",
            src: include_str!("scene.cljrs"),
        })
    }
}

// ── Promote (args that need the port) ──────────────────────────────────────

/// The scene `{:index n}` or `{:name "x"}` names.
fn target<P: ScenePort>(port: &P, a: &Value) -> Result<usize, String> {
    let names: Vec<String> = port.scene_list().into_iter().map(|(n, _)| n).collect();
    args::pick(a, &names, "scene")
}

/// Cue `:cue` of the open scene.
fn cue_arg<P: CuePort>(port: &P, a: &Value, key: &str) -> Result<usize, String> {
    let i = args::usize(a, key)?;
    let len = port.timeline().cues.len();
    if i < len {
        Ok(i)
    } else {
        Err(format!("cue {i} out of range (have {len})"))
    }
}

/// The preset `:preset` names (index, exact or unique substring), else the
/// loaded one.
fn preset_arg<P: CuePort>(port: &P, a: &Value) -> Result<String, String> {
    let names = port.preset_names();
    let pick = match a.get("preset") {
        None | Some(Value::Null) => {
            return port
                .current_preset_name()
                .ok_or_else(|| "no preset is loaded; give :preset".to_owned());
        }
        Some(Value::String(s)) => json!({"name": s}),
        Some(n @ Value::Number(_)) => json!({"index": n}),
        Some(_) => return Err(":preset must be an index or a name".into()),
    };
    args::pick(&pick, &names, "preset").map(|i| names[i].clone())
}

// ── Boundary ───────────────────────────────────────────────────────────────
// Every op body has the `OpFn` signature, so infallible reads still return
// `OpResult`.

#[allow(clippy::unnecessary_wraps)]
fn list<P: ScenePort>(port: &mut P, _: &Value) -> OpResult {
    Ok(listing(&port.scene_list(), port.current_scene()))
}

#[allow(clippy::unnecessary_wraps)]
fn current<P: ScenePort + CuePort>(port: &mut P, _: &Value) -> OpResult {
    let scene = port
        .current_scene()
        .map_or(Value::Null, |i| named(&port.scene_list(), i));
    Ok(state_json(scene, &port.timeline()))
}

fn save<P: ScenePort>(port: &mut P, a: &Value) -> OpResult {
    let name = args::str(a, "name")?;
    let index = port.save_scene(name)?;
    Ok(named(&port.scene_list(), index))
}

fn open<P: ScenePort>(port: &mut P, a: &Value) -> OpResult {
    let index = target(port, a)?;
    port.open_scene(index)?;
    Ok(named(&port.scene_list(), index))
}

fn load<P: ScenePort + CuePort>(port: &mut P, a: &Value) -> OpResult {
    let index = target(port, a)?;
    port.play_scene(index)?;
    let mut v = named(&port.scene_list(), index);
    v["playback"] = playback_json(&port.timeline());
    Ok(v)
}

fn delete<P: ScenePort>(port: &mut P, a: &Value) -> OpResult {
    let index = target(port, a)?;
    let gone = named(&port.scene_list(), index);
    port.delete_scene(index)?;
    Ok(gone)
}

fn rename<P: ScenePort>(port: &mut P, a: &Value) -> OpResult {
    let index = target(port, a)?;
    let to = args::str(a, "to")?;
    let index = port.rename_scene(index, to)?;
    Ok(named(&port.scene_list(), index))
}

fn duplicate<P: ScenePort>(port: &mut P, a: &Value) -> OpResult {
    let index = target(port, a)?;
    let copy = port.duplicate_scene(index)?;
    Ok(named(&port.scene_list(), copy))
}

fn add_cue<P: CuePort>(port: &mut P, a: &Value) -> OpResult {
    let preset = preset_arg(port, a)?;
    let at = args::opt_usize(a, "at");
    if a.get("at").is_some_and(|v| !v.is_null()) && at.is_none() {
        return Err(":at must be a non-negative integer".into());
    }
    let index = port.insert_cue(at, &preset);
    let t = port.timeline();
    t.cues
        .get(index)
        .map(|c| cue_json(index, c))
        .ok_or_else(|| format!("cue {index} vanished after insert"))
}

fn remove_cue<P: CuePort>(port: &mut P, a: &Value) -> OpResult {
    let index = cue_arg(port, a, "cue")?;
    let gone = cue_json(index, &port.timeline().cues[index]);
    if !port.remove_cue(index) {
        return Err(format!("cue {index} could not be removed"));
    }
    Ok(gone)
}

fn move_cue<P: CuePort>(port: &mut P, a: &Value) -> OpResult {
    let from = cue_arg(port, a, "from")?;
    let to = cue_arg(port, a, "to")?;
    port.move_cue(from, to);
    let t = port.timeline();
    Ok(cue_json(to, &t.cues[to]))
}

fn set_cue<P: CuePort>(port: &mut P, a: &Value) -> OpResult {
    let index = cue_arg(port, a, "cue")?;
    let edit = CueEdit::from_args(a)?;
    if !edit.is_empty() && !port.edit_cue(index, &edit) {
        return Err(format!("cue {index} could not be edited"));
    }
    Ok(cue_json(index, &port.timeline().cues[index]))
}

/// Run `t` and report where playback is; op bodies return it as is.
#[allow(clippy::unnecessary_wraps)]
fn drive<P: CuePort>(port: &mut P, t: Transport) -> OpResult {
    port.transport(t);
    Ok(playback_json(&port.timeline()))
}

fn require_cues<P: CuePort>(port: &P) -> Result<(), String> {
    if port.timeline().cues.is_empty() {
        Err("the open scene has no cues".into())
    } else {
        Ok(())
    }
}

fn require_playing<P: CuePort>(port: &P) -> Result<(), String> {
    if port.timeline().active {
        Ok(())
    } else {
        Err("the timeline is stopped; (scene/play!) starts it".into())
    }
}

fn play<P: CuePort>(port: &mut P, a: &Value) -> OpResult {
    require_cues(port)?;
    let cue = match a.get("cue") {
        None | Some(Value::Null) => 0,
        Some(_) => cue_arg(port, a, "cue")?,
    };
    drive(port, Transport::Play(cue))
}

fn stop<P: CuePort>(port: &mut P, _: &Value) -> OpResult {
    drive(port, Transport::Stop)
}

fn toggle<P: CuePort>(port: &mut P, _: &Value) -> OpResult {
    if !port.timeline().active {
        require_cues(port)?;
    }
    drive(port, Transport::Toggle)
}

fn next<P: CuePort>(port: &mut P, _: &Value) -> OpResult {
    require_playing(port)?;
    drive(port, Transport::Next)
}

fn prev<P: CuePort>(port: &mut P, _: &Value) -> OpResult {
    require_playing(port)?;
    drive(port, Transport::Prev)
}

fn jump<P: CuePort>(port: &mut P, a: &Value) -> OpResult {
    let cue = cue_arg(port, a, "cue")?;
    drive(port, Transport::Jump(cue))
}

fn timeline<P: CuePort>(port: &mut P, a: &Value) -> OpResult {
    let loop_mode = match a.get("loop") {
        None | Some(Value::Null) => None,
        Some(_) => Some(args::bool(a, "loop")?),
    };
    let kind = args::opt_str(a, "advance").map(AdvanceKind::parse).transpose()?;
    if a.get("advance").is_some_and(|v| !v.is_null() && !v.is_string()) {
        return Err(":advance must be manual, timer or beats".into());
    }
    let beats = match a.get("beats") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .and_then(|b| u32::try_from(b).ok())
                .ok_or(":beats must be a positive integer")?,
        ),
    };
    let advance = plan_advance(port.timeline().advance, kind, beats)?;
    if loop_mode.is_some() || advance.is_some() {
        port.configure_timeline(loop_mode, advance);
    }
    Ok(timeline_json(&port.timeline()))
}
