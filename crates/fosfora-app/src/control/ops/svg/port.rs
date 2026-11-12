//! What the SVG Offset ops need from the app (ISP), and the one adapter that
//! reaches into [`App`] for it (DIP). The action itself lives in
//! `actions.rs`, shared with `main.rs`.

use crate::app::App;
use crate::ui::panels::svg_offset_panel::{SvgOffsetCommand, SvgOffsetInfo};

/// The SVG Offset slice of the app. Every op addresses one layer.
pub trait SvgPort {
    fn active_layer(&self) -> usize;
    fn layer_count(&self) -> usize;
    /// Source path (`None` = built-in ornament), shape count and settings of
    /// `layer`'s SVG Offset sim; `None` when the layer has none.
    fn svg_state(&self, layer: usize) -> Option<SvgOffsetInfo>;
    /// Run a panel command on `layer` (see [`App::apply_svg_offset_command`]).
    /// A reload from a path that is not a file is refused before loading.
    fn apply_svg(&mut self, layer: usize, cmd: SvgOffsetCommand) -> Result<(), String>;
}

impl SvgPort for App {
    fn active_layer(&self) -> usize {
        self.layer_stack.active_layer
    }

    fn layer_count(&self) -> usize {
        self.layer_stack.layers.len()
    }

    fn svg_state(&self, layer: usize) -> Option<SvgOffsetInfo> {
        let svg = self
            .layer_stack
            .layers
            .get(layer)?
            .as_effect()?
            .pass_executor
            .svg_offset
            .as_ref()?;
        Some(SvgOffsetInfo {
            path: svg.path.clone(),
            shapes: svg.shape_count(),
            settings: svg.settings.clone(),
        })
    }

    fn apply_svg(&mut self, layer: usize, cmd: SvgOffsetCommand) -> Result<(), String> {
        if let Some(p) = cmd.path.as_deref().filter(|_| cmd.reload) {
            if !std::path::Path::new(p).is_file() {
                return Err(format!("no such file: {p}"));
            }
        }
        self.apply_svg_offset_command(layer, cmd)
    }
}
