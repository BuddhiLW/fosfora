//! The catalog's Media tab: the clips of every media pack
//! (`FOSFORA_MEDIA_DIRS`, see [`crate::media::pack`]) as pictures, grouped
//! under family headings.
//!
//! Click a clip to add it as a media layer above the selected one, or drag it
//! onto the stack to put it somewhere else (`stack_panel` draws where it
//! lands). Either way the intent carries the item's key, not a path:
//! `main.rs` picks the clip that suits the output's shape when it handles it.
//! A clip not rendered yet is shown dimmed and does nothing.
//!
//! Stills are decoded the first time a tile scrolls into view, a few a frame,
//! and kept as textures; the hover preview decodes on a thread
//! ([`CatalogThumbs`] does both).

use egui::{Color32, RichText, Sense, Stroke, TextureId, Ui, Vec2};

use super::catalog_panel::{CARD_PAD, GAP, NAME_H, PIC, Target, hovered_for, tile_id};
use crate::media::pack::{MediaItem, families, family_label};
use crate::media::pack_watch::MediaPacks;
use crate::ui::catalog_thumbs::CatalogThumbs;
use crate::ui::theme::colors::theme_colors;

/// What a media tile carries while it is dragged.
#[derive(Clone, Debug)]
pub struct MediaDrag {
    /// [`MediaItem::key`].
    pub key: String,
    pub title: String,
}

/// The intent key for adding a pack clip; its value is `(item key, the
/// stack index the new layer goes to)`.
pub const MEDIA_INTENT: &str = "catalog_media";

/// Send the intent that adds the item `key` as a layer at `at`.
pub fn send_add(ctx: &egui::Context, key: String, at: usize) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(MEDIA_INTENT), (key, at)));
}

/// Take the pending intent, if any.
pub fn take_add(ctx: &egui::Context) -> Option<(String, usize)> {
    ctx.data_mut(|d| {
        let id = egui::Id::new(MEDIA_INTENT);
        let v = d.get_temp(id);
        d.remove::<(String, usize)>(id);
        v
    })
}

/// Stills decoded per frame at most: a pack is ~70 of them, and decoding all
/// at once stalls the frame the tab opens on.
const DECODES_PER_FRAME: usize = 4;

/// Does `item` match the search text (already lowercased)?
pub fn matches(item: &MediaItem, q: &str) -> bool {
    q.is_empty()
        || item.title.to_lowercase().contains(q)
        || item.description.to_lowercase().contains(q)
        || item.family.to_lowercase().contains(q)
        || item.id.to_lowercase().contains(q)
        || item.tags.iter().any(|t| t.to_lowercase().contains(q))
}

/// How many items the tab shows for this search.
pub fn count(packs: &MediaPacks, q: &str) -> usize {
    packs.items.iter().filter(|i| matches(i, q)).count()
}

/// The grid: one heading per family, in catalog order, then its clips.
pub fn draw_grid(
    ui: &mut Ui,
    packs: &MediaPacks,
    thumbs: &mut CatalogThumbs,
    q: &str,
    target: &Target,
) {
    let tc = theme_colors(ui.ctx());
    let shown: Vec<&MediaItem> = packs.items.iter().filter(|i| matches(i, q)).collect();
    if shown.is_empty() {
        let msg = if packs.items.is_empty() {
            "No media packs found. FOSFORA_MEDIA_DIRS names a folder holding a media.json."
        } else {
            "Nothing matches."
        };
        ui.label(RichText::new(msg).size(13.0).color(tc.text_secondary));
        return;
    }
    let owned: Vec<MediaItem> = shown.iter().map(|i| (*i).clone()).collect();
    let mut budget = DECODES_PER_FRAME;
    for family in families(&owned) {
        let group: Vec<&MediaItem> = shown
            .iter()
            .copied()
            .filter(|i| family_label(&i.family) == family)
            .collect();
        let ready = group.iter().filter(|i| !i.unrendered()).count();
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(&family).size(14.0).strong());
            let n = if ready == group.len() {
                format!("{ready}")
            } else {
                format!("{ready} of {} rendered", group.len())
            };
            ui.label(RichText::new(n).size(12.0).color(tc.text_secondary));
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(GAP);
            for item in &group {
                tile(ui, item, thumbs, &mut budget, target);
            }
        });
        ui.add_space(GAP);
    }
}

/// The still for `item`, decoding it now if the frame's budget allows.
fn still(
    ctx: &egui::Context,
    thumbs: &mut CatalogThumbs,
    item: &MediaItem,
    budget: &mut usize,
) -> Option<TextureId> {
    let key = item.key();
    if !thumbs.is_cached(&key) {
        if *budget == 0 {
            ctx.request_repaint();
            return None;
        }
        *budget -= 1;
    }
    thumbs.still(ctx, &key, item.thumb.as_deref())
}

fn tile(
    ui: &mut Ui,
    item: &MediaItem,
    thumbs: &mut CatalogThumbs,
    budget: &mut usize,
    target: &Target,
) {
    let tc = theme_colors(ui.ctx());
    let key = item.key();
    let size = Vec2::new(PIC.x, PIC.y + NAME_H) + Vec2::splat(2.0 * CARD_PAD);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let playable = !item.unrendered();
    let sense = if playable {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let resp = ui.interact(rect, tile_id(&key), sense);
    if playable {
        resp.dnd_set_drag_payload(MediaDrag {
            key: key.clone(),
            title: item.title.clone(),
        });
    }
    let pic_rect = egui::Rect::from_min_size(rect.min + Vec2::splat(CARD_PAD), PIC);

    let mut pic = None;
    if ui.is_rect_visible(rect) {
        pic = still(ui.ctx(), thumbs, item, budget);
        if let (Some(secs), Some(preview)) = (hovered_for(ui.ctx(), &key), &item.preview) {
            let anim = format!("{key}#preview");
            pic = thumbs.animated_file(ui.ctx(), &anim, preview, secs).or(pic);
        }
        let painter = ui.painter();
        let stroke = if resp.hovered() && playable {
            Stroke::new(1.5_f32, tc.hover_border)
        } else {
            Stroke::new(1.0_f32, tc.card_border)
        };
        painter.rect(rect, 5.0, tc.card_bg, stroke, egui::StrokeKind::Inside);
        let tint = if playable {
            Color32::WHITE
        } else {
            Color32::from_gray(95)
        };
        draw_picture(painter, pic_rect, pic, item, tint, &tc);
        if !playable {
            let label = egui::Rect::from_center_size(pic_rect.center(), Vec2::new(110.0, 22.0));
            painter.rect_filled(label, 4.0, Color32::from_black_alpha(170));
            painter.text(
                label.center(),
                egui::Align2::CENTER_CENTER,
                "not rendered",
                egui::FontId::proportional(12.0),
                Color32::from_gray(210),
            );
        }
        let name = RichText::new(crate::ui::widgets::truncate_chars(&item.title, 24)).size(13.0);
        let galley = egui::WidgetText::from(name).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            PIC.x,
            egui::TextStyle::Body,
        );
        let color = if playable {
            tc.text_primary
        } else {
            tc.text_secondary
        };
        ui.painter().galley(
            egui::pos2(pic_rect.left(), pic_rect.bottom() + 4.0),
            galley,
            color,
        );
    }

    if resp.dragged() {
        drag_ghost(ui.ctx(), pic, item);
    }
    if resp.clicked() && target.can_add {
        super::stack_panel::show_layer_in_inspector(ui.ctx());
        send_add(ui.ctx(), key.clone(), target.active);
    }
    let resp = resp.on_hover_ui(|ui| tooltip(ui, item, target));
    if playable {
        resp.context_menu(|ui| menu(ui, &key, target));
    }
}

fn tooltip(ui: &mut Ui, item: &MediaItem, target: &Target) {
    let tc = theme_colors(ui.ctx());
    ui.set_max_width(340.0);
    ui.label(RichText::new(&item.title).size(14.0).strong());
    let mut facts = vec![family_label(&item.family).to_string()];
    if item.seconds > 0.0 {
        facts.push(format!("{:.0} s", item.seconds));
    }
    facts.push(
        match (&item.hd, &item.uw) {
            (Some(_), Some(_)) => "16:9 and ultrawide",
            (Some(_), None) => "16:9",
            (None, Some(_)) => "ultrawide",
            (None, None) => "not rendered",
        }
        .to_string(),
    );
    if item.looping {
        facts.push("seamless loop".into());
    }
    ui.label(
        RichText::new(facts.join(" · "))
            .size(12.0)
            .color(tc.text_secondary),
    );
    if !item.description.is_empty() && item.description != item.title {
        ui.label(RichText::new(&item.description).size(13.0));
    }
    if !item.tags.is_empty() {
        let tags: Vec<String> = item.tags.iter().map(|t| format!("#{t}")).collect();
        ui.label(
            RichText::new(tags.join(" "))
                .size(12.0)
                .color(tc.text_secondary),
        );
    }
    ui.add_space(4.0);
    let how = if item.unrendered() {
        "Not rendered yet. It appears here once its clip is rendered and the pack rebuilt."
    } else if !target.can_add {
        "The stack is full."
    } else {
        "Click: add as a media layer above the selected one. Drag onto the stack to put it \
         elsewhere. The ultrawide clip plays on 21:9 output."
    };
    ui.label(RichText::new(how).size(12.0).color(tc.text_secondary));
}

fn menu(ui: &mut Ui, key: &str, target: &Target) {
    let add = |ui: &mut Ui, label: &str, at: usize| {
        if ui
            .add_enabled(target.can_add, egui::Button::new(label))
            .on_disabled_hover_text("The stack is full")
            .clicked()
        {
            super::stack_panel::show_layer_in_inspector(ui.ctx());
            send_add(ui.ctx(), key.to_string(), at);
            ui.close();
        }
    };
    add(ui, "Add above the selected layer", target.active);
    add(ui, "Add at the bottom of the stack", usize::MAX);
}

/// The still, or a placeholder naming the clip when there is none yet.
fn draw_picture(
    painter: &egui::Painter,
    rect: egui::Rect,
    pic: Option<TextureId>,
    item: &MediaItem,
    tint: Color32,
    tc: &crate::ui::theme::colors::ThemeColors,
) {
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    match pic {
        Some(tex) => {
            painter.rect_filled(rect, 3.0, Color32::BLACK);
            painter.image(tex, rect, uv, tint);
        }
        None => {
            painter.rect_filled(rect, 3.0, tc.widget_bg);
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                crate::ui::widgets::truncate_chars(&item.title, 18),
                egui::FontId::proportional(14.0),
                tc.text_secondary,
            );
        }
    }
}

/// The picture follows the pointer while it is dragged.
fn drag_ghost(ctx: &egui::Context, pic: Option<TextureId>, item: &MediaItem) {
    let Some(pos) = ctx.pointer_latest_pos() else {
        return;
    };
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    let tc = theme_colors(ctx);
    let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("v2_catalog_ghost"));
    let painter = ctx.layer_painter(layer);
    let rect = egui::Rect::from_min_size(pos + Vec2::new(12.0, 12.0), PIC * 0.7);
    draw_picture(
        &painter,
        rect,
        pic,
        item,
        Color32::from_white_alpha(220),
        &tc,
    );
    painter.rect_stroke(
        rect,
        3.0,
        Stroke::new(1.5_f32, tc.text_primary),
        egui::StrokeKind::Outside,
    );
}

#[cfg(test)]
#[path = "catalog_media_tests.rs"]
mod tests;
