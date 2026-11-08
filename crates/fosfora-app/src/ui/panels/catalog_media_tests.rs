//! The Media tab through real pointer events: a click and a drop send the
//! add intent, an unrendered clip refuses both, and families are headed in
//! catalog order.

use std::path::{Path, PathBuf};

use super::*;
use crate::effect::loader::EffectLoader;
use crate::gpu::layer::{BlendMode, LayerInfo};
use crate::media::pack::parse;
use crate::ui::panels::catalog_panel::{Target, draw_catalog};
use crate::ui::panels::stack_panel;

const FIXTURE: &str = include_str!("../../../tests/fixtures/media-pack/media.json");

fn packs() -> MediaPacks {
    let mut p = MediaPacks::empty();
    p.items = parse(FIXTURE, Path::new("/nonexistent/pack")).unwrap();
    p
}

fn layer(name: &str) -> LayerInfo {
    LayerInfo {
        name: name.into(),
        custom_name: None,
        live_opacity: None,
        live_enabled: None,
        effect_index: Some(0),
        effect_name: Some(name.into()),
        blend_mode: BlendMode::default(),
        opacity: 1.0,
        displace_amount: 0.0,
        enabled: true,
        locked: false,
        pinned: false,
        has_particles: false,
        shader_error: None,
        is_media: false,
        media_file_name: None,
        media_is_animated: false,
        media_is_video: false,
        media_is_live: false,
        chain: None,
        needs_layer_below: false,
    }
}

struct Harness {
    ctx: egui::Context,
    loader: EffectLoader,
    packs: MediaPacks,
    layers: Vec<LayerInfo>,
    thumbs: CatalogThumbs,
    time: f64,
}

impl Harness {
    fn new() -> Self {
        let ctx = egui::Context::default();
        crate::ui::theme::colors::set_theme_colors(
            &ctx,
            crate::ui::theme::palette::Palette::GRAY.colors(),
        );
        // Open on the Media tab.
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("v2_catalog_tab"), "media-packs".to_string()));
        Self {
            ctx,
            loader: EffectLoader::for_test(""),
            packs: packs(),
            layers: vec![layer("Aurora"), layer("Drift")],
            thumbs: CatalogThumbs::new(PathBuf::from("/nonexistent")),
            time: 0.0,
        }
    }

    fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
        self.time += 1.0 / 60.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 1400.0),
            )),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        let (loader, packs, layers, thumbs) =
            (&self.loader, &self.packs, &self.layers, &mut self.thumbs);
        self.ctx.run(input, |ctx| {
            egui::SidePanel::left("stack")
                .exact_width(600.0)
                .show(ctx, |ui| {
                    let pics = stack_panel::StackPictures {
                        thumbs: None,
                        aspect: 16.0 / 9.0,
                    };
                    stack_panel::draw_stack(ui, layers, 0, None, &[], &pics);
                });
            egui::CentralPanel::default().show(ctx, |ui| {
                let target = Target {
                    current: None,
                    locked: false,
                    can_add: true,
                    active: 0,
                };
                draw_catalog(ui, loader, packs, &[], thumbs, &target);
            });
        })
    }

    fn key(&self, id: &str) -> String {
        self.packs
            .items
            .iter()
            .find(|i| i.id == id)
            .expect("in the fixture")
            .key()
    }

    fn tile_rect(&self, id: &str) -> egui::Rect {
        self.ctx
            .read_response(tile_id(&self.key(id)))
            .expect("tile drawn")
            .rect
    }

    fn row_rect(&self, i: usize) -> egui::Rect {
        self.ctx
            .data(|d| d.get_temp(egui::Id::new(format!("v2_row_{i}")).with("rect")))
            .expect("row drawn")
    }

    fn press(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }

    fn click(&mut self, id: &str) -> Option<(String, usize)> {
        self.frame(vec![]);
        let at = self.tile_rect(id).center();
        self.frame(vec![egui::Event::PointerMoved(at), Self::press(at, true)]);
        self.frame(vec![Self::press(at, false)]);
        take_add(&self.ctx)
    }

    fn drag(&mut self, id: &str, to: egui::Pos2) -> Option<(String, usize)> {
        self.frame(vec![]);
        let from = self.tile_rect(id).center();
        self.frame(vec![
            egui::Event::PointerMoved(from),
            Self::press(from, true),
        ]);
        for k in 1..=8 {
            let p = from + (to - from) * (k as f32 / 8.0);
            self.frame(vec![egui::Event::PointerMoved(p)]);
        }
        self.frame(vec![egui::Event::PointerMoved(to)]);
        self.frame(vec![Self::press(to, false)]);
        take_add(&self.ctx)
    }
}

#[test]
fn clicking_a_clip_adds_it_above_the_selected_layer() {
    let mut h = Harness::new();
    let key = h.key("graphene-fermi");
    assert_eq!(h.click("graphene-fermi"), Some((key, 0)));
}

#[test]
fn an_unrendered_clip_does_nothing() {
    let mut h = Harness::new();
    assert_eq!(h.click("lissajous-3d"), None);
    let row1 = h.row_rect(1);
    assert_eq!(h.drag("girih-gold", row1.center()), None);
}

#[test]
fn dropping_a_clip_on_the_stack_inserts_a_layer_there() {
    let mut h = Harness::new();
    h.frame(vec![]);
    let row1 = h.row_rect(1);
    let key = h.key("fold-9");
    assert_eq!(
        h.drag("fold-9", row1.center()),
        Some((key.clone(), 1)),
        "the middle of a row puts the clip above it: a clip replaces no effect"
    );
    assert_eq!(
        h.drag("fold-9", egui::pos2(row1.center().x, row1.bottom() - 3.0)),
        Some((key, 2)),
        "the bottom edge puts it below"
    );
}

#[test]
fn families_are_headed_in_catalog_order() {
    let mut h = Harness::new();
    let out = h.frame(vec![]);
    let headings: Vec<String> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) => Some(t.galley.job.text.clone()),
            _ => None,
        })
        .filter(|t| ["Orbits", "Curves", "Lattices", "Tilings"].contains(&t.as_str()))
        .collect();
    assert_eq!(headings, ["Orbits", "Curves", "Lattices", "Tilings"]);
}

#[test]
fn search_reaches_tags_and_descriptions() {
    let p = packs();
    assert_eq!(count(&p, ""), 5);
    assert_eq!(count(&p, "dirac"), 1, "a tag");
    assert_eq!(count(&p, "three-body atlas"), 1, "a description");
    assert_eq!(count(&p, "tilings"), 2, "a family");
}
