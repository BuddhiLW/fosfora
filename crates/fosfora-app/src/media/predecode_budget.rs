//! How much RAM a video pre-decode may take, and whether a clip fits.
//!
//! The duration cap alone bounds nothing: 60 s of 2560x1080 at 30 fps is
//! ~19 GiB of RGBA. A clip is pre-decoded only when its estimated frames fit
//! [`predecode_budget_bytes`]; past it the media layer streams instead.

use super::video::VideoMeta;

/// Environment variable overriding [`DEFAULT_PREDECODE_MAX_MB`].
pub const PREDECODE_MAX_MB_ENV: &str = "FOSFORA_PREDECODE_MAX_MB";

/// RAM one clip may take as pre-decoded frames, in MiB. Same figure as the
/// GIF pre-decode budget.
pub const DEFAULT_PREDECODE_MAX_MB: u64 = 2048;

impl VideoMeta {
    /// Bytes of one decoded RGBA frame.
    pub fn frame_bytes(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height) * 4
    }

    /// Frames the probe expects (0 when it reported no duration).
    pub fn estimated_frames(&self) -> u64 {
        let n = (self.duration_secs * self.fps).ceil();
        if n.is_finite() && n > 0.0 {
            n as u64
        } else {
            0
        }
    }

    /// RAM a full pre-decode is expected to take.
    pub fn estimated_predecode_bytes(&self) -> u64 {
        self.estimated_frames().saturating_mul(self.frame_bytes())
    }
}

/// The budget in MiB from the raw env value: a positive integer, else the default.
pub fn parse_predecode_max_mb(raw: Option<&str>) -> u64 {
    raw.and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|&mb| mb > 0)
        .unwrap_or(DEFAULT_PREDECODE_MAX_MB)
}

/// The pre-decode budget in bytes, read from [`PREDECODE_MAX_MB_ENV`].
pub fn predecode_budget_bytes() -> u64 {
    let raw = std::env::var(PREDECODE_MAX_MB_ENV).ok();
    parse_predecode_max_mb(raw.as_deref()).saturating_mul(1 << 20)
}

/// Why a clip should not be pre-decoded, or `None` when it fits both the
/// duration cap and the byte budget.
pub fn predecode_refusal(meta: &VideoMeta, max_secs: f64, budget_bytes: u64) -> Option<String> {
    if meta.duration_secs > max_secs {
        return Some(format!(
            "{:.0} s is over the {:.0} s pre-decode limit",
            meta.duration_secs, max_secs
        ));
    }
    let need = meta.estimated_predecode_bytes();
    if need > budget_bytes {
        return Some(format!(
            "pre-decoding needs ~{} MiB, over the {} MiB budget ({PREDECODE_MAX_MB_ENV})",
            need >> 20,
            budget_bytes >> 20
        ));
    }
    None
}

/// Why a pre-decode stopped without frames.
#[derive(Debug, Clone, PartialEq)]
pub enum PredecodeError {
    /// The frames outgrew the byte budget mid-decode (the probe's estimate
    /// was short or missing). The caller may stream the clip instead.
    OverBudget(String),
    /// Cancelled, ffmpeg failed, or nothing came out.
    Failed(String),
}

impl From<PredecodeError> for String {
    fn from(e: PredecodeError) -> String {
        match e {
            PredecodeError::OverBudget(s) | PredecodeError::Failed(s) => s,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::video::MAX_PREDECODE_SECS;

    fn meta(width: u32, height: u32, fps: f64, duration_secs: f64) -> VideoMeta {
        VideoMeta {
            width,
            height,
            fps,
            duration_secs,
        }
    }

    // The clip that took 12.6 GB live: 40 s of 2560x1080 at 30 fps.
    #[test]
    fn estimates_the_ram_a_predecode_takes() {
        let m = meta(2560, 1080, 30.0, 40.0);
        assert_eq!(m.frame_bytes(), 2560 * 1080 * 4);
        assert_eq!(m.estimated_frames(), 1200);
        assert_eq!(m.estimated_predecode_bytes(), 1200 * 2560 * 1080 * 4);
        assert_eq!(meta(64, 36, 30.0, 0.0).estimated_frames(), 0);
        assert_eq!(meta(64, 36, f64::NAN, 2.0).estimated_frames(), 0);
    }

    #[test]
    fn budget_env_parses_or_falls_back() {
        assert_eq!(parse_predecode_max_mb(None), DEFAULT_PREDECODE_MAX_MB);
        assert_eq!(parse_predecode_max_mb(Some("512")), 512);
        assert_eq!(parse_predecode_max_mb(Some(" 8192 ")), 8192);
        for bad in ["", "0", "-1", "2G", "lots"] {
            assert_eq!(
                parse_predecode_max_mb(Some(bad)),
                DEFAULT_PREDECODE_MAX_MB,
                "{bad:?}"
            );
        }
    }

    // Under the 60 s cap is not enough: the bytes must fit too.
    #[test]
    fn a_short_but_huge_clip_is_refused_by_bytes() {
        let budget = DEFAULT_PREDECODE_MAX_MB << 20;
        let huge = meta(2560, 1080, 30.0, 40.0);
        let why = predecode_refusal(&huge, MAX_PREDECODE_SECS, budget).expect("refused");
        assert!(
            why.contains("12656 MiB") && why.contains("2048 MiB"),
            "{why}"
        );
        let small = meta(1280, 720, 30.0, 10.0);
        assert_eq!(predecode_refusal(&small, MAX_PREDECODE_SECS, budget), None);
        let long = meta(64, 36, 30.0, 61.0);
        let why = predecode_refusal(&long, MAX_PREDECODE_SECS, budget).expect("refused");
        assert!(why.contains("61 s"), "{why}");
    }

    // Property: within the duration cap, refusal happens exactly when the
    // estimate exceeds the budget (seeded xorshift, 500 shapes).
    #[test]
    fn refusal_matches_the_estimate_for_many_shapes() {
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..500 {
            let m = meta(
                1 + (next() % 4096) as u32,
                1 + (next() % 2160) as u32,
                1.0 + (next() % 120) as f64,
                (next() % 60) as f64,
            );
            let budget = next() % (8 << 30);
            let refused = predecode_refusal(&m, MAX_PREDECODE_SECS, budget).is_some();
            assert_eq!(
                refused,
                m.estimated_predecode_bytes() > budget,
                "{m:?} {budget}"
            );
        }
    }

    #[test]
    fn predecode_error_converts_to_its_message() {
        assert_eq!(
            String::from(PredecodeError::OverBudget("big".into())),
            "big"
        );
        assert_eq!(String::from(PredecodeError::Failed("bad".into())), "bad");
    }
}
