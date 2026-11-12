//! The App action behind the SVG Offset panel. `main.rs`'s `svg_offset_cmd`
//! handler and the `svg/*` ops both call it, so a click and an op run the
//! same code.

use crate::app::App;
use crate::ui::panels::svg_offset_panel::SvgOffsetCommand;

/// The message for a layer without an SVG Offset sim.
pub(crate) fn no_svg(layer: usize) -> String {
    format!("layer {layer} has no SVG Offset")
}

impl App {
    /// Apply an SVG Offset panel command to `layer`: new settings and, with
    /// `cmd.reload`, the SVG re-read from `cmd.path` (`None` is the built-in
    /// ornament) at the sim's output size. On success the layer's shader error
    /// clears and the preset is marked dirty; a failed load is logged and
    /// shown as the layer's shader error. Bind groups are rebuilt either way.
    /// Fails when the layer has no SVG Offset sim or the load fails.
    pub fn apply_svg_offset_command(
        &mut self,
        layer: usize,
        cmd: SvgOffsetCommand,
    ) -> Result<(), String> {
        let e = self
            .layer_stack
            .layers
            .get_mut(layer)
            .ok_or_else(|| format!("there is no layer {layer}"))?
            .as_effect_mut()
            .ok_or_else(|| no_svg(layer))?;
        let Some(svg) = &mut e.pass_executor.svg_offset else {
            return Err(no_svg(layer));
        };
        let result = if cmd.reload {
            let [width, height] = svg.output_size();
            svg.reload(
                &self.gpu.device,
                &self.gpu.queue,
                cmd.path.as_deref(),
                cmd.settings,
                width,
                height,
            )
        } else {
            svg.settings = cmd.settings;
            Ok(())
        };
        match &result {
            Ok(()) => {
                e.shader_error = None;
                self.preset_store.mark_dirty();
            }
            Err(err) => {
                log::error!("SVG load: {err}");
                e.shader_error = Some(err.clone());
            }
        }
        e.pass_executor.rebuild_all_bind_groups(
            &self.gpu.device,
            &e.uniform_buffer,
            &self.placeholder,
            &self.audio_textures,
        );
        result
    }
}
