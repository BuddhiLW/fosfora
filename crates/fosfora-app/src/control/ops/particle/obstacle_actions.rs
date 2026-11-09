//! The App actions behind the Obstacle panel; see [`super::actions`].

use std::path::{Path, PathBuf};

use super::actions::{not_built, particles, particles_mut};
use super::model::{MediaKind, ObstacleFollowUp};
use crate::app::App;
use crate::gpu::particle::{ObstacleSource, SourceRequest};
use crate::ui::panels::obstacle_panel::ObstacleCommand;

/// Follow-up work an obstacle command leaves for after the layer borrow.
enum Then {
    Nothing,
    VideoDialog,
    Webcam,
    #[cfg_attr(not(feature = "depth"), allow(dead_code))]
    Depth,
    DownloadDepth,
    Cleared,
}

impl App {
    /// Apply an Obstacle panel command to `layer`. Image and model loads come
    /// back as the dialog the window still has to open. Marks the preset dirty.
    pub fn apply_obstacle_command(
        &mut self,
        layer: usize,
        cmd: ObstacleCommand,
    ) -> Result<ObstacleFollowUp, String> {
        if matches!(cmd, ObstacleCommand::None) {
            return Ok(ObstacleFollowUp::Done);
        }
        self.preset_store.mark_dirty();
        let (device, queue) = (&self.gpu.device, &self.gpu.queue);
        let ps = particles_mut(&mut self.layer_stack, layer)?;
        let mut then = Then::Nothing;
        match cmd {
            ObstacleCommand::None => {}
            ObstacleCommand::SetEnabled(v) => ps.obstacle_enabled = v,
            ObstacleCommand::SetMode(v) => ps.obstacle_mode = v,
            ObstacleCommand::SetFit(v) => ps.obstacle_fit = v,
            ObstacleCommand::SetThreshold(v) => ps.obstacle_threshold = v,
            ObstacleCommand::SetElasticity(v) => ps.obstacle_elasticity = v,
            ObstacleCommand::LoadImage => return Ok(ObstacleFollowUp::ImageDialog),
            ObstacleCommand::LoadModel => return Ok(ObstacleFollowUp::ModelDialog),
            ObstacleCommand::LoadVideo => then = Then::VideoDialog,
            ObstacleCommand::UseWebcam => {
                if !cfg!(feature = "webcam") {
                    return Err(not_built("webcam"));
                }
                ps.obstacle_enabled = true;
                ps.obstacle_source = Some(ObstacleSource::Webcam);
                ps.obstacle_image_path = None;
                then = Then::Webcam;
            }
            ObstacleCommand::UseDepth => {
                #[cfg(not(feature = "depth"))]
                return Err(not_built("depth"));
                #[cfg(feature = "depth")]
                {
                    if !crate::depth::model::ort_available() {
                        return Err("ONNX Runtime is not available for depth estimation".into());
                    }
                    ps.obstacle_enabled = true;
                    ps.obstacle_source = Some(ObstacleSource::Depth);
                    ps.obstacle_image_path = None;
                    then = Then::Depth;
                }
            }
            ObstacleCommand::DownloadDepthModel => then = Then::DownloadDepth,
            ObstacleCommand::Clear => {
                ps.clear_obstacle(device, queue);
                then = Then::Cleared;
            }
            ObstacleCommand::SetWaterEnabled(v) => ps.obstacle_water_enabled = v,
            ObstacleCommand::SetWaterLevel(v) => ps.obstacle_water_params.level_scale = v,
            ObstacleCommand::SetWaterSource(v) => ps.obstacle_water_params.source_rate = v,
            ObstacleCommand::SetWaterDrain(v) => ps.obstacle_water_params.drain = v,
            ObstacleCommand::SetWaterFlux(v) => ps.obstacle_water_params.flux_gain = v,
            ObstacleCommand::SetModelSpin(v) => ps.obstacle_model_spin = v,
            ObstacleCommand::SetModelDisplay(v) => ps.obstacle_model_display = v,
            ObstacleCommand::SetFluidEnabled(v) => ps.obstacle_fluid_enabled = v,
            ObstacleCommand::SetFluidSpeed(v) => ps.obstacle_fluid_params.flow_speed = v,
            ObstacleCommand::SetFluidCoupling(v) => ps.obstacle_fluid_coupling = v,
            ObstacleCommand::SetFluidVorticity(v) => ps.obstacle_fluid_params.vorticity = v,
            ObstacleCommand::SetFluidViscosity(v) => ps.obstacle_fluid_params.viscosity = v,
            ObstacleCommand::SetFluidGrid(v) => ps.obstacle_fluid_grid = v,
        }
        self.finish_obstacle_command(layer, then)?;
        Ok(ObstacleFollowUp::Done)
    }

    /// The part of an obstacle command that needs the whole app.
    fn finish_obstacle_command(&mut self, layer: usize, then: Then) -> Result<(), String> {
        match then {
            Then::Nothing => Ok(()),
            Then::VideoDialog => self.load_obstacle_video(layer, None),
            Then::Webcam => {
                #[cfg(feature = "webcam")]
                self.ensure_default_webcam()
                    .map_err(|e| format!("Webcam failed: {e}"))?;
                Ok(())
            }
            Then::Depth => {
                #[cfg(feature = "depth")]
                {
                    let camera = self
                        .ensure_default_webcam()
                        .map_err(|e| format!("Webcam failed: {e}"));
                    if self.depth_thread.is_none() {
                        let model_path = crate::depth::model::model_path();
                        let thread = crate::depth::thread::DepthThread::start(model_path)
                            .map_err(|e| format!("Failed to start depth thread: {e}"))?;
                        self.depth_thread = Some(thread);
                        log::info!("Depth estimation thread started");
                    }
                    camera?;
                }
                Ok(())
            }
            Then::DownloadDepth => {
                #[cfg(feature = "depth")]
                {
                    if self
                        .depth_download
                        .as_ref()
                        .is_none_or(|p| !p.is_downloading())
                    {
                        self.depth_download = Some(crate::depth::model::download_model());
                        log::info!("Starting depth model download");
                    }
                    Ok(())
                }
                #[cfg(not(feature = "depth"))]
                {
                    Err(not_built("depth"))
                }
            }
            Then::Cleared => {
                #[cfg(feature = "depth")]
                {
                    self.depth_thread = None;
                }
                #[cfg(feature = "webcam")]
                self.cleanup_webcam_if_unused();
                Ok(())
            }
        }
    }

    /// Load the obstacle file at `path` onto `layer`: a 3D model for
    /// `.glb/.gltf/.ply/.splat`, else an image. Stops any camera or depth
    /// obstacle feed. Marks the preset dirty.
    pub fn load_obstacle_file(&mut self, layer: usize, path: &Path) -> Result<(), String> {
        let is_model = super::obstacle_file_kind(path) == MediaKind::Model;
        let (device, queue) = (&self.gpu.device, &self.gpu.queue);
        let ps = particles_mut(&mut self.layer_stack, layer)?;
        let loaded = if is_model {
            ps.set_obstacle_model(device, path)
                .map(|()| log::info!("Loaded obstacle model: {}", path.display()))
                .map_err(|e| format!("Model load failed: {e}"))
        } else {
            image::open(path)
                .map(|img| {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    let path = path.to_string_lossy().into_owned();
                    ps.set_obstacle_image(device, queue, &rgba, w, h, Some(path));
                })
                .map_err(|e| format!("Failed to load obstacle image: {e}"))
        };
        #[cfg(feature = "depth")]
        {
            self.depth_thread = None;
        }
        #[cfg(feature = "webcam")]
        self.cleanup_webcam_if_unused();
        self.preset_store.mark_dirty();
        loaded
    }

    /// Load an obstacle video onto `layer`: the file at `path`, or the native
    /// dialog for `None`. Decoding runs on the source loader's thread.
    pub fn load_obstacle_video(&mut self, layer: usize, path: Option<PathBuf>) -> Result<(), String> {
        particles(&self.layer_stack, layer)?;
        self.preset_store.mark_dirty();
        let request = SourceRequest {
            layer_idx: layer,
            as_obstacle: true,
            ..Default::default()
        };
        self.start_source_load(MediaKind::Video, path, request)
    }

    /// Feed the obstacle from listed camera `index` and make it the default.
    /// The previous camera keeps running until the new one does.
    pub fn switch_obstacle_webcam(&mut self, index: u32) -> Result<(), String> {
        #[cfg(feature = "webcam")]
        {
            let started = self
                .webcam_device_name(index)
                .ok_or_else(|| "Camera is no longer connected".to_string())
                .and_then(|name| self.ensure_webcam(&name));
            match started {
                Ok(_) => {
                    self.set_default_webcam(index);
                    self.cleanup_webcam_if_unused();
                    Ok(())
                }
                Err(e) => {
                    self.refresh_webcam_devices();
                    Err(format!("Camera failed: {e}"))
                }
            }
        }
        #[cfg(not(feature = "webcam"))]
        {
            let _ = index;
            Err(not_built("webcam"))
        }
    }
}
