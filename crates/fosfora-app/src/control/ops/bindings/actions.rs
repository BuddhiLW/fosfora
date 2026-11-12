//! Audio-input and binding actions shared by the UI signal handlers in
//! `main.rs` and the binding ops, so both take one code path.

use crate::app::App;
use crate::audio::StructureConfig;
use crate::bindings::templates::builtin_templates;
use crate::bindings::types::BindingScope;
use crate::effect::EffectLoader;
use crate::gpu::layer::LayerStack;
use crate::settings::BandScale;
use crate::ui::panels::binding_helpers::LayerParamInfo;

/// Each layer's effect name and bindable (Float and Bool) param names, as the
/// Binding Matrix and the templates read them.
pub fn layer_param_infos(stack: &LayerStack, effects: &EffectLoader) -> Vec<LayerParamInfo> {
    stack
        .layers
        .iter()
        .enumerate()
        .map(|(index, l)| LayerParamInfo {
            index,
            effect_name: l
                .effect_index()
                .and_then(|idx| effects.effects.get(idx))
                .map(|eff| eff.name.clone())
                .unwrap_or_default(),
            param_names: l
                .param_store
                .defs
                .iter()
                .filter(|d| {
                    matches!(
                        d,
                        crate::params::ParamDef::Float { .. }
                            | crate::params::ParamDef::Bool { .. }
                    )
                })
                .map(|d| d.name().to_string())
                .collect(),
        })
        .collect()
}

impl App {
    /// Change the band scaling: saved to Settings, and the audio pipeline
    /// rebuilt so the analyzer uses it.
    pub fn apply_band_scale(&mut self, scale: BandScale) {
        self.settings.band_scale = scale;
        self.settings.save();
        self.audio.set_band_scale(scale);
    }

    /// Set the input trim (clamped by the audio system) and save it.
    pub fn apply_input_trim(&mut self, db: f32) {
        self.audio.set_input_trim_db(db);
        self.settings.input_trim_db = self.audio.input_trim_db();
        self.settings.save();
    }

    /// Set the A/V sync offset (clamped by the beat tracker) and save it.
    pub fn apply_sync_offset(&mut self, ms: f32) {
        let ms = {
            let mut tempo = self.audio.tempo().lock().unwrap_or_else(|e| e.into_inner());
            tempo.set_sync_offset_ms(ms);
            tempo.sync_offset_ms()
        };
        self.settings.sync_offset_ms = ms;
        self.settings.save();
    }

    /// Save the live structure-detection tuning to Settings.
    pub fn persist_structure_tuning(&mut self) {
        self.settings.structure_tuning = *self
            .audio
            .tuning()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        self.settings.save();
    }

    /// Replace the live structure-detection tuning and save it.
    pub fn apply_structure_tuning(&mut self, tuning: StructureConfig) {
        *self
            .audio
            .tuning()
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = tuning;
        self.persist_structure_tuning();
    }

    /// Listen to `device` (`None`: the default input) and save the choice.
    pub fn apply_audio_device(&mut self, device: Option<String>) {
        self.audio.switch_device(device.as_deref());
        self.settings.audio_device = device;
        self.settings.save();
    }

    /// Apply built-in binding template `template` to `layer`, as the matrix's
    /// Templates menu does; the ids of the bindings it created.
    pub fn apply_binding_template(
        &mut self,
        template: usize,
        layer: usize,
    ) -> Result<Vec<String>, String> {
        let tmpl = builtin_templates()
            .get(template)
            .ok_or_else(|| format!("template index {template} out of range"))?;
        let info = layer_param_infos(&self.layer_stack, &self.effect_loader)
            .into_iter()
            .nth(layer)
            .ok_or_else(|| format!("layer index {layer} out of range"))?;
        if info.effect_name.is_empty() {
            return Err(format!(
                "layer {layer} runs no effect, so a template has nothing to bind"
            ));
        }
        let before: Vec<String> = self
            .binding_bus
            .bindings
            .iter()
            .map(|b| b.id.clone())
            .collect();
        self.binding_bus
            .apply_template(tmpl, layer, &info.effect_name, &info.param_names);
        Ok(self
            .binding_bus
            .bindings
            .iter()
            .filter(|b| !before.contains(&b.id))
            .map(|b| b.id.clone())
            .collect())
    }

    /// Write one scope's bindings now: the global file, or the loaded user
    /// preset's sidecar. What was saved: `"global"` or the preset name.
    pub fn try_save_bindings(&mut self, scope: BindingScope) -> Result<String, String> {
        match scope {
            BindingScope::Global => {
                self.binding_bus.save_global();
                Ok("global".into())
            }
            BindingScope::Preset => {
                let current = self
                    .preset_store
                    .current_preset
                    .ok_or("no preset is loaded; save one first (preset/save)")?;
                if current < self.preset_store.builtin_count {
                    return Err(
                        "the loaded preset is built-in; save it as a user preset first".into(),
                    );
                }
                let name = self
                    .preset_store
                    .current_name()
                    .ok_or("no preset is loaded")?
                    .to_owned();
                self.binding_bus.save_preset_bindings(&name);
                Ok(name)
            }
        }
    }
}
