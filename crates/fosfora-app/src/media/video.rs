//! Video pre-decode via ffmpeg subprocess (feature-gated behind `video`).
//!
//! - `ffprobe` probes metadata (dimensions, fps, duration) synchronously at load time
//! - `ffmpeg -f rawvideo -pix_fmt rgba` decodes ALL frames to memory in one pass
//! - Returns `MediaSource::Animated` — identical to GIF, instant random access
//! - RAM cost: ~3.7MB per frame at 1280x720. A 30s@30fps clip = ~3.3GB, so a
//!   clip is pre-decoded only within the `predecode_budget` byte budget; past it the
//!   media layer streams instead (`decoder::load_video`).

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use super::ffmpeg_stderr::{spawn_tail, zero_frames_error};
use super::predecode_budget::{
    PREDECODE_MAX_MB_ENV, PredecodeError, predecode_budget_bytes, predecode_refusal,
};
use super::types::DecodedFrame;

/// Check if ffmpeg/ffprobe are available on the system. Cached per process.
pub fn ffmpeg_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        Command::new("ffprobe")
            .arg("-version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

/// Video metadata from ffprobe.
#[derive(Debug, Clone)]
pub struct VideoMeta {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration_secs: f64,
}

/// Probe video metadata using ffprobe.
pub fn probe_video(path: &Path) -> Result<VideoMeta, String> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_streams",
            "-show_format",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("ffprobe failed to execute: {e}"))?;

    if !output.status.success() {
        return Err("ffprobe returned non-zero exit code".to_string());
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Failed to parse ffprobe JSON: {e}"))?;

    let streams = json["streams"]
        .as_array()
        .ok_or("No streams in ffprobe output")?;

    let video_stream = streams
        .iter()
        .find(|s| s["codec_type"].as_str() == Some("video"))
        .ok_or("No video stream found")?;

    let width = video_stream["width"].as_u64().ok_or("Missing width")? as u32;
    let height = video_stream["height"].as_u64().ok_or("Missing height")? as u32;

    let fps = parse_frame_rate(video_stream["r_frame_rate"].as_str().unwrap_or("30/1"));

    let duration_secs = json["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| {
            video_stream["duration"]
                .as_str()
                .and_then(|s| s.parse::<f64>().ok())
        })
        .unwrap_or(0.0);

    Ok(VideoMeta {
        width,
        height,
        fps,
        duration_secs,
    })
}

fn parse_frame_rate(rate: &str) -> f64 {
    if let Some((num, den)) = rate.split_once('/') {
        let n: f64 = num.parse().unwrap_or(30.0);
        let d: f64 = den.parse().unwrap_or(1.0);
        if d > 0.0 { n / d } else { 30.0 }
    } else {
        rate.parse().unwrap_or(30.0)
    }
}

/// Maximum video duration (seconds) we'll pre-decode. Beyond this, reject.
pub const MAX_PREDECODE_SECS: f64 = 60.0;

/// Pre-decode all video frames via a single ffmpeg run, stopping once they
/// would take more than `budget_bytes`.
/// Returns (frames, delays_ms) ready for `MediaSource::Animated`.
pub fn decode_all_frames(
    path: &Path,
    meta: &VideoMeta,
    progress: &super::decoder::MediaProgress,
    budget_bytes: u64,
) -> Result<(Vec<DecodedFrame>, Vec<u32>), PredecodeError> {
    use std::sync::atomic::Ordering;
    let frame_bytes = meta.frame_bytes();
    let frame_size = frame_bytes as usize;
    let delay_ms = (1000.0 / meta.fps).round() as u32;
    let est_frames = meta.estimated_frames();
    let budget_frames = budget_bytes / frame_bytes.max(1);
    log::info!(
        "Pre-decoding video: ~{} frames, ~{}MB RAM (budget {}MB)",
        est_frames,
        meta.estimated_predecode_bytes() >> 20,
        budget_bytes >> 20,
    );

    let mut child = Command::new("ffmpeg")
        .args(["-v", "error"])
        .args(super::hwaccel::input_args())
        .args(["-i"])
        .arg(path)
        .args(super::hwaccel::output_args())
        .args([
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-s",
            &format!("{}x{}", meta.width, meta.height),
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| PredecodeError::Failed(format!("Failed to spawn ffmpeg: {e}")))?;
    let stderr_tail = spawn_tail(child.stderr.take());
    let finish = |child: &mut std::process::Child| {
        let status = child.wait().ok();
        let text = stderr_tail.and_then(|h| h.join().ok()).unwrap_or_default();
        (status, text)
    };

    progress.total.store(
        est_frames.min(u64::from(u32::MAX)) as u32,
        Ordering::Relaxed,
    );
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = finish(&mut child);
        return Err(PredecodeError::Failed("ffmpeg: no stdout pipe".into()));
    };

    let reserve = est_frames.min(budget_frames) as usize;
    let mut frames = Vec::with_capacity(reserve);
    let mut delays_ms = Vec::with_capacity(reserve);
    let mut buf = vec![0u8; frame_size];

    while stdout.read_exact(&mut buf).is_ok() {
        if progress.cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = finish(&mut child);
            return Err(PredecodeError::Failed("cancelled".to_string()));
        }
        if frames.len() as u64 >= budget_frames {
            let _ = child.kill();
            let _ = finish(&mut child);
            return Err(PredecodeError::OverBudget(format!(
                "pre-decoding went past the {} MiB budget at frame {} ({PREDECODE_MAX_MB_ENV})",
                budget_bytes >> 20,
                frames.len() + 1,
            )));
        }
        frames.push(DecodedFrame {
            data: buf.clone(),
            width: meta.width,
            height: meta.height,
        });
        delays_ms.push(delay_ms.max(1));
        progress.done.fetch_add(1, Ordering::Relaxed);
    }
    drop(stdout);

    let (status, stderr) = finish(&mut child);
    let exit_code = status.and_then(|s| s.code());
    if frames.is_empty() {
        return Err(PredecodeError::Failed(zero_frames_error(
            exit_code, &stderr,
        )));
    }
    if status.is_some_and(|s| !s.success()) {
        log::warn!(
            "ffmpeg exited with {status:?} after {} frames: {stderr}",
            frames.len()
        );
    }

    log::info!(
        "Decoded {} video frames ({}MB)",
        frames.len(),
        (frames.len() as u64 * frame_bytes) >> 20,
    );

    Ok((frames, delays_ms))
}

/// Everything a video source needs, in one call: the ffmpeg check, the probe,
/// the pre-decode length cap and the decode itself, reporting through
/// `progress` and stopping early once its `cancel` flag is set. Every video
/// source (media layer, particle source, obstacle) comes through here so the
/// cap and the error words are the same everywhere (#3159).
pub fn decode_video(
    path: &Path,
    progress: &super::decoder::MediaProgress,
) -> Result<(Vec<DecodedFrame>, Vec<u32>), String> {
    if !ffmpeg_available() {
        return Err("ffmpeg/ffprobe not found on PATH".to_string());
    }
    let meta = probe_video(path)?;
    log::info!(
        "Video probe: {}x{}, {:.2} fps, {:.1}s",
        meta.width,
        meta.height,
        meta.fps,
        meta.duration_secs,
    );
    let budget = predecode_budget_bytes();
    if let Some(why) = predecode_refusal(&meta, MAX_PREDECODE_SECS, budget) {
        return Err(format!(
            "Video too large to pre-decode: {why}. Use a shorter or smaller clip."
        ));
    }
    decode_all_frames(path, &meta, progress, budget).map_err(String::from)
}
