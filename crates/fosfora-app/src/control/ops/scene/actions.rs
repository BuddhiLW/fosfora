//! Scene actions on [`App`], shared by the Scenes panel signals (`main.rs`)
//! and the scene ops, so a click and an op take the same code path.
//! None of these autosave; the caller does, once per batch of edits.

use super::{CueEdit, apply_cue_edit, fill_timer_holds};
use crate::app::App;
use crate::scene::types::{AdvanceMode, SceneSet};

impl App {
    /// Save the timeline as scene `name`. A name no scene has yet starts a
    /// blank, stopped, manual, non-looping cue list. Returns its index.
    pub fn try_save_scene(&mut self, name: &str) -> anyhow::Result<usize> {
        let is_new = !self.scene_store.scenes.iter().any(|(n, _)| n == name);
        if is_new {
            self.timeline.cues.clear();
            self.timeline.stop();
            self.timeline.loop_mode = false;
            self.timeline.advance_mode = AdvanceMode::Manual;
        }
        let set = SceneSet {
            version: 1,
            name: name.to_owned(),
            cues: self.timeline.cues.clone(),
            loop_mode: self.timeline.loop_mode,
            advance_mode: self.timeline.advance_mode,
        };
        self.scene_store.save(name, set)
    }

    /// [`App::try_save_scene`], logging a failure.
    pub fn save_scene(&mut self, name: &str) {
        if let Err(e) = self.try_save_scene(name) {
            log::error!("Failed to save scene: {e}");
        }
    }

    /// Advance to the next cue (no-op while stopped).
    pub fn scene_go_next(&mut self) {
        let event = self.timeline.go_next();
        self.process_timeline_event(event);
    }

    /// Back to the previous cue (no-op while stopped).
    pub fn scene_go_prev(&mut self) {
        let event = self.timeline.go_prev();
        self.process_timeline_event(event);
    }

    /// Jump to cue `index`, starting playback if stopped.
    pub fn scene_jump_to_cue(&mut self, index: usize) {
        let event = self.timeline.go_to_cue(index);
        self.process_timeline_event(event);
    }

    /// Stop if playing, else play from the first cue (if there is one).
    pub fn toggle_scene_play(&mut self) {
        if self.timeline.active {
            self.timeline.stop();
        } else if !self.timeline.cues.is_empty() {
            let event = self.timeline.start(0);
            self.process_timeline_event(event);
        }
    }

    /// Append a cue for `preset`; its index.
    pub fn add_scene_cue(&mut self, preset: String) -> usize {
        let cue = self.new_cue(preset);
        self.timeline.cues.push(cue);
        self.timeline.cues.len() - 1
    }

    /// Insert a cue for `preset` at `at` (clamped to the end); its index.
    pub fn insert_scene_cue(&mut self, at: usize, preset: String) -> usize {
        let at = at.min(self.timeline.cues.len());
        let cue = self.new_cue(preset);
        self.timeline.insert_cue(at, cue);
        at
    }

    /// Remove cue `index`; false when there is no such cue.
    pub fn remove_scene_cue(&mut self, index: usize) -> bool {
        if index >= self.timeline.cues.len() {
            return false;
        }
        self.timeline.cues.remove(index);
        self.timeline.notify_cue_removed(index);
        true
    }

    /// Apply `edit` to cue `index`; false when there is no such cue.
    pub fn edit_scene_cue(&mut self, index: usize, edit: &CueEdit) -> bool {
        match self.timeline.cues.get_mut(index) {
            Some(cue) => {
                apply_cue_edit(cue, edit);
                true
            }
            None => false,
        }
    }

    pub fn set_scene_loop(&mut self, loop_mode: bool) {
        self.timeline.loop_mode = loop_mode;
    }

    /// Change the advance mode. Entering Timer gives every cue without a
    /// hold the default one, so the timer can advance.
    pub fn set_scene_advance_mode(&mut self, mode: AdvanceMode) {
        if mode == AdvanceMode::Timer {
            fill_timer_holds(&mut self.timeline.cues);
        }
        self.timeline.advance_mode = mode;
    }

    /// Set beats per cue; false (and no change) unless the timeline is
    /// beat-synced.
    pub fn set_scene_beats_per_cue(&mut self, beats: u32) -> bool {
        match &mut self.timeline.advance_mode {
            AdvanceMode::BeatSync { beats_per_cue } => {
                *beats_per_cue = beats;
                true
            }
            _ => false,
        }
    }
}
