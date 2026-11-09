//! The pre-decode RAM budget and ffmpeg error reporting against real ffmpeg
//! runs. Each test skips where ffmpeg is missing.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use super::decoder::{MediaProgress, MediaSource, load_video_with_cap};
use super::predecode_budget::PredecodeError;
use super::video::{VideoMeta, decode_all_frames, ffmpeg_available, probe_video};

/// A 2 s, 10 fps, 64x36 test clip, or None where ffmpeg is missing.
fn clip(dir: &Path) -> Option<PathBuf> {
    if !ffmpeg_available() {
        return None;
    }
    let path = dir.join("clip.mp4");
    let ok = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("testsrc=size=64x36:rate=10:duration=2")
        .args(["-pix_fmt", "yuv420p"])
        .arg(&path)
        .status()
        .is_ok_and(|s| s.success());
    ok.then_some(path)
}

// A short clip whose frames would outgrow the RAM budget streams too:
// the 2 s 64x36 clip needs ~180 KiB, so a 64 KiB budget refuses it.
#[test]
fn a_video_over_the_ram_budget_streams() {
    let dir = tempfile::tempdir().unwrap();
    let Some(path) = clip(dir.path()) else {
        eprintln!("ffmpeg not found; skipped");
        return;
    };
    let progress = MediaProgress::default();
    let source = load_video_with_cap(&path, &progress, 60.0, 64 << 10).expect("streams");
    assert!(matches!(source, MediaSource::Streaming(_)));
    assert_eq!(
        progress.done.load(Ordering::Relaxed),
        0,
        "nothing pre-decoded"
    );
}

// When the probe under-reports, the budget still holds during the decode,
// and loading falls back to streaming instead of growing without bound.
#[test]
fn a_decode_that_outgrows_its_budget_stops() {
    let dir = tempfile::tempdir().unwrap();
    let Some(path) = clip(dir.path()) else {
        eprintln!("ffmpeg not found; skipped");
        return;
    };
    let mut meta = probe_video(&path).unwrap();
    meta.duration_secs = 0.0; // as if the container had no duration
    let budget = 5 * meta.frame_bytes();
    let progress = MediaProgress::default();
    match decode_all_frames(&path, &meta, &progress, budget) {
        Err(PredecodeError::OverBudget(why)) => assert!(why.contains("frame 6"), "{why}"),
        Err(e) => panic!("expected over budget, got {e:?}"),
        Ok((frames, _)) => panic!("decoded {} frames past the budget", frames.len()),
    }
    assert_eq!(progress.done.load(Ordering::Relaxed), 5);
}

// A file ffmpeg cannot read says why instead of a bare "zero frames".
#[test]
fn a_broken_video_reports_ffmpegs_error() {
    if !ffmpeg_available() {
        eprintln!("ffmpeg not found; skipped");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("broken.mp4");
    std::fs::write(&path, b"not a video at all").unwrap();
    let meta = VideoMeta {
        width: 64,
        height: 36,
        fps: 10.0,
        duration_secs: 2.0,
    };
    match decode_all_frames(&path, &meta, &MediaProgress::default(), u64::MAX) {
        Err(PredecodeError::Failed(e)) => {
            assert!(
                e.starts_with("ffmpeg decoded zero frames (exit code"),
                "{e}"
            );
            assert!(!e.contains("no error output"), "stderr was dropped: {e}");
        }
        other => panic!("expected a failure, got {:?}", other.map(|(f, _)| f.len())),
    }
}
