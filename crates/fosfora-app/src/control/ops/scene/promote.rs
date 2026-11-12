//! Promote: scene op args into domain values, and the ranges the cue strip
//! allows.

use serde_json::Value;

use crate::control::args;
use crate::scene::types::{AdvanceMode, TransitionType};

/// Transition seconds the cue strip allows.
pub const SECS_RANGE: (f32, f32) = (0.1, 30.0);
/// Hold seconds the cue strip allows.
pub const HOLD_RANGE: (f32, f32) = (0.5, 600.0);
/// Beats per cue the cue strip allows.
pub const BEATS_RANGE: (u32, u32) = (1, 64);
/// The hold a cue gets when the timeline enters Timer mode without one.
pub const DEFAULT_HOLD_SECS: f32 = 4.0;
/// Beats per cue when entering BeatSync without a count.
pub const DEFAULT_BEATS: u32 = 4;

/// Advance mode without its parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvanceKind {
    Manual,
    Timer,
    Beats,
}

impl AdvanceKind {
    pub const ALL: &[AdvanceKind] = &[Self::Manual, Self::Timer, Self::Beats];

    /// `manual`, `timer`, `beats` (or `beat-sync`), any case.
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw
            .to_ascii_lowercase()
            .replace(['-', ' ', '_'], "")
            .as_str()
        {
            "manual" => Ok(Self::Manual),
            "timer" => Ok(Self::Timer),
            "beats" | "beatsync" | "beat" => Ok(Self::Beats),
            _ => Err(format!(
                ":advance must be manual, timer or beats, got {raw:?}"
            )),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Timer => "timer",
            Self::Beats => "beats",
        }
    }

    pub fn of(mode: AdvanceMode) -> Self {
        match mode {
            AdvanceMode::Manual => Self::Manual,
            AdvanceMode::Timer => Self::Timer,
            AdvanceMode::BeatSync { .. } => Self::Beats,
        }
    }
}

/// The advance mode the cue strip's mode selector sends (`0` manual,
/// `1` timer, `2` beat-synced at [`DEFAULT_BEATS`], anything else manual).
pub fn advance_from_ui_id(id: u32) -> AdvanceMode {
    match id {
        1 => AdvanceMode::Timer,
        2 => AdvanceMode::BeatSync {
            beats_per_cue: DEFAULT_BEATS,
        },
        _ => AdvanceMode::Manual,
    }
}

/// A change to one cue; `None` fields stay as they are.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CueEdit {
    pub transition: Option<TransitionType>,
    pub secs: Option<f32>,
    /// `Some(None)` clears the hold (the cue waits for Go).
    pub hold: Option<Option<f32>>,
    /// Trimmed; empty clears the label.
    pub label: Option<String>,
}

impl CueEdit {
    pub fn transition(t: TransitionType) -> Self {
        Self {
            transition: Some(t),
            ..Self::default()
        }
    }
    pub fn secs(s: f32) -> Self {
        Self {
            secs: Some(s),
            ..Self::default()
        }
    }
    pub fn hold(h: Option<f32>) -> Self {
        Self {
            hold: Some(h),
            ..Self::default()
        }
    }
    pub fn label(l: impl Into<String>) -> Self {
        Self {
            label: Some(l.into()),
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// From `{:transition :secs :hold :label}`; values outside the cue
    /// strip's ranges are refused. `:hold false` (or nil) clears the hold.
    pub fn from_args(a: &Value) -> Result<Self, String> {
        let transition = args::opt_transition(a, "transition")?;
        let secs = match a.get("secs") {
            None | Some(Value::Null) => None,
            Some(_) => Some(in_range(args::opt_f32(a, "secs"), SECS_RANGE, "secs")?),
        };
        let hold = match a.get("hold") {
            None => None,
            Some(Value::Null | Value::Bool(false)) => Some(None),
            Some(_) => Some(Some(in_range(
                args::opt_f32(a, "hold"),
                HOLD_RANGE,
                "hold",
            )?)),
        };
        let label = match a.get("label") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(_) => return Err(":label must be a string".into()),
        };
        Ok(Self {
            transition,
            secs,
            hold,
            label,
        })
    }
}

fn in_range(v: Option<f32>, (lo, hi): (f32, f32), key: &str) -> Result<f32, String> {
    v.filter(|v| v.is_finite() && (lo..=hi).contains(v))
        .ok_or_else(|| format!(":{key} must be a number in {lo}..{hi}"))
}

/// A playback command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Next,
    Prev,
    Jump(usize),
    Toggle,
    Play(usize),
    Stop,
}
