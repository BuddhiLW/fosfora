//! Pipeline: pure planning and the JSON views the scene ops return.

use serde_json::{Value, json};

use super::port::TimelineView;
use super::promote::{AdvanceKind, BEATS_RANGE, CueEdit, DEFAULT_BEATS, DEFAULT_HOLD_SECS};
use crate::control::args;
use crate::scene::types::{AdvanceMode, SceneCue};

/// The advance mode after a `{:advance :beats}` request, or `None` when it
/// asks for no change. `:beats` alone retunes a beat-synced timeline;
/// entering `beats` keeps the current count or takes [`DEFAULT_BEATS`].
pub fn plan_advance(
    current: AdvanceMode,
    kind: Option<AdvanceKind>,
    beats: Option<u32>,
) -> Result<Option<AdvanceMode>, String> {
    if let Some(b) = beats {
        if !(BEATS_RANGE.0..=BEATS_RANGE.1).contains(&b) {
            return Err(format!(
                ":beats must be in {}..{}",
                BEATS_RANGE.0, BEATS_RANGE.1
            ));
        }
    }
    let held = match current {
        AdvanceMode::BeatSync { beats_per_cue } => Some(beats_per_cue),
        _ => None,
    };
    match (kind, beats) {
        (None, None) => Ok(None),
        (Some(AdvanceKind::Manual), None) => Ok(Some(AdvanceMode::Manual)),
        (Some(AdvanceKind::Timer), None) => Ok(Some(AdvanceMode::Timer)),
        (Some(AdvanceKind::Beats), b) => Ok(Some(AdvanceMode::BeatSync {
            beats_per_cue: b.or(held).unwrap_or(DEFAULT_BEATS),
        })),
        (None, Some(b)) if held.is_some() => Ok(Some(AdvanceMode::BeatSync { beats_per_cue: b })),
        (_, Some(_)) => Err(":beats only applies with :advance beats".into()),
    }
}

/// Give every cue without a hold [`DEFAULT_HOLD_SECS`].
pub fn fill_timer_holds(cues: &mut [SceneCue]) {
    for cue in cues.iter_mut().filter(|c| c.hold_secs.is_none()) {
        cue.hold_secs = Some(DEFAULT_HOLD_SECS);
    }
}

/// Apply `edit` to `cue`.
pub fn apply_cue_edit(cue: &mut SceneCue, edit: &CueEdit) {
    if let Some(t) = edit.transition {
        cue.transition = t;
    }
    if let Some(s) = edit.secs {
        cue.transition_secs = s;
    }
    if let Some(h) = edit.hold {
        cue.hold_secs = h;
    }
    if let Some(l) = &edit.label {
        let l = l.trim();
        cue.label = (!l.is_empty()).then(|| l.to_owned());
    }
}

/// `{index preset label transition secs hold}`.
pub fn cue_json(index: usize, cue: &SceneCue) -> Value {
    json!({
        "index": index,
        "preset": cue.preset_name,
        "label": cue.label,
        "transition": args::transition_name(cue.transition),
        "secs": cue.transition_secs,
        "hold": cue.hold_secs,
    })
}

/// `{mode beats-per-cue}`.
pub fn advance_json(mode: AdvanceMode) -> Value {
    let beats = match mode {
        AdvanceMode::BeatSync { beats_per_cue } => Some(beats_per_cue),
        _ => None,
    };
    json!({"mode": AdvanceKind::of(mode).name(), "beats-per-cue": beats})
}

/// `[{index name cues current}]`.
pub fn listing(scenes: &[(String, usize)], current: Option<usize>) -> Value {
    Value::Array(
        scenes
            .iter()
            .enumerate()
            .map(|(i, (n, c))| json!({"index": i, "name": n, "cues": c, "current": current == Some(i)}))
            .collect(),
    )
}

/// `{index name cues}` for scene `index`, or nil.
pub fn named(scenes: &[(String, usize)], index: usize) -> Value {
    match scenes.get(index) {
        Some((n, c)) => json!({"index": index, "name": n, "cues": c}),
        None => Value::Null,
    }
}

/// `{playing cue transition}`: where playback is.
pub fn playback_json(t: &TimelineView) -> Value {
    json!({
        "playing": t.active,
        "cue": t.active.then_some(t.current_cue),
        "transition": t.transition.map(|(from, to, progress)| {
            json!({"from": from, "to": to, "progress": progress})
        }),
    })
}

/// `{loop advance}`.
pub fn timeline_json(t: &TimelineView) -> Value {
    json!({"loop": t.loop_mode, "advance": advance_json(t.advance)})
}

/// Everything `scene/current` reports: `scene` plus playback, loop,
/// advance and the cue list.
pub fn state_json(scene: Value, t: &TimelineView) -> Value {
    let mut v = playback_json(t);
    v["scene"] = scene;
    v["loop"] = json!(t.loop_mode);
    v["advance"] = advance_json(t.advance);
    v["cues"] = Value::Array(
        t.cues
            .iter()
            .enumerate()
            .map(|(i, c)| cue_json(i, c))
            .collect(),
    );
    v
}
