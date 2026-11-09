//! What the preset ops need from the app (ISP), and the one adapter that
//! reaches into [`App`] for it (DIP).

use crate::app::App;
use crate::scene::types::TransitionType;

/// The preset slice of the app. Indices are positions in the preset list,
/// built-ins first.
pub trait PresetPort {
    /// Every preset name, in list order.
    fn preset_names(&self) -> Vec<String>;
    /// How many leading presets are built-in (read-only).
    fn builtin_count(&self) -> usize;
    /// The loaded preset, if any.
    fn current_preset(&self) -> Option<usize>;
    /// The preset being switched to, else the current one.
    fn target_preset(&self) -> Option<usize>;
    /// Whether the live stack changed since the current preset was loaded.
    fn preset_dirty(&self) -> bool;
    /// Save the live stack as `name`; its index.
    fn save_preset(&mut self, name: &str) -> Result<usize, String>;
    /// Load with a cut.
    fn load_preset(&mut self, index: usize);
    /// Switch with the default transition.
    fn switch_preset(&mut self, index: usize);
    fn delete_preset(&mut self, index: usize) -> Result<(), String>;
    /// Copy preset `index` to a new user preset `name`; its index.
    fn copy_preset(&mut self, index: usize, name: &str) -> Result<usize, String>;
    /// Forget the current preset without touching the stack.
    fn deselect_preset(&mut self);
    /// Clear every layer and forget the current preset (the "New" button).
    fn new_preset(&mut self);
    /// Re-read the presets folder.
    fn rescan_presets(&mut self);
    /// The default switch transition: kind, seconds, whether a Dissolve keeps
    /// the outgoing preset moving.
    fn preset_transition(&self) -> (TransitionType, f32, bool);
    /// Change any part of the default switch transition (saved to Settings).
    fn set_preset_transition(
        &mut self,
        kind: Option<TransitionType>,
        secs: Option<f32>,
        keep_moving: Option<bool>,
    );
}

impl PresetPort for App {
    fn preset_names(&self) -> Vec<String> {
        self.preset_store
            .presets
            .iter()
            .map(|(n, _)| n.clone())
            .collect()
    }

    fn builtin_count(&self) -> usize {
        self.preset_store.builtin_count
    }

    fn current_preset(&self) -> Option<usize> {
        self.preset_store.current_preset
    }

    fn target_preset(&self) -> Option<usize> {
        App::target_preset(self)
    }

    fn preset_dirty(&self) -> bool {
        self.preset_store.dirty
    }

    fn save_preset(&mut self, name: &str) -> Result<usize, String> {
        self.try_save_preset(name).map_err(|e| e.to_string())
    }

    fn load_preset(&mut self, index: usize) {
        App::load_preset(self, index);
    }

    fn switch_preset(&mut self, index: usize) {
        App::switch_preset(self, index);
    }

    fn delete_preset(&mut self, index: usize) -> Result<(), String> {
        self.preset_store.delete(index).map_err(|e| e.to_string())
    }

    fn copy_preset(&mut self, index: usize, name: &str) -> Result<usize, String> {
        self.preset_store
            .copy_preset(index, name)
            .map_err(|e| e.to_string())
    }

    fn deselect_preset(&mut self) {
        self.preset_store.current_preset = None;
        self.preset_store.dirty = false;
    }

    fn new_preset(&mut self) {
        self.deselect_preset();
        self.clear_all_layers();
    }

    fn rescan_presets(&mut self) {
        self.preset_store.scan();
    }

    fn preset_transition(&self) -> (TransitionType, f32, bool) {
        (
            self.settings.preset_transition,
            self.settings.preset_transition_secs,
            self.settings.dissolve_keeps_moving,
        )
    }

    fn set_preset_transition(
        &mut self,
        kind: Option<TransitionType>,
        secs: Option<f32>,
        keep_moving: Option<bool>,
    ) {
        App::set_preset_transition(self, kind, secs, keep_moving);
    }
}
