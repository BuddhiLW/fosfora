//! Particle ops: the Particles, Lattice, Helix, Obstacle and Volumetric
//! panels, morph targets, splat scenes and particle quality. Every op works
//! on the active layer, or the one `{:layer n}` names.
//!
//! Promote (`promote`) -> Pipeline (`pipeline`, pure) -> Boundary
//! (`look_ops`, `source_ops`, `shape_ops`: one port call per op). The ports
//! (`port`) are split by panel; the App work behind them is in the
//! `*_actions.rs` files, which the panels' signal handlers call too.

mod actions;
mod look_ops;
pub mod model;
mod morph_actions;
mod obstacle_actions;
pub mod pipeline;
pub mod port;
pub mod promote;
mod shape_ops;
mod source_ops;
#[cfg(test)]
mod tests;

use serde_json::{Value, json};

pub use self::model::{MediaKind, MorphEdit, ObstacleFollowUp, SimEdit, SourceLoad, VideoEdit};
pub use self::pipeline::obstacle_file_kind;
pub use self::port::{MorphPort, ObstaclePort, ParticleLayers, ParticleSimPort, ParticleSourcePort};
use self::{look_ops as look, shape_ops as shape, source_ops as src};
use crate::control::{CljrsNs, ControlAddon, OpResult, OpSpec};

pub struct Addon;

/// Everything the particle ops need from their context.
pub trait ParticlePorts: ParticleSimPort + ParticleSourcePort + ObstaclePort + MorphPort {}
impl<T: ParticleSimPort + ParticleSourcePort + ObstaclePort + MorphPort> ParticlePorts for T {}

const L: &str = ":layer int?";

impl<C: ParticlePorts + 'static> ControlAddon<C> for Addon {
    fn id(&self) -> &'static str {
        "particle"
    }

    fn ops(&self) -> Vec<OpSpec<C>> {
        vec![
            OpSpec::new("particle/state", "{:layer int?}", "Sim, source, obstacle, morph, lattice?/helix? of a layer.", state::<C>),
            OpSpec::new("particle/sim", "{:layer? :emit-rate :burst :lifetime :speed :size :drag :trail-length}", "Read, or change any of, the emission params.", look::sim::<C>),
            OpSpec::new("particle/quality", "{:value low|medium|high|ultra|max?}", "Read, or set, the particle quality (reloads the active effect).", look::quality::<C>),
            OpSpec::new("particle/volumetric", "{:enabled bool? :params map?}", "Read, or change, the volumetric look.", look::volumetric::<C>),
            OpSpec::new("particle/lattice", "{:layer? :params map? :reseed bool?}", "Read, or patch, a lattice effect's params.", look::lattice::<C>),
            OpSpec::new("particle/helix", "{:layer? :params map?}", "Read, or patch, a helix effect's params.", look::helix::<C>),
            OpSpec::new("particle/source", L, "The layer's particle source: kind, path, video transport, model pose.", src::state::<C>),
            OpSpec::new("particle/builtins", "{}", "The built-in source pictures.", src::builtins::<C>),
            OpSpec::new("particle/load-builtin", "{:index int | :name str, :layer?}", "Load a built-in picture as the source.", src::load_builtin::<C>),
            OpSpec::new("particle/load-image", "{:path str?, :layer?}", "Load an image as the source (no :path opens the dialog).", src::load_image::<C>),
            OpSpec::new("particle/load-video", "{:path str?, :layer?}", "Load a video as the source (no :path opens the dialog).", src::load_video::<C>),
            OpSpec::new("particle/load-model", "{:path str?, :layer?}", "Load a 3D model as the source (no :path opens the dialog).", src::load_model::<C>),
            OpSpec::new("particle/model-pose", "{:layer? :yaw :pitch :scale :ambient :light-mix :light-x :light-y :light-z :rays}", "Read, or change, a model source's pose and light.", src::model_pose::<C>),
            OpSpec::new("particle/webcams", "{}", "The connected cameras.", src::webcams::<C>),
            OpSpec::new("particle/webcam", "{:device str?, :layer?}", "A camera as the source (the default one without :device).", src::webcam::<C>),
            OpSpec::new("particle/video", "{:layer? :playing :looping :speed :seek}", "Read, or change, a video source's transport.", src::video::<C>),
            OpSpec::new("particle/splat", L, "The layer's splat scene state.", src::splat::<C>),
            OpSpec::new("particle/splat-load", "{:path str?, :layer?}", "Load a splat scene (no :path opens the dialog).", src::splat_load::<C>),
            OpSpec::new("particle/splat-demo", "{}", "Download the demo splat scene onto the active splat layer.", src::splat_demo::<C>),
            OpSpec::new("particle/obstacle", "{:layer? :cmd str? :value any? :path str?}", "The obstacle; with :cmd (mode, threshold, clear, load-image...) change it first.", shape::obstacle::<C>),
            OpSpec::new("particle/obstacle-load", "{:path str, :kind image|video|model?, :layer?}", "Load an obstacle file (kind from the extension by default).", shape::obstacle_load::<C>),
            OpSpec::new("particle/obstacle-webcam", "{:index int | :name str}", "Feed camera obstacles from another camera.", shape::obstacle_webcam::<C>),
            OpSpec::new("particle/morph", L, "The morph state: slots, targets, progress, style, cycle, hold.", shape::morph::<C>),
            OpSpec::new("particle/morph-trigger", "{:target int, :layer?}", "Morph to a loaded target.", shape::morph_trigger::<C>),
            OpSpec::new("particle/morph-source", "{:target int, :layer?}", "Set the target morphs start from.", shape::morph_source::<C>),
            OpSpec::new("particle/morph-cycle", "{:mode off|beat|timed, :layer?}", "Set morph auto-cycling.", shape::morph_cycle::<C>),
            OpSpec::new("particle/morph-style", "{:style spring|explode|flow|cascade|direct, :layer?}", "Set the morph transition style.", shape::morph_style::<C>),
            OpSpec::new("particle/morph-hold", "{:secs num, :layer?}", "Set how long a shape holds before the next auto morph.", shape::morph_hold::<C>),
            OpSpec::new("particle/morph-blend", "{:progress 0..1, :layer?}", "Scrub the current morph by hand.", shape::morph_blend::<C>),
            OpSpec::new("particle/morph-clear", "{:slot 0..3, :layer?}", "Empty a morph slot (higher slots shift down).", shape::morph_clear::<C>),
            OpSpec::new("particle/morph-geometry", "{:shape circle|ring|grid|spiral|heart|star, :slot?}", "A generated shape as a morph target.", shape::morph_geometry::<C>),
            OpSpec::new("particle/morph-text", "{:text str, :slot?}", "Rendered text as a morph target.", shape::morph_text::<C>),
            OpSpec::new("particle/morph-snapshot", "{:slot?}", "The particles as they are now, as a morph target.", shape::morph_snapshot::<C>),
            OpSpec::new("particle/morph-load", "{:kind image|video|model, :path str?, :slot?}", "A file as morph target(s) (no :path opens the dialog).", shape::morph_load::<C>),
        ]
    }

    fn ui_signals(&self) -> &'static [(&'static str, &'static str)] {
        &[
            ("particle_emit_rate", "particle/sim"),
            ("particle_burst", "particle/sim"),
            ("particle_lifetime", "particle/sim"),
            ("particle_speed", "particle/sim"),
            ("particle_size", "particle/sim"),
            ("particle_drag", "particle/sim"),
            ("particle_trail_length", "particle/sim"),
            ("set_particle_quality", "particle/quality"),
            ("lattice_cmd", "particle/lattice"),
            ("helix_cmd", "particle/helix"),
            ("particle_select_builtin", "particle/load-builtin"),
            ("particle_load_image", "particle/load-image"),
            ("particle_load_video", "particle/load-video"),
            ("particle_load_model", "particle/load-model"),
            ("particle_model_pose", "particle/model-pose"),
            ("particle_webcam", "particle/webcam"),
            ("particle_webcam_device", "particle/webcam"),
            ("particle_video_playing", "particle/video"),
            ("particle_video_looping", "particle/video"),
            ("particle_video_speed", "particle/video"),
            ("particle_video_seek", "particle/video"),
            ("splat_load_scene", "particle/splat-load"),
            ("splat_download_demo", "particle/splat-demo"),
            ("obstacle_cmd", "particle/obstacle"),
            ("switch_obstacle_webcam_device", "particle/obstacle-webcam"),
            ("morph_trigger_target", "particle/morph-trigger"),
            ("morph_set_source", "particle/morph-source"),
            ("morph_auto_cycle", "particle/morph-cycle"),
            ("morph_style", "particle/morph-style"),
            ("morph_hold_duration", "particle/morph-hold"),
            ("morph_manual_blend", "particle/morph-blend"),
            ("morph_clear_slot", "particle/morph-clear"),
            ("morph_add_geometry", "particle/morph-geometry"),
            ("morph_add_text", "particle/morph-text"),
            ("morph_snapshot", "particle/morph-snapshot"),
            ("morph_load_image", "particle/morph-load"),
            ("morph_load_video", "particle/morph-load"),
            ("morph_load_model", "particle/morph-load"),
        ]
    }

    fn cljrs(&self) -> Option<CljrsNs> {
        Some(CljrsNs {
            ns: "fosfora.particle",
            alias: "particle",
            src: include_str!("particle.cljrs"),
        })
    }
}

/// `particle/state`: one layer's particle system at a glance.
fn state<P: ParticlePorts>(port: &mut P, a: &Value) -> OpResult {
    let layer = promote::layer(port, a)?;
    let sim = port
        .sim_state(layer)
        .ok_or_else(|| format!("layer {layer} has no particle system"))?;
    let source = port.source_state(layer).unwrap_or_default();
    Ok(json!({
        "layer": layer,
        "sim": pipeline::sim_json(layer, &sim),
        "source": pipeline::source_json(layer, &source),
        "obstacle": port.obstacle_state(layer).map(|s| pipeline::obstacle_json(layer, &s)),
        "morph": port.morph_state(layer).map(|m| pipeline::morph_json(layer, &m)),
        "lattice": port.lattice(layer).is_some(),
        "helix": port.helix(layer).is_some(),
        "splat": port.splat_state(layer).is_some_and(|s| s.is_splat),
    }))
}
