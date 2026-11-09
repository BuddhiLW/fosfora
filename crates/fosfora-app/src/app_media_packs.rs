//! The App side of the catalog's Media tab: keep the packs fresh, and turn a
//! picked clip into a media layer.
//!
//! A clip goes through [`App::start_media_layer`], the background decode the
//! "add media layer" file dialog and the REPL's `f/media!` use. Media layers
//! loop by default, and every pack clip is a seamless loop.

use std::path::PathBuf;

use super::App;

/// A pack clip waiting to decode, and the stack index its layer goes to.
pub struct PendingPlacement {
    pub path: PathBuf,
    pub at: usize,
}

impl App {
    /// Reread the media packs if a manifest changed, and drop the pictures of
    /// the old ones so rewritten thumbs are decoded again.
    pub fn poll_media_packs(&mut self) {
        if self.media_packs.poll() {
            self.catalog_thumbs.forget_prefix("media:");
            log::info!(
                "Media packs reloaded: {} item(s)",
                self.media_packs.items.len()
            );
        }
    }

    /// Add the pack item `key` as a layer at stack index `at` (past the end:
    /// at the bottom), with the clip that suits the output's shape. Returns
    /// false when there is no such item or it has no clip.
    pub fn add_pack_media(&mut self, key: &str, at: usize) -> bool {
        let (w, h) = self.render_size;
        let aspect = w as f32 / h.max(1) as f32;
        let Some(path) = self
            .media_packs
            .items
            .iter()
            .find(|i| i.key() == key)
            .and_then(|i| i.clip_for(aspect))
            .map(PathBuf::from)
        else {
            return false;
        };
        let before = self.media_loads.len();
        self.start_media_layer(path.clone());
        if self.media_loads.len() > before {
            self.media_placements.push(PendingPlacement { path, at });
        }
        true
    }

    /// A decode for `path` finished. If it was a pack clip and its layer
    /// `landed` (at the bottom of the stack), move that layer to where its
    /// tile was dropped. Either way the placement is used up.
    pub(super) fn place_pack_media(&mut self, path: &std::path::Path, landed: bool) {
        let Some(i) = self.media_placements.iter().position(|p| p.path == path) else {
            return;
        };
        let at = self.media_placements.remove(i).at;
        let n = self.layer_stack.layers.len();
        if !landed || n == 0 {
            return;
        }
        let to = at.min(n - 1);
        self.move_layer(n - 1, to);
        self.layer_stack.active_layer = to;
        self.sync_active_layer();
    }
}
