//! The theme and interface-scale actions the Appearance page and the ops
//! share: `main.rs` turns a `ThemeIntent` into one of these calls, and the
//! [`super::port`] adapters call them too.

use std::path::PathBuf;

use crate::app::App;
use crate::ui::theme::{ThemeMode, custom};

impl App {
    /// Read the themes folder again, and keep drawing with the theme in use
    /// (re-read, if it is a file).
    pub fn reload_themes(&mut self) {
        self.custom_themes = custom::load_dir(&custom::themes_dir());
        custom::publish(&self.egui_overlay.context(), &self.custom_themes);
        self.apply_theme(self.settings.theme.clone());
    }

    /// Save the theme in use as a new file in the themes folder and switch to
    /// it; the new theme. The error is the one the status bar shows.
    pub fn save_theme_copy(&mut self) -> Result<ThemeMode, String> {
        let name = format!(
            "{} copy",
            self.settings.theme.display_name(&self.custom_themes)
        );
        let path = custom::write_new(&custom::themes_dir(), &name, &self.egui_overlay.palette)
            .map_err(|e| format!("Could not save the theme: {e}"))?;
        log::info!("theme saved to {}", path.display());
        self.custom_themes = custom::load_dir(&custom::themes_dir());
        custom::publish(&self.egui_overlay.context(), &self.custom_themes);
        let theme = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| ThemeMode::Custom(s.into()))
            .ok_or_else(|| format!("Could not save the theme: odd file name {}", path.display()))?;
        self.apply_theme(theme.clone());
        Ok(theme)
    }

    /// Show the themes folder in the system file browser; the folder.
    pub fn open_themes_folder(&mut self) -> Result<PathBuf, String> {
        let dir = custom::themes_dir();
        custom::reveal(&dir).map_err(|e| format!("Could not open the themes folder: {e}"))?;
        Ok(dir)
    }

    /// Hand the interface scale (1.0 = 100 %) to the frame loop, as the
    /// Appearance slider does on release; it clamps, applies and saves it.
    pub fn request_ui_scale(&mut self, scale: f32) {
        use crate::ui::panels::appearance_panel::SET_UI_SCALE;
        self.egui_overlay
            .context()
            .data_mut(|d| d.insert_temp(egui::Id::new(SET_UI_SCALE), scale));
    }

    /// The quit dialog's Cancel: stop asking. The frame loop then clears the
    /// dialog's `quit_dialog_shown` flag.
    pub fn cancel_quit(&mut self) {
        self.quit_requested = false;
    }
}
