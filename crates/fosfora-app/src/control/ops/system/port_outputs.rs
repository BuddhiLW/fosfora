//! The [`OutputPort`] adapter for [`App`]: each sink only where its feature
//! and OS build it; the others answer "not built".

#[cfg(all(target_os = "linux", feature = "v4l2"))]
use super::model::PixelFormat;
use super::model::{OutputChange, OutputKind, OutputStatus, not_built};
use super::port::OutputPort;
use crate::app::App;

#[cfg(any(
    feature = "ndi",
    all(target_os = "linux", feature = "v4l2"),
    all(target_os = "windows", feature = "spout"),
    all(target_os = "macos", feature = "syphon")
))]
fn pipeline_error(state: &crate::output::pipeline::OutputState) -> Option<String> {
    match state {
        crate::output::pipeline::OutputState::Error(e) => Some(e.clone()),
        _ => None,
    }
}

impl OutputPort for App {
    fn output_status(&self, kind: OutputKind) -> OutputStatus {
        let base = OutputStatus::unavailable(kind);
        match kind {
            #[cfg(feature = "ndi")]
            OutputKind::Ndi => {
                let c = &self.ndi.config;
                OutputStatus {
                    available: true,
                    enabled: c.enabled,
                    running: self.ndi.is_running(),
                    error: pipeline_error(&self.ndi.pipeline.state),
                    name: Some(c.source_name.clone()),
                    resolution: c.resolution,
                    alpha_from_luma: Some(c.alpha_from_luma),
                    frames_sent: self.ndi.frames_sent(),
                    ..base
                }
            }
            #[cfg(all(target_os = "linux", feature = "v4l2"))]
            OutputKind::V4l2 => {
                let c = &self.v4l2.config;
                OutputStatus {
                    available: true,
                    enabled: c.enabled,
                    running: self.v4l2.is_running(),
                    error: pipeline_error(&self.v4l2.pipeline.state),
                    resolution: c.resolution,
                    device: c.device_path.clone(),
                    pixel_format: Some(match c.pixel_format {
                        crate::v4l2::types::V4l2PixelFormat::Yuyv => PixelFormat::Yuyv,
                        crate::v4l2::types::V4l2PixelFormat::Bgrx => PixelFormat::Bgrx,
                    }),
                    devices: self.v4l2.devices.iter().map(|d| d.path.clone()).collect(),
                    frames_sent: self.v4l2.frames_sent(),
                    ..base
                }
            }
            #[cfg(all(target_os = "windows", feature = "spout"))]
            OutputKind::Spout => {
                let c = &self.spout.config;
                OutputStatus {
                    available: true,
                    enabled: c.enabled,
                    running: self.spout.is_running(),
                    error: pipeline_error(&self.spout.pipeline.state),
                    name: Some(c.sender_name.clone()),
                    resolution: c.resolution,
                    frames_sent: self.spout.frames_sent(),
                    ..base
                }
            }
            #[cfg(all(target_os = "macos", feature = "syphon"))]
            OutputKind::Syphon => {
                let c = &self.syphon.config;
                OutputStatus {
                    available: true,
                    enabled: c.enabled,
                    running: self.syphon.is_running(),
                    error: pipeline_error(&self.syphon.pipeline.state),
                    name: Some(c.server_name.clone()),
                    resolution: c.resolution,
                    frames_sent: self.syphon.frames_sent(),
                    ..base
                }
            }
            #[allow(unreachable_patterns)]
            _ => base,
        }
    }

    fn set_output_enabled(&mut self, kind: OutputKind, on: bool) -> Result<(), String> {
        let built = match kind {
            #[cfg(feature = "ndi")]
            OutputKind::Ndi => {
                self.set_ndi_enabled(on);
                true
            }
            #[cfg(all(target_os = "linux", feature = "v4l2"))]
            OutputKind::V4l2 => {
                self.set_v4l2_enabled(on);
                true
            }
            #[cfg(all(target_os = "windows", feature = "spout"))]
            OutputKind::Spout => {
                self.set_spout_enabled(on);
                true
            }
            #[cfg(all(target_os = "macos", feature = "syphon"))]
            OutputKind::Syphon => {
                self.set_syphon_enabled(on);
                true
            }
            #[allow(unreachable_patterns)]
            _ => {
                let _ = on;
                false
            }
        };
        if built { Ok(()) } else { Err(not_built(kind)) }
    }

    fn configure_output(
        &mut self,
        kind: OutputKind,
        changes: &[OutputChange],
    ) -> Result<(), String> {
        let built = match kind {
            #[cfg(feature = "ndi")]
            OutputKind::Ndi => {
                self.configure_ndi(|c| {
                    for ch in changes {
                        match ch {
                            OutputChange::Name(n) => c.source_name.clone_from(n),
                            OutputChange::Resolution(r) => c.resolution = *r,
                            OutputChange::AlphaFromLuma(b) => c.alpha_from_luma = *b,
                            OutputChange::Device(_) | OutputChange::PixelFormat(_) => {}
                        }
                    }
                });
                true
            }
            #[cfg(all(target_os = "linux", feature = "v4l2"))]
            OutputKind::V4l2 => {
                self.configure_v4l2(|c| {
                    use crate::v4l2::types::V4l2PixelFormat;
                    for ch in changes {
                        match ch {
                            OutputChange::Resolution(r) => c.resolution = *r,
                            OutputChange::Device(d) => c.device_path.clone_from(d),
                            OutputChange::PixelFormat(PixelFormat::Yuyv) => {
                                c.pixel_format = V4l2PixelFormat::Yuyv;
                            }
                            OutputChange::PixelFormat(PixelFormat::Bgrx) => {
                                c.pixel_format = V4l2PixelFormat::Bgrx;
                            }
                            OutputChange::Name(_) | OutputChange::AlphaFromLuma(_) => {}
                        }
                    }
                });
                true
            }
            #[cfg(all(target_os = "windows", feature = "spout"))]
            OutputKind::Spout => {
                self.configure_spout(|c| {
                    for ch in changes {
                        match ch {
                            OutputChange::Name(n) => c.sender_name.clone_from(n),
                            OutputChange::Resolution(r) => c.resolution = *r,
                            _ => {}
                        }
                    }
                });
                true
            }
            #[cfg(all(target_os = "macos", feature = "syphon"))]
            OutputKind::Syphon => {
                self.configure_syphon(|c| {
                    for ch in changes {
                        match ch {
                            OutputChange::Name(n) => c.server_name.clone_from(n),
                            OutputChange::Resolution(r) => c.resolution = *r,
                            _ => {}
                        }
                    }
                });
                true
            }
            #[allow(unreachable_patterns)]
            _ => {
                let _ = changes;
                false
            }
        };
        if built { Ok(()) } else { Err(not_built(kind)) }
    }

    fn refresh_v4l2_devices(&mut self) -> Result<(), String> {
        #[cfg(all(target_os = "linux", feature = "v4l2"))]
        {
            self.v4l2.refresh_devices();
            Ok(())
        }
        #[cfg(not(all(target_os = "linux", feature = "v4l2")))]
        Err(not_built(OutputKind::V4l2))
    }

    fn output_window(&self) -> (Vec<String>, Option<String>) {
        (
            self.displays
                .iter()
                .map(crate::output_window::DisplayInfo::label)
                .collect(),
            self.output_window
                .as_ref()
                .map(|ow| ow.display_name.clone()),
        )
    }

    fn open_output_window(&mut self, index: usize) {
        crate::ui::panels::output_window_panel::request_open(&self.egui_overlay.context(), index);
    }

    fn close_output_window(&mut self) {
        App::close_output_window(self);
    }
}
