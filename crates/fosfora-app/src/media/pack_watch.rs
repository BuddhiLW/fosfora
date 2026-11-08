//! The loaded media packs ([`super::pack`]) and the watch that rereads them
//! when tessera rewrites a manifest (it re-runs `clojure -M:media` as new
//! renders land).

use std::path::PathBuf;

use super::pack::{MediaItem, extra_dirs, scan};

/// The loaded packs, and the watch that reloads them.
pub struct MediaPacks {
    dirs: Vec<PathBuf>,
    pub items: Vec<MediaItem>,
    /// Bumped on every reload, so a view can tell the list changed.
    pub generation: u64,
    #[cfg(feature = "desktop")]
    watch: Option<Watch>,
}

#[cfg(feature = "desktop")]
struct Watch {
    _debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
    rx: crossbeam_channel::Receiver<()>,
}

impl MediaPacks {
    /// The packs [`super::pack::ENV`] names, scanned now and watched.
    pub fn from_env() -> Self {
        Self::new(extra_dirs())
    }

    /// The packs in `dirs`, scanned now and watched for changes.
    pub fn new(dirs: Vec<PathBuf>) -> Self {
        let items = scan(&dirs);
        Self {
            #[cfg(feature = "desktop")]
            watch: watch(&dirs),
            dirs,
            items,
            generation: 0,
        }
    }

    /// No pack directories, nothing watched: for tests and headless use.
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    /// Is any pack directory configured? The Media tab shows only then.
    pub fn configured(&self) -> bool {
        !self.dirs.is_empty() || !self.items.is_empty()
    }

    /// Reread every manifest if one changed since the last call. Cheap when
    /// nothing did. Returns whether it reloaded.
    pub fn poll(&mut self) -> bool {
        let changed = self.watch_fired();
        if changed {
            self.reload();
        }
        changed
    }

    #[cfg(feature = "desktop")]
    fn watch_fired(&self) -> bool {
        let Some(w) = &self.watch else {
            return false;
        };
        let mut fired = false;
        while w.rx.try_recv().is_ok() {
            fired = true;
        }
        fired
    }

    #[cfg(not(feature = "desktop"))]
    fn watch_fired(&self) -> bool {
        false
    }

    /// Reread every manifest now.
    pub fn reload(&mut self) {
        self.items = scan(&self.dirs);
        self.generation += 1;
    }
}

/// Watch `dirs` recursively and signal when a `media.json` is written,
/// created or removed. On Linux a read is an event too (inotify OPEN), so
/// the file's stamp is compared before signalling: reading the manifest to
/// reload it must not trigger another reload.
#[cfg(feature = "desktop")]
fn watch(dirs: &[PathBuf]) -> Option<Watch> {
    use std::collections::HashMap;
    use std::path::Path;

    use notify_debouncer_mini::{DebouncedEventKind, new_debouncer};

    use super::pack::{is_manifest, manifests};

    type Stamp = Option<(std::time::SystemTime, u64)>;
    fn stamp(p: &Path) -> Stamp {
        let m = std::fs::metadata(p).ok()?;
        Some((m.modified().ok()?, m.len()))
    }

    let dirs: Vec<&PathBuf> = dirs.iter().filter(|d| d.is_dir()).collect();
    if dirs.is_empty() {
        return None;
    }
    let mut seen: HashMap<PathBuf, Stamp> = HashMap::new();
    for d in &dirs {
        for m in manifests(d) {
            let s = stamp(&m);
            seen.insert(m, s);
        }
    }
    let (tx, rx) = crossbeam_channel::unbounded();
    let debouncer = new_debouncer(
        // The pack writer rewrites the manifest after its clips and thumbs:
        // half a second lets a burst of writes settle into one reload.
        std::time::Duration::from_millis(500),
        move |res: Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>| {
            let Ok(events) = res else {
                return;
            };
            let mut changed = false;
            for e in events {
                if e.kind != DebouncedEventKind::Any || !is_manifest(&e.path) {
                    continue;
                }
                let now = stamp(&e.path);
                if seen.insert(e.path.clone(), now) != Some(now) {
                    changed = true;
                }
            }
            if changed {
                let _ = tx.send(());
            }
        },
    );
    let mut debouncer = match debouncer {
        Ok(d) => d,
        Err(e) => {
            log::warn!("Media packs will not reload: cannot create a file watcher: {e}");
            return None;
        }
    };
    for d in dirs {
        match debouncer
            .watcher()
            .watch(d, notify::RecursiveMode::Recursive)
        {
            Ok(()) => log::info!("Watching {} for media pack changes", d.display()),
            Err(e) => log::warn!("Media pack {} will not reload: {e}", d.display()),
        }
    }
    Some(Watch {
        _debouncer: debouncer,
        rx,
    })
}

#[cfg(all(test, feature = "desktop", target_os = "linux"))]
mod tests {
    use super::*;

    // tessera rewrites media.json as renders land; the catalog picks the new
    // items up without a restart. Reading the manifest (which the reload
    // does) is not itself a change.
    #[test]
    fn a_rewritten_manifest_reloads_and_a_read_does_not() {
        let root = std::env::temp_dir().join(format!("fosfora-media-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("pack")).unwrap();
        let manifest = root.join("pack/media.json");
        std::fs::write(&manifest, r#"{"items":[{"id":"a"}]}"#).unwrap();

        let mut packs = MediaPacks::new(vec![root.clone()]);
        assert_eq!(packs.items.len(), 1);
        let wait = |packs: &mut MediaPacks, ms: u64, stop_early: bool| {
            let until = std::time::Instant::now() + std::time::Duration::from_millis(ms);
            let mut reloaded = false;
            while std::time::Instant::now() < until && !(reloaded && stop_early) {
                reloaded |= packs.poll();
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            reloaded
        };

        std::fs::read_to_string(&manifest).unwrap();
        assert!(!wait(&mut packs, 1200, false), "a read is not a change");

        std::fs::write(
            &manifest,
            r#"{"items":[{"id":"a"},{"id":"b","clips":{"hd":"b.mp4"}}]}"#,
        )
        .unwrap();
        assert!(wait(&mut packs, 5000, true), "a rewrite reloads");
        assert_eq!(packs.items.len(), 2);
        assert_eq!(packs.generation, 1);
        let _ = std::fs::remove_dir_all(&root);
    }
}
