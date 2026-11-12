//! The App action behind the Morph panel; see [`super::actions`].

use super::actions::particles_mut;
use super::model::MorphEdit;
use crate::app::App;
use crate::gpu::particle::types::{MorphTargetDef, ParticleAux};
use crate::gpu::particle::{ParticleSystem, SourceRequest};

/// Load `data` into morph slot `slot` and record what it holds.
fn load_morph_slot(ps: &mut ParticleSystem, slot: u32, data: Vec<ParticleAux>, label: String) {
    if let Some(morph) = ps.morph_state.as_mut() {
        morph.load_target(slot, data);
    }
    if let Some(targets) = ps.def.morph_targets.as_mut() {
        let slot = slot as usize;
        while targets.len() <= slot {
            targets.push(MorphTargetDef {
                source: String::new(),
                color: None,
            });
        }
        targets[slot] = MorphTargetDef {
            source: label,
            color: None,
        };
    }
}

fn defined_morph_slots(ps: &ParticleSystem) -> u32 {
    ps.def
        .morph_targets
        .as_ref()
        .map_or(0, |t| u32::try_from(t.len()).unwrap_or(u32::MAX))
}

impl App {
    /// Apply one frame's morph edits to `layer`. `selected` is the slot the
    /// panel has selected. Returns whether that selection was used up (the
    /// panel then clears it). Fails when the effect has no morph targets.
    pub fn apply_morph(
        &mut self,
        layer: usize,
        edit: &MorphEdit,
        selected: Option<u32>,
    ) -> Result<bool, String> {
        use crate::gpu::particle::morph::{AutoCycle, generate_geometry};
        let (device, queue) = (&self.gpu.device, &self.gpu.queue);
        let ps = particles_mut(&mut self.layer_stack, layer)?;
        let Some(morph) = ps.morph_state.as_mut() else {
            return Err(format!("layer {layer}'s effect has no morph targets"));
        };
        if let Some(target) = edit.trigger {
            morph.trigger_morph(target);
        }
        if let Some(mode) = edit.auto_cycle {
            morph.auto_cycle = match mode {
                0 => AutoCycle::Off,
                1 => AutoCycle::OnBeat,
                _ => AutoCycle::Timed(4.0),
            };
        }
        if let Some(hold) = edit.hold {
            morph.hold_duration = hold;
        }
        if let Some(style) = edit.style {
            morph.transition_style = style;
        }
        if let Some(src) = edit.set_source {
            morph.source_index = src.min(morph.target_count.saturating_sub(1));
        }
        if let Some(progress) = edit.blend {
            morph.progress = progress;
            morph.transitioning = progress < 1.0;
            morph.hold_timer = 0.0;
        }

        let pick = selected.unwrap_or_else(|| defined_morph_slots(ps)).min(3);
        let mut upload = false;
        if let Some(clear) = edit.clear_slot {
            let slot = clear.min(3);
            if let Some(m) = ps.morph_state.as_mut() {
                m.remove_target(slot);
            }
            if let Some(targets) = ps.def.morph_targets.as_mut() {
                if (slot as usize) < targets.len() {
                    targets.remove(slot as usize);
                }
            }
            upload = true;
        }
        if let Some(shape) = &edit.add_geometry {
            let data = generate_geometry(shape, ps.max_particles, ps.def.initial_size);
            load_morph_slot(ps, pick, data, format!("geometry:{shape}"));
            upload = true;
        }
        if let Some(text) = &edit.add_text {
            let data = crate::gpu::particle::text_source::render_text_to_particles(
                text,
                ps.max_particles,
                ps.def.initial_size,
            );
            if !data.is_empty() {
                load_morph_slot(ps, pick, data, format!("text:{text}"));
                upload = true;
            }
        }
        let mut consumed = false;
        if upload {
            ps.upload_morph_targets(device, queue);
            consumed = true;
        }
        if edit.snapshot {
            let slot = selected.unwrap_or_else(|| defined_morph_slots(ps)).min(3);
            let data = ps.snapshot_particles(device, queue);
            if !data.is_empty() {
                load_morph_slot(ps, slot, data, "snapshot".to_string());
                ps.upload_morph_targets(device, queue);
            }
            consumed = true;
        }
        if let Some((kind, path)) = &edit.load {
            let request = SourceRequest {
                layer_idx: layer,
                as_morph_target: true,
                morph_slot: selected,
                ..Default::default()
            };
            self.start_source_load(*kind, path.clone(), request)?;
            consumed = true;
        }
        Ok(consumed)
    }
}
