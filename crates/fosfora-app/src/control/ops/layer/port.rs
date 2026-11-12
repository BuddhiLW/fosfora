//! What the layer ops need from the app, split by role (ISP), and the one
//! adapter per role that reaches into [`App`] (DIP).

use crate::app::App;
use crate::effect::format::PostProcessDef;
use crate::gpu::layer::{BlendMode, Layer};
use crate::media::types::PlayDirection;
use crate::params::{ParamDef, ParamValue};
use crate::settings::RtmpStream;

// ── Port vocabulary ──────────────────────────────────────────────────────

/// What a layer shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    Effect,
    Media,
    Webcam,
}

/// One layer as the ops see it.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerView {
    pub name: String,
    pub custom_name: Option<String>,
    pub kind: LayerKind,
    /// The effect an effect layer runs.
    pub effect: Option<String>,
    /// The file (or camera) a media layer shows.
    pub media: Option<String>,
    pub blend: BlendMode,
    pub opacity: f32,
    pub displace: f32,
    pub enabled: bool,
    pub locked: bool,
    pub pinned: bool,
    pub shader_error: Option<String>,
}

impl LayerView {
    /// The name the stack panel shows: the custom one, else the default.
    pub fn label(&self) -> &str {
        self.custom_name.as_deref().unwrap_or(&self.name)
    }
}

/// A change to a layer's own fields; `None` leaves a field alone.
/// `name: Some(None)` clears a custom name.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LayerEdit {
    pub opacity: Option<f32>,
    pub blend: Option<BlendMode>,
    pub displace: Option<f32>,
    pub enabled: Option<bool>,
    pub locked: Option<bool>,
    pub pinned: Option<bool>,
    pub name: Option<Option<String>>,
}

/// One catalog effect.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectView {
    pub name: String,
    pub category: String,
    pub builtin: bool,
    pub favorite: bool,
    pub hidden: bool,
}

/// A media layer's transport.
#[derive(Debug, Clone, PartialEq)]
pub struct TransportView {
    pub playing: bool,
    pub looping: bool,
    pub speed: f32,
    pub direction: PlayDirection,
    pub position_secs: f64,
    pub duration_secs: f64,
    pub video: bool,
    pub live: bool,
}

/// A change to a media layer's transport; `None` leaves a field alone.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TransportEdit {
    /// Flip play/pause (the transport button).
    pub toggle: bool,
    pub playing: Option<bool>,
    pub looping: Option<bool>,
    pub speed: Option<f32>,
    pub direction: Option<PlayDirection>,
    pub seek_secs: Option<f64>,
}

/// A media file still decoding for a new layer, as the media panel shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaLoadView {
    pub file: String,
    /// Frames decoded, and how many the probe expects (0 = not known yet).
    pub done: u32,
    pub total: u32,
    pub secs: f32,
}

/// The cameras to pick from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WebcamList {
    /// `(device index, name)`, as the active backend lists them.
    pub devices: Vec<(u32, String)>,
    /// The default camera's device index.
    pub default: u32,
    pub ffmpeg: bool,
}

// ── Ports ────────────────────────────────────────────────────────────────

/// The layer stack: order, selection and each layer's own fields.
pub trait StackPort {
    /// Every layer, top first.
    fn layers(&self) -> Vec<LayerView>;
    fn active_layer(&self) -> usize;
    /// Add an empty layer and select it; its index.
    fn add_layer(&mut self) -> Result<usize, String>;
    /// Add an empty layer at position `at` (`0..=len`) and select it; `at`.
    fn insert_layer(&mut self, at: usize) -> Result<usize, String>;
    fn remove_layer(&mut self, index: usize) -> Result<(), String>;
    /// Back to the launch stack (Clear stack).
    fn clear_layers(&mut self);
    fn move_layer(&mut self, from: usize, to: usize);
    fn select_layer(&mut self, index: usize);
    /// Apply `edit`; a locked layer refuses blend, opacity, displace and
    /// its switch.
    fn edit_layer(&mut self, index: usize, edit: &LayerEdit) -> Result<(), String>;
}

/// The effects catalog and the effect files behind it.
pub trait EffectPort {
    /// Every effect, in catalog order (indices are positions here).
    fn effects(&self) -> Vec<EffectView>;
    /// Run effect `effect` on layer `layer`.
    fn load_effect(&mut self, layer: usize, effect: usize) -> Result<(), String>;
    fn set_favorite(&mut self, name: &str, on: bool);
    /// Write a new effect from the template and load it.
    fn create_effect(&mut self, name: &str) -> Result<(), String>;
    /// Copy effect `source` to a new user effect `name` and load it.
    fn copy_effect(&mut self, source: usize, name: &str) -> Result<(), String>;
    /// Delete a user effect's files; the deleted name.
    fn delete_effect(&mut self, index: usize) -> Result<String, String>;
    /// Open layer `layer`'s shader in the editor; the shader's path.
    fn open_shader_editor(&mut self, layer: usize) -> Result<String, String>;
    /// Write the editor's dirty tabs; the paths written.
    fn save_shader_editor(&mut self) -> Result<Vec<String>, String>;
    /// The WGSL layer `layer` runs, one source per pass; `None` when it is
    /// not an effect layer.
    fn shader_sources(&self, layer: usize) -> Option<Vec<String>>;
}

/// A layer's effect params (base values, not binding modulation).
pub trait ParamPort {
    fn param_defs(&self, layer: usize) -> Vec<ParamDef>;
    fn param_value(&self, layer: usize, name: &str) -> Option<ParamValue>;
    fn set_param(&mut self, layer: usize, name: &str, value: ParamValue);
    /// Back to the effect's defaults: one param, or all with `None`.
    fn reset_params(&mut self, layer: usize, name: Option<&str>);
}

/// Media layers, their transport, and cameras.
pub trait MediaPort {
    /// Start decoding `path` into a new media layer (it appears when ready).
    fn add_media(&mut self, path: &str) -> Result<(), String>;
    /// The files still decoding into new layers, in list order.
    fn media_loads(&self) -> Vec<MediaLoadView>;
    /// Stop decoding load `index` of [`Self::media_loads`] (the panel's Cancel).
    fn cancel_media(&mut self, index: usize) -> Result<(), String>;
    /// `None` when `layer` is not a media layer.
    fn transport(&self, layer: usize) -> Option<TransportView>;
    fn edit_transport(&mut self, layer: usize, edit: &TransportEdit) -> Result<(), String>;
    fn webcams(&mut self) -> Result<WebcamList, String>;
    /// Add a camera layer (the next unused camera for `None`); its device.
    fn add_webcam(&mut self, device: Option<&str>) -> Result<String, String>;
    /// Point a live layer at device `device`; the device's name.
    fn switch_webcam(&mut self, layer: usize, device: u32) -> Result<String, String>;
    fn set_mirror(&mut self, layer: usize, mirror: bool) -> Result<(), String>;
    /// Switch the capture backend; how many devices it lists.
    fn set_ffmpeg_webcam(&mut self, on: bool) -> Result<usize, String>;
    fn rtmp_streams(&self) -> Vec<RtmpStream>;
    fn set_rtmp_streams(&mut self, streams: Vec<RtmpStream>) -> Result<(), String>;
}

/// Master post-processing.
pub trait PostFxPort {
    fn postprocess(&self) -> PostProcessDef;
    fn has_previous_postprocess(&self) -> bool;
    /// Make layer `layer`'s effect's own post-processing Master's.
    fn adopt_postprocess(&mut self, layer: usize);
    fn restore_previous_postprocess(&mut self);
    fn replace_postprocess(&mut self, def: PostProcessDef);
}

/// Everything the layer addon needs.
pub trait LayerPorts: StackPort + EffectPort + ParamPort + MediaPort + PostFxPort {}
impl<T: StackPort + EffectPort + ParamPort + MediaPort + PostFxPort> LayerPorts for T {}

// ── App adapters ─────────────────────────────────────────────────────────

#[cfg_attr(feature = "webcam", allow(dead_code))]
const NO_WEBCAM: &str = "this build has no webcam support";

fn view_of(layer: &Layer, effects: &[crate::effect::format::PfxEffect]) -> LayerView {
    let (kind, media) = match layer.as_media() {
        Some(m) if m.is_live() => (LayerKind::Webcam, Some(m.file_name.clone())),
        Some(m) => (LayerKind::Media, Some(m.file_name.clone())),
        None => (LayerKind::Effect, None),
    };
    LayerView {
        name: layer.name.clone(),
        custom_name: layer.custom_name.clone(),
        kind,
        effect: layer
            .effect_index()
            .and_then(|i| effects.get(i))
            .map(|e| e.name.clone()),
        media,
        blend: layer.blend_mode,
        opacity: layer.opacity,
        displace: layer.displace_amount,
        enabled: layer.enabled,
        locked: layer.locked,
        pinned: layer.pinned,
        shader_error: layer.shader_error().map(str::to_owned),
    }
}

impl StackPort for App {
    fn layers(&self) -> Vec<LayerView> {
        self.layer_stack
            .layers
            .iter()
            .map(|l| view_of(l, &self.effect_loader.effects))
            .collect()
    }

    fn active_layer(&self) -> usize {
        self.layer_stack.active_layer
    }

    fn add_layer(&mut self) -> Result<usize, String> {
        self.layer_add()
    }

    fn insert_layer(&mut self, at: usize) -> Result<usize, String> {
        self.layer_insert(at)
    }

    fn remove_layer(&mut self, index: usize) -> Result<(), String> {
        self.layer_remove(index)
    }

    fn clear_layers(&mut self) {
        self.layer_clear();
    }

    fn move_layer(&mut self, from: usize, to: usize) {
        self.layer_move(from, to);
    }

    fn select_layer(&mut self, index: usize) {
        self.layer_select(index);
    }

    fn edit_layer(&mut self, index: usize, edit: &LayerEdit) -> Result<(), String> {
        self.layer_edit(index, edit)
    }
}

impl EffectPort for App {
    fn effects(&self) -> Vec<EffectView> {
        self.effect_loader
            .effects
            .iter()
            .map(|e| EffectView {
                name: e.name.clone(),
                category: e.category.clone(),
                builtin: crate::effect::EffectLoader::is_builtin(e),
                favorite: self.settings.favorite_effects.contains(&e.name),
                hidden: e.hidden,
            })
            .collect()
    }

    fn load_effect(&mut self, layer: usize, effect: usize) -> Result<(), String> {
        self.layer_load_effect(layer, effect)
    }

    fn set_favorite(&mut self, name: &str, on: bool) {
        self.effect_set_favorite(name, Some(on));
    }

    fn create_effect(&mut self, name: &str) -> Result<(), String> {
        self.create_new_effect(name).map_err(|e| e.to_string())
    }

    fn copy_effect(&mut self, source: usize, name: &str) -> Result<(), String> {
        self.effect_copy(source, name)
    }

    fn delete_effect(&mut self, index: usize) -> Result<String, String> {
        self.effect_delete(index)
    }

    fn open_shader_editor(&mut self, layer: usize) -> Result<String, String> {
        self.shader_editor_open(layer)
            .map(|p| p.display().to_string())
    }

    fn save_shader_editor(&mut self) -> Result<Vec<String>, String> {
        self.shader_editor_save()
            .map(|ps| ps.iter().map(|p| p.display().to_string()).collect())
    }

    fn shader_sources(&self, layer: usize) -> Option<Vec<String>> {
        let effect = self.layer_stack.layers.get(layer)?.as_effect()?;
        Some(effect.shader_sources.clone())
    }
}

impl ParamPort for App {
    fn param_defs(&self, layer: usize) -> Vec<ParamDef> {
        self.layer_stack
            .layers
            .get(layer)
            .map(|l| l.param_store.defs.clone())
            .unwrap_or_default()
    }

    fn param_value(&self, layer: usize, name: &str) -> Option<ParamValue> {
        self.layer_stack
            .layers
            .get(layer)?
            .param_store
            .get(name)
            .cloned()
    }

    fn set_param(&mut self, layer: usize, name: &str, value: ParamValue) {
        if let Some(l) = self.layer_stack.layers.get_mut(layer) {
            l.param_store.set(name, value);
            self.preset_store.mark_dirty();
        }
    }

    fn reset_params(&mut self, layer: usize, name: Option<&str>) {
        if let Some(l) = self.layer_stack.layers.get_mut(layer) {
            match name {
                Some(n) => l.param_store.reset(n),
                None => l.param_store.reset_all(),
            }
            l.param_store.changed = true;
            self.preset_store.mark_dirty();
        }
    }
}

impl MediaPort for App {
    fn add_media(&mut self, path: &str) -> Result<(), String> {
        self.media_add(std::path::PathBuf::from(path))
    }

    fn media_loads(&self) -> Vec<MediaLoadView> {
        self.media_loads
            .iter()
            .map(crate::ui::panels::media_panel::MediaLoading::of)
            .map(|m| MediaLoadView {
                file: m.file_name,
                done: m.done,
                total: m.total,
                secs: m.secs,
            })
            .collect()
    }

    fn cancel_media(&mut self, index: usize) -> Result<(), String> {
        let len = self.media_loads.len();
        if index >= len {
            return Err(format!("media load {index} out of range (have {len})"));
        }
        self.cancel_media_load(index);
        Ok(())
    }

    fn transport(&self, layer: usize) -> Option<TransportView> {
        let m = self.layer_stack.layers.get(layer)?.as_media()?;
        Some(TransportView {
            playing: m.transport.playing,
            looping: m.transport.looping,
            speed: m.transport.speed,
            direction: m.transport.direction,
            position_secs: m.position_secs(),
            duration_secs: m.duration_secs(),
            video: m.is_video(),
            live: m.is_live(),
        })
    }

    fn edit_transport(&mut self, layer: usize, edit: &TransportEdit) -> Result<(), String> {
        self.media_transport_edit(layer, edit)
    }

    #[cfg(feature = "webcam")]
    fn webcams(&mut self) -> Result<WebcamList, String> {
        self.refresh_webcam_devices();
        Ok(WebcamList {
            devices: self.webcam_devices.clone(),
            default: self.webcam_device_index,
            ffmpeg: self.use_ffmpeg_webcam,
        })
    }

    #[cfg(not(feature = "webcam"))]
    fn webcams(&mut self) -> Result<WebcamList, String> {
        Err(NO_WEBCAM.into())
    }

    #[cfg(feature = "webcam")]
    fn add_webcam(&mut self, device: Option<&str>) -> Result<String, String> {
        self.webcam_add_layer(device)
    }

    #[cfg(not(feature = "webcam"))]
    fn add_webcam(&mut self, _device: Option<&str>) -> Result<String, String> {
        Err(NO_WEBCAM.into())
    }

    #[cfg(feature = "webcam")]
    fn switch_webcam(&mut self, layer: usize, device: u32) -> Result<String, String> {
        self.webcam_switch(layer, device)
    }

    #[cfg(not(feature = "webcam"))]
    fn switch_webcam(&mut self, _layer: usize, _device: u32) -> Result<String, String> {
        Err(NO_WEBCAM.into())
    }

    #[cfg(feature = "webcam")]
    fn set_mirror(&mut self, layer: usize, mirror: bool) -> Result<(), String> {
        self.webcam_mirror(layer, mirror)
    }

    #[cfg(not(feature = "webcam"))]
    fn set_mirror(&mut self, _layer: usize, _mirror: bool) -> Result<(), String> {
        Err(NO_WEBCAM.into())
    }

    #[cfg(feature = "webcam")]
    fn set_ffmpeg_webcam(&mut self, on: bool) -> Result<usize, String> {
        self.webcam_set_ffmpeg(on)
    }

    #[cfg(not(feature = "webcam"))]
    fn set_ffmpeg_webcam(&mut self, _on: bool) -> Result<usize, String> {
        Err(NO_WEBCAM.into())
    }

    fn rtmp_streams(&self) -> Vec<RtmpStream> {
        self.settings.rtmp_streams.clone()
    }

    #[cfg(feature = "webcam")]
    fn set_rtmp_streams(&mut self, streams: Vec<RtmpStream>) -> Result<(), String> {
        App::set_rtmp_streams(self, streams);
        Ok(())
    }

    #[cfg(not(feature = "webcam"))]
    fn set_rtmp_streams(&mut self, _streams: Vec<RtmpStream>) -> Result<(), String> {
        Err(NO_WEBCAM.into())
    }
}

impl PostFxPort for App {
    fn postprocess(&self) -> PostProcessDef {
        self.current_postprocess()
    }

    fn has_previous_postprocess(&self) -> bool {
        self.master_postprocess_previous.is_some()
    }

    fn adopt_postprocess(&mut self, layer: usize) {
        self.postfx_adopt(layer);
    }

    fn restore_previous_postprocess(&mut self) {
        App::restore_previous_postprocess(self);
        self.preset_store.mark_dirty();
    }

    fn replace_postprocess(&mut self, def: PostProcessDef) {
        self.replace_master_postprocess(def);
        self.preset_store.mark_dirty();
    }
}
