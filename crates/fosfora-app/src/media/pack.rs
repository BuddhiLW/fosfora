//! Media packs: folders of ready-made clips with a manifest and thumbnails,
//! browsed in the catalog's Media tab.
//!
//! `FOSFORA_MEDIA_DIRS` lists directories (same rules as
//! `FOSFORA_EFFECT_DIRS`: split like `PATH`, a leading `~` is home). Each is
//! searched recursively for `media.json`, the manifest tessera's
//! `tessera.media/emit-pack!` writes (tessera `docs/media.md`):
//!
//! ```json
//! {"version": 1,
//!  "items": [{"id": "choreographies", "title": "Three-Body Choreographies",
//!             "description": "...", "family": "Orbits", "tags": ["three-body"],
//!             "seconds": 24, "fps": 30, "loop": true,
//!             "clips": {"hd": "choreographies/clip-hd.mp4",
//!                       "uw": "choreographies/clip-uw.mp4"},
//!             "thumb": "choreographies/thumb.webp",
//!             "preview": "choreographies/preview.webp"}]}
//! ```
//!
//! Every path is relative to the manifest's directory and is made absolute
//! here. Unknown fields are ignored, so a newer pack still reads. An item
//! with no clips has not been rendered yet: it is kept, so the catalog can
//! show it dimmed, but it cannot be played.
//!
//! The manifest is rewritten whenever new renders land; [`super::pack_watch`]
//! watches the directories and rereads every pack when one changes.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The variable naming the pack directories.
pub const ENV: &str = "FOSFORA_MEDIA_DIRS";

/// The manifest's file name.
pub const MANIFEST: &str = "media.json";

/// Families in the order the catalog shows them. A family not listed here
/// goes after these, alphabetically.
pub const FAMILY_ORDER: [&str; 10] = [
    "Orbits", "Curves", "Knots", "Fractals", "Lattices", "Waves", "Quantum", "Tilings", "Flows",
    "Scenes",
];

/// Output at least this wide (width over height) plays the ultrawide clip.
/// 21:9 output is 2.33:1 (2560x1080 is 2.37); 16:9 is 1.78.
pub const ULTRAWIDE_ASPECT: f32 = 2.2;

/// Pack directories from [`ENV`], absolute, in the order given.
pub fn extra_dirs() -> Vec<PathBuf> {
    crate::trama::dirs::parse(std::env::var_os(ENV), std::env::var_os("HOME"))
}

/// A manifest as written on disk.
#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default)]
    items: Vec<RawItem>,
}

#[derive(Debug, Deserialize)]
struct RawItem {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    family: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    seconds: f64,
    #[serde(default)]
    fps: f64,
    #[serde(default, rename = "loop")]
    looping: bool,
    #[serde(default)]
    clips: RawClips,
    #[serde(default)]
    thumb: Option<String>,
    #[serde(default)]
    preview: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawClips {
    #[serde(default)]
    hd: Option<String>,
    #[serde(default)]
    uw: Option<String>,
}

/// One clip of a pack, with every path absolute.
#[derive(Clone, Debug, PartialEq)]
pub struct MediaItem {
    pub id: String,
    /// The short tile title; the id when the manifest has none.
    pub title: String,
    pub description: String,
    pub family: String,
    pub tags: Vec<String>,
    pub seconds: f64,
    pub fps: f64,
    /// The clip is seamless.
    pub looping: bool,
    /// The 16:9 clip (1920x1080).
    pub hd: Option<PathBuf>,
    /// The ultrawide clip (2560x1080).
    pub uw: Option<PathBuf>,
    /// A still, 480x270.
    pub thumb: Option<PathBuf>,
    /// An animated WebP, a few seconds long.
    pub preview: Option<PathBuf>,
}

impl MediaItem {
    /// Not rendered yet: there is no clip to play.
    pub fn unrendered(&self) -> bool {
        self.hd.is_none() && self.uw.is_none()
    }

    /// The clip for output of this aspect ratio (width over height): the
    /// ultrawide one on output at least [`ULTRAWIDE_ASPECT`] wide, when it
    /// exists; otherwise the 16:9 one; otherwise whichever exists.
    pub fn clip_for(&self, aspect: f32) -> Option<&Path> {
        let (first, second) = if aspect >= ULTRAWIDE_ASPECT {
            (&self.uw, &self.hd)
        } else {
            (&self.hd, &self.uw)
        };
        first.as_deref().or(second.as_deref())
    }

    /// A key that tells this item from every other, across packs: an id is
    /// unique only inside its own manifest.
    pub fn key(&self) -> String {
        let at = self
            .thumb
            .as_deref()
            .or(self.hd.as_deref())
            .or(self.uw.as_deref());
        match at {
            Some(p) => format!("media:{}@{}", self.id, p.display()),
            None => format!("media:{}", self.id),
        }
    }
}

/// `rel` against the manifest's directory. Absolute paths are kept, and an
/// empty one means none.
fn resolve(base: &Path, rel: Option<String>) -> Option<PathBuf> {
    let rel = rel?;
    if rel.trim().is_empty() {
        return None;
    }
    Some(base.join(rel))
}

/// Parse a manifest's JSON. `base` is the directory it lives in. Items with
/// no id are dropped.
pub fn parse(json: &str, base: &Path) -> Result<Vec<MediaItem>, serde_json::Error> {
    let manifest: Manifest = serde_json::from_str(json)?;
    Ok(manifest
        .items
        .into_iter()
        .filter(|i| !i.id.is_empty())
        .map(|i| MediaItem {
            title: if i.title.is_empty() {
                i.id.clone()
            } else {
                i.title
            },
            id: i.id,
            description: i.description,
            family: i.family,
            tags: i.tags,
            seconds: i.seconds,
            fps: i.fps,
            looping: i.looping,
            hd: resolve(base, i.clips.hd),
            uw: resolve(base, i.clips.uw),
            thumb: resolve(base, i.thumb),
            preview: resolve(base, i.preview),
        })
        .collect())
}

/// Read one manifest file. `None` (logged) when it is unreadable.
pub fn read_manifest(path: &Path) -> Option<Vec<MediaItem>> {
    let json = std::fs::read_to_string(path)
        .map_err(|e| log::warn!("Failed to read {}: {e}", path.display()))
        .ok()?;
    let base = path.parent().unwrap_or(Path::new("."));
    parse(&json, base)
        .map_err(|e| log::warn!("Failed to parse {}: {e}", path.display()))
        .ok()
}

/// Is `path` a manifest? The watcher reloads on these only.
pub fn is_manifest(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n == MANIFEST)
}

/// Every `media.json` under `dir`, recursively, sorted by path.
pub fn manifests(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for path in entries.filter_map(Result::ok).map(|e| e.path()) {
        if path.is_dir() {
            out.extend(manifests(&path));
        } else if is_manifest(&path) {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// The items of every manifest in `dirs`, in directory then path order.
pub fn scan(dirs: &[PathBuf]) -> Vec<MediaItem> {
    let mut items = Vec::new();
    for dir in dirs {
        if !dir.is_dir() {
            log::warn!("{ENV} names {}, which is not a directory", dir.display());
            continue;
        }
        for manifest in manifests(dir) {
            if let Some(found) = read_manifest(&manifest) {
                let ready = found.iter().filter(|i| !i.unrendered()).count();
                log::info!(
                    "Media pack {}: {} item(s), {ready} rendered",
                    manifest.display(),
                    found.len()
                );
                items.extend(found);
            }
        }
    }
    items
}

/// Where `family` sorts: its place in [`FAMILY_ORDER`], or after all of them.
pub fn family_rank(family: &str) -> usize {
    FAMILY_ORDER
        .iter()
        .position(|f| f.eq_ignore_ascii_case(family))
        .unwrap_or(FAMILY_ORDER.len())
}

/// The heading an item's family is shown under.
pub fn family_label(family: &str) -> &str {
    if family.trim().is_empty() {
        "Other"
    } else {
        family
    }
}

/// The families present in `items`, in catalog order: [`FAMILY_ORDER`]
/// first, then any others alphabetically. Items with no family are grouped
/// under "Other", last.
pub fn families(items: &[MediaItem]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for i in items {
        let f = family_label(&i.family);
        if !out.iter().any(|o| o == f) {
            out.push(f.to_string());
        }
    }
    out.sort_by(|a, b| {
        let key = |s: &str| (s == "Other", family_rank(s));
        key(a).cmp(&key(b)).then_with(|| a.cmp(b))
    });
    out
}

#[cfg(test)]
#[path = "pack_tests.rs"]
mod tests;
