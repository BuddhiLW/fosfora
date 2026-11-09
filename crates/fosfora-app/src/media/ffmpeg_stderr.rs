//! Keep what an ffmpeg child says on stderr, so a failure reports ffmpeg's
//! own words and exit status instead of a bare "decoded zero frames".

use std::io::Read;
use std::process::ChildStderr;
use std::thread::JoinHandle;

/// Most of ffmpeg's stderr kept for an error message.
const STDERR_TAIL_BYTES: usize = 4096;

/// Append `chunk` to `tail`, keeping only the last `max` bytes.
fn push_tail(tail: &mut Vec<u8>, chunk: &[u8], max: usize) {
    tail.extend_from_slice(chunk);
    if tail.len() > max {
        let cut = tail.len() - max;
        tail.drain(..cut);
    }
}

/// Drain `stderr` on its own thread (an undrained pipe can stall ffmpeg),
/// returning its last few KiB as text when joined.
pub fn spawn_tail(stderr: Option<ChildStderr>) -> Option<JoinHandle<String>> {
    let mut stderr = stderr?;
    std::thread::Builder::new()
        .name("ffmpeg-stderr".into())
        .spawn(move || {
            let mut tail = Vec::new();
            let mut chunk = [0u8; 1024];
            while let Ok(n) = stderr.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                push_tail(&mut tail, &chunk[..n], STDERR_TAIL_BYTES);
            }
            String::from_utf8_lossy(&tail).trim().to_string()
        })
        .ok()
}

/// The error for a run that produced no frames, with ffmpeg's last line.
pub fn zero_frames_error(exit_code: Option<i32>, stderr: &str) -> String {
    let exit = match exit_code {
        Some(c) => format!("exit code {c}"),
        None => "killed by a signal".to_string(),
    };
    match stderr.lines().rev().find(|l| !l.trim().is_empty()) {
        Some(line) => format!("ffmpeg decoded zero frames ({exit}): {}", line.trim()),
        None => format!("ffmpeg decoded zero frames ({exit}, no error output)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_keeps_the_last_bytes() {
        let mut tail = Vec::new();
        push_tail(&mut tail, b"abcdef", 4);
        assert_eq!(tail, b"cdef");
        push_tail(&mut tail, b"gh", 4);
        assert_eq!(tail, b"efgh");
        push_tail(&mut tail, b"0123456789", 4);
        assert_eq!(tail, b"6789");
    }

    #[test]
    fn zero_frames_error_carries_ffmpegs_words() {
        let e = zero_frames_error(
            Some(1),
            "[h264 @ 0x1] decode_slice_header error\n\
             clip.mp4: Invalid data found when processing input\n",
        );
        assert_eq!(
            e,
            "ffmpeg decoded zero frames (exit code 1): \
             clip.mp4: Invalid data found when processing input"
        );
        assert_eq!(
            zero_frames_error(None, "  \n"),
            "ffmpeg decoded zero frames (killed by a signal, no error output)"
        );
    }

    // A real child: the tail comes back once the pipe closes.
    #[test]
    fn tail_reads_a_child_to_the_end() {
        let child = std::process::Command::new("sh")
            .args(["-c", "echo first >&2; echo last >&2"])
            .stderr(std::process::Stdio::piped())
            .spawn();
        let Ok(mut child) = child else {
            eprintln!("no sh; skipped");
            return;
        };
        let handle = spawn_tail(child.stderr.take()).expect("thread");
        let _ = child.wait();
        assert_eq!(handle.join().unwrap(), "first\nlast");
    }
}
