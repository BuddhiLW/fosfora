//! What the scene ops need from the app (ISP), split by role: the scene
//! library on disk ([`ScenePort`]) and the cue timeline ([`CuePort`]), plus
//! the one adapter that reaches into [`App`] for each (DIP).

use super::{CueEdit, Transport};
use crate::app::App;
use crate::scene::types::{AdvanceMode, SceneCue};

/// A read-only copy of the cue timeline.
#[derive(Debug, Clone, Default)]
pub struct TimelineView {
    pub cues: Vec<SceneCue>,
    /// Whether the timeline is playing.
    pub active: bool,
    /// The cue being held, or transitioned to.
    pub current_cue: usize,
    pub loop_mode: bool,
    pub advance: AdvanceMode,
    /// `(from, to, progress 0..1)` while a transition runs.
    pub transition: Option<(usize, usize, f32)>,
}

/// The scene library: scene files, by position in the sorted list.
pub trait ScenePort {
    /// Every scene as `(name, cue count)`, in list order.
    fn scene_list(&self) -> Vec<(String, usize)>;
    /// The scene being edited, if any.
    fn current_scene(&self) -> Option<usize>;
    /// Save the timeline as scene `name` (a new name starts a blank cue
    /// list, as the Save button does); its index.
    fn save_scene(&mut self, name: &str) -> Result<usize, String>;
    /// Make scene `index` the one being edited, without playing it.
    fn open_scene(&mut self, index: usize) -> Result<(), String>;
    /// Open scene `index` and play it from its first cue.
    fn play_scene(&mut self, index: usize) -> Result<(), String>;
    fn delete_scene(&mut self, index: usize) -> Result<(), String>;
    /// Rename scene `index`; its index after the rename.
    fn rename_scene(&mut self, index: usize, name: &str) -> Result<usize, String>;
    /// Copy scene `index` as "<name> copy"; the copy's index.
    fn duplicate_scene(&mut self, index: usize) -> Result<usize, String>;
}

/// The cue timeline of the open scene. Every edit autosaves the open scene.
pub trait CuePort {
    fn timeline(&self) -> TimelineView;
    /// Every preset name, in list order.
    fn preset_names(&self) -> Vec<String>;
    /// The loaded preset's name, if any.
    fn current_preset_name(&self) -> Option<String>;
    /// Add a cue for `preset` at `at` (clamped), or at the end; its index.
    fn insert_cue(&mut self, at: Option<usize>, preset: &str) -> usize;
    /// Remove cue `index`; false when there is no such cue.
    fn remove_cue(&mut self, index: usize) -> bool;
    /// Move cue `from` so it lands at `to`; playback follows the cue.
    fn move_cue(&mut self, from: usize, to: usize);
    /// Apply `edit` to cue `index`; false when there is no such cue.
    fn edit_cue(&mut self, index: usize, edit: &CueEdit) -> bool;
    /// Change the loop flag and/or the advance mode.
    fn configure_timeline(&mut self, loop_mode: Option<bool>, advance: Option<AdvanceMode>);
    /// Drive playback.
    fn transport(&mut self, t: Transport);
}

impl ScenePort for App {
    fn scene_list(&self) -> Vec<(String, usize)> {
        self.scene_store
            .scenes
            .iter()
            .map(|(n, s)| (n.clone(), s.cues.len()))
            .collect()
    }

    fn current_scene(&self) -> Option<usize> {
        self.scene_store.current_scene
    }

    fn save_scene(&mut self, name: &str) -> Result<usize, String> {
        self.try_save_scene(name).map_err(|e| e.to_string())
    }

    fn open_scene(&mut self, index: usize) -> Result<(), String> {
        if App::open_scene(self, index) {
            Ok(())
        } else {
            Err(format!("no scene at index {index}"))
        }
    }

    fn play_scene(&mut self, index: usize) -> Result<(), String> {
        if index >= self.scene_store.scenes.len() {
            return Err(format!("no scene at index {index}"));
        }
        App::load_scene(self, index);
        Ok(())
    }

    fn delete_scene(&mut self, index: usize) -> Result<(), String> {
        self.scene_store.delete(index).map_err(|e| e.to_string())
    }

    fn rename_scene(&mut self, index: usize, name: &str) -> Result<usize, String> {
        self.scene_store
            .rename(index, name)
            .map_err(|e| e.to_string())
    }

    fn duplicate_scene(&mut self, index: usize) -> Result<usize, String> {
        self.scene_store.duplicate(index).map_err(|e| e.to_string())
    }
}

impl CuePort for App {
    fn timeline(&self) -> TimelineView {
        use crate::scene::timeline::TimelineInfoState;
        let info = self.timeline.info();
        TimelineView {
            cues: self.timeline.cues.clone(),
            active: info.active,
            current_cue: info.current_cue,
            loop_mode: info.loop_mode,
            advance: info.advance_mode,
            transition: match info.state {
                TimelineInfoState::Transitioning {
                    from, to, progress, ..
                } => Some((from, to, progress)),
                _ => None,
            },
        }
    }

    fn preset_names(&self) -> Vec<String> {
        self.preset_store
            .presets
            .iter()
            .map(|(n, _)| n.clone())
            .collect()
    }

    fn current_preset_name(&self) -> Option<String> {
        self.preset_store
            .current_preset
            .and_then(|i| self.preset_store.presets.get(i))
            .map(|(n, _)| n.clone())
    }

    fn insert_cue(&mut self, at: Option<usize>, preset: &str) -> usize {
        let index = match at {
            Some(at) => self.insert_scene_cue(at, preset.to_owned()),
            None => self.add_scene_cue(preset.to_owned()),
        };
        self.autosave_scene();
        index
    }

    fn remove_cue(&mut self, index: usize) -> bool {
        let removed = self.remove_scene_cue(index);
        if removed {
            self.autosave_scene();
        }
        removed
    }

    fn move_cue(&mut self, from: usize, to: usize) {
        self.timeline.move_cue(from, to);
        self.autosave_scene();
    }

    fn edit_cue(&mut self, index: usize, edit: &CueEdit) -> bool {
        let edited = self.edit_scene_cue(index, edit);
        if edited {
            self.autosave_scene();
        }
        edited
    }

    fn configure_timeline(&mut self, loop_mode: Option<bool>, advance: Option<AdvanceMode>) {
        if let Some(l) = loop_mode {
            self.set_scene_loop(l);
        }
        if let Some(mode) = advance {
            self.set_scene_advance_mode(mode);
        }
        self.autosave_scene();
    }

    fn transport(&mut self, t: Transport) {
        match t {
            Transport::Next => self.scene_go_next(),
            Transport::Prev => self.scene_go_prev(),
            Transport::Jump(i) => self.scene_jump_to_cue(i),
            Transport::Toggle => self.toggle_scene_play(),
            Transport::Play(i) => {
                let event = self.timeline.start(i);
                self.process_timeline_event(event);
            }
            Transport::Stop => self.timeline.stop(),
        }
    }
}
