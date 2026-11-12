//! Contextual SVG source/geometry controls; performance knobs live in .pfx inputs.
use crate::gpu::svg_offset::SvgOffsetSettings;

#[derive(Clone)]
pub struct SvgOffsetInfo {
    pub path: Option<String>,
    pub shapes: usize,
    pub settings: SvgOffsetSettings,
}
#[derive(Clone, Default)]
pub struct SvgOffsetCommand {
    pub path: Option<String>,
    pub settings: SvgOffsetSettings,
    pub reload: bool,
}

pub fn draw_svg_offset_panel(ui: &mut egui::Ui, info: &SvgOffsetInfo) {
    let mut settings = info.settings.clone();
    let mut path = info.path.clone();
    let mut changed = false;
    let mut reload = false;
    ui.horizontal(|ui| {
        #[cfg(feature = "desktop")]
        if ui.button("Load SVG…").clicked() {
            if let Some(file) = rfd::FileDialog::new()
                .add_filter("SVG", &["svg"])
                .pick_file()
            {
                path = Some(file.to_string_lossy().into_owned());
                reload = true;
            }
        }
        if ui.button("Default ornament").clicked() {
            path = None;
            reload = true;
        }
        if ui.button("Reload").clicked() {
            reload = true;
        }
    });
    ui.label(format!(
        "{} shapes · {}",
        info.shapes,
        info.path.as_deref().unwrap_or("Built-in ornament")
    ));
    ui.label("Amount, motion, palette and order are in Parameters — bindable.");
    ui.small("Vector fills/strokes and transforms; filters, clip paths and raster images are not reproduced. Original palette uses one color per shape.");
    ui.horizontal(|ui| {
        ui.label("Quality");
        for (value, name) in [(1, "Full"), (2, "Half"), (4, "Quarter")] {
            if ui
                .selectable_value(&mut settings.quality, value, name)
                .changed()
            {
                reload = true;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Background");
        for (value, name) in [(0, "Alpha"), (1, "Black"), (2, "White")] {
            changed |= ui
                .selectable_value(&mut settings.background, value, name)
                .changed();
        }
    });
    changed |= ui
        .add(egui::Slider::new(&mut settings.offset[0], -1.0..=1.0).text("Offset X"))
        .changed();
    changed |= ui
        .add(egui::Slider::new(&mut settings.offset[1], -1.0..=1.0).text("Offset Y"))
        .changed();
    changed |= ui
        .add(egui::Slider::new(&mut settings.rotation, -180.0..=180.0).text("Rotate"))
        .changed();
    ui.collapsing("Keyer", |ui| {
        changed |= ui
            .add(egui::Slider::new(&mut settings.key_width, 0.0..=1.0).text("Width"))
            .changed();
        changed |= ui
            .add(egui::Slider::new(&mut settings.key_phase, 0.0..=1.0).text("Phase"))
            .changed();
        changed |= ui
            .add(egui::Slider::new(&mut settings.key_soft, 0.001..=0.5).text("Softness"))
            .changed();
        changed |= ui
            .add(egui::Slider::new(&mut settings.key_repeat, 1.0..=16.0).text("Repeat"))
            .changed();
    });
    if reload || changed {
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                egui::Id::new("svg_offset_cmd"),
                SvgOffsetCommand {
                    path,
                    settings,
                    reload,
                },
            );
        });
    }
}
