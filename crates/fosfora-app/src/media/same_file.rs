//! Whether two paths name the same media file, so a file already decoding
//! is not decoded again: a double drop, or a repeated REPL or pack call,
//! used to start a second full pre-decode of the same clip.

use std::path::Path;

/// True when `a` and `b` resolve to the same file. Paths that cannot be
/// resolved (the file is gone) compare as written.
pub fn same_media_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings_of_one_file_match() {
        let dir = tempfile::tempdir().unwrap();
        let clip = dir.path().join("clip.mp4");
        std::fs::write(&clip, b"x").unwrap();
        let roundabout = dir.path().join(".").join("sub").join("..").join("clip.mp4");
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        assert!(same_media_file(&clip, &clip));
        assert!(same_media_file(&clip, &roundabout));
    }

    #[test]
    fn different_files_do_not_match() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.mp4"), dir.path().join("b.mp4"));
        std::fs::write(&a, b"x").unwrap();
        std::fs::write(&b, b"x").unwrap();
        assert!(!same_media_file(&a, &b));
    }

    #[test]
    fn missing_files_compare_as_written() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("gone.mp4");
        assert!(same_media_file(&gone, &gone));
        assert!(!same_media_file(&gone, &dir.path().join("other.mp4")));
    }
}
