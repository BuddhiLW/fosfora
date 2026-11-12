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

// A cancel stops a decode whose ffmpeg has stalled and sends no frame:
// ffmpeg reading a FIFO nobody writes blocks forever, so a cancel checked
// only between frames would never be seen.
#[test]
fn a_cancel_stops_a_stalled_decode() {
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    if !ffmpeg_available() {
        eprintln!("ffmpeg not found; skipped");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("stalled.mp4");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .is_ok_and(|s| s.success());
    if !made {
        eprintln!("mkfifo not available; skipped");
        return;
    }
    let meta = VideoMeta {
        width: 64,
        height: 36,
        fps: 10.0,
        duration_secs: 2.0,
    };
    let progress = Arc::new(MediaProgress::default());
    let (tx, rx) = crossbeam_channel::bounded(1);
    let (p, path) = (progress.clone(), fifo.clone());
    std::thread::spawn(move || {
        let _ = tx.send(decode_all_frames(&path, &meta, &p, u64::MAX));
    });
    std::thread::sleep(Duration::from_millis(300));
    let asked = Instant::now();
    progress.cancel.store(true, Ordering::Relaxed);
    let result = rx.recv_timeout(Duration::from_secs(5));
    if result.is_err() {
        // Unstick the regressed decode: a writer that closes at once is EOF.
        drop(std::fs::OpenOptions::new().write(true).open(&fifo));
    }
    match result {
        Ok(Err(PredecodeError::Failed(e))) => assert_eq!(e, "cancelled"),
        Ok(other) => panic!("expected cancelled, got {:?}", other.map(|(f, _)| f.len())),
        Err(e) => panic!("a cancel did not stop the stalled decode within 5 s: {e}"),
    }
    assert!(
        asked.elapsed() < Duration::from_secs(2),
        "{:?}",
        asked.elapsed()
    );
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

/// Resident set size of this process in KiB, from /proc (Linux only).
fn rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

// A by-hand measurement against real clips, through the same entry point a
// dropped file takes, with the budget read from FOSFORA_PREDECODE_MAX_MB.
// Run: FOSFORA_PREDECODE_VERIFY_DIR=<dir with clip.mp4, small.mp4>
//      FOSFORA_PREDECODE_MAX_MB=64 cargo test ... -- --ignored --nocapture measure_
#[test]
#[ignore = "needs FOSFORA_PREDECODE_VERIFY_DIR with real clips"]
fn measure_predecode_budget_on_real_clips() {
    use super::decoder::load_media_with;
    use super::predecode_budget::{predecode_budget_bytes, predecode_refusal};
    use super::video::MAX_PREDECODE_SECS;
    let dir = PathBuf::from(std::env::var("FOSFORA_PREDECODE_VERIFY_DIR").expect("dir"));
    let budget = predecode_budget_bytes();
    eprintln!("budget: {} MiB", budget >> 20);
    for name in ["clip.mp4", "small.mp4"] {
        let path = dir.join(name);
        let meta = probe_video(&path).expect("probe");
        let refusal = predecode_refusal(&meta, MAX_PREDECODE_SECS, budget);
        let before = rss_kib().unwrap_or(0);
        let started = std::time::Instant::now();
        let progress = MediaProgress::default();
        let source = load_media_with(&path, &progress).expect("loads");
        let kind = match &source {
            MediaSource::Streaming(_) => "streaming",
            MediaSource::Animated { .. } => "pre-decoded",
            _ => "other",
        };
        let after = rss_kib().unwrap_or(0);
        eprintln!(
            "{name}: {}x{} {:.2} fps {:.1} s, estimate {} MiB, refusal {:?} -> {kind}, \
             {} frames, {} done, RSS {} -> {} MiB, {:.2} s",
            meta.width,
            meta.height,
            meta.fps,
            meta.duration_secs,
            meta.estimated_predecode_bytes() >> 20,
            refusal,
            source.frame_count(),
            progress.done.load(Ordering::Relaxed),
            before >> 10,
            after >> 10,
            started.elapsed().as_secs_f64(),
        );
        let fits = meta.estimated_predecode_bytes() <= budget;
        assert_eq!(
            matches!(source, MediaSource::Animated { .. }),
            fits,
            "{name}"
        );
    }
}
