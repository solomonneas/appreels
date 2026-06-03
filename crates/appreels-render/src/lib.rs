//! appreels post-render: frame recorded video with the polish-core look.

mod timeline;

pub use timeline::{Caption, Card, CursorSample, Timeline, ZoomCue, cursor_at, parse_cursor_track};

use std::io::{Read, Write};
use std::process::{Command, Stdio};

use image::RgbaImage;
use polish_core::{PresentationStyle, compose_frame};
use thiserror::Error;

/// Basic properties of a video stream.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

/// Parse `ffprobe -show_entries stream=width,height,r_frame_rate -of default=noprint_wrappers=1`.
pub fn parse_ffprobe(output: &str) -> Option<VideoInfo> {
    let mut width = None;
    let mut height = None;
    let mut fps = None;
    for line in output.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "width" => width = value.trim().parse().ok(),
            "height" => height = value.trim().parse().ok(),
            "r_frame_rate" => fps = parse_frame_rate(value.trim()),
            _ => {}
        }
    }
    Some(VideoInfo {
        width: width?,
        height: height?,
        fps: fps?,
    })
}

fn parse_frame_rate(value: &str) -> Option<f64> {
    match value.split_once('/') {
        Some((num, den)) => {
            let n: f64 = num.parse().ok()?;
            let d: f64 = den.parse().ok()?;
            if d == 0.0 { None } else { Some(n / d) }
        }
        None => value.parse().ok(),
    }
}

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("failed to run `{program}`: {source}")]
    Spawn {
        program: String,
        source: std::io::Error,
    },
    #[error("`ffmpeg/ffprobe` failed: {0}")]
    Failed(String),
    #[error("could not probe input video: {0}")]
    Probe(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// ffprobe args that print width/height/r_frame_rate for the first video stream.
pub fn ffprobe_args(input: &str) -> Vec<String> {
    vec![
        "-v".into(),
        "error".into(),
        "-select_streams".into(),
        "v:0".into(),
        "-show_entries".into(),
        "stream=width,height,r_frame_rate".into(),
        "-of".into(),
        "default=noprint_wrappers=1".into(),
        input.into(),
    ]
}

/// ffmpeg args to decode `input` to a raw rgba framestream on stdout.
pub fn decode_args(input: &str) -> Vec<String> {
    vec![
        "-v".into(),
        "error".into(),
        "-i".into(),
        input.into(),
        "-f".into(),
        "rawvideo".into(),
        "-pix_fmt".into(),
        "rgba".into(),
        "-".into(),
    ]
}

/// ffmpeg args to encode a raw rgba framestream (`canvas_w`x`canvas_h` @ `fps`) on stdin to `output`.
pub fn encode_args(canvas_w: u32, canvas_h: u32, fps: f64, output: &str) -> Vec<String> {
    vec![
        "-y".into(),
        "-v".into(),
        "error".into(),
        "-f".into(),
        "rawvideo".into(),
        "-pix_fmt".into(),
        "rgba".into(),
        "-s".into(),
        format!("{canvas_w}x{canvas_h}"),
        "-r".into(),
        format!("{fps:.5}"),
        "-i".into(),
        "-".into(),
        "-vf".into(),
        "pad=ceil(iw/2)*2:ceil(ih/2)*2".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        output.into(),
    ]
}

/// Probe a video's dimensions and frame rate.
pub fn probe(input: &str) -> Result<VideoInfo, RenderError> {
    let out = Command::new("ffprobe")
        .args(ffprobe_args(input))
        .output()
        .map_err(|source| RenderError::Spawn {
            program: "ffprobe".into(),
            source,
        })?;
    if !out.status.success() {
        return Err(RenderError::Probe(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    parse_ffprobe(&String::from_utf8_lossy(&out.stdout))
        .ok_or_else(|| RenderError::Probe("missing stream fields".into()))
}

/// Decode `input`, frame each frame through `compose_frame`, and re-encode to `output`.
pub fn frame_video(
    input: &str,
    output: &str,
    style: &PresentationStyle,
) -> Result<VideoInfo, RenderError> {
    let info = probe(input)?;
    let (w, h) = (info.width, info.height);
    // compose_frame canvas size for this input + style.
    let canvas_w = w + style.padding * 2;
    let canvas_h = h + style.padding * 2 + style.shadow_offset_y as u32;

    let mut decoder = Command::new("ffmpeg")
        .args(decode_args(input))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|source| RenderError::Spawn {
            program: "ffmpeg".into(),
            source,
        })?;
    let mut encoder = Command::new("ffmpeg")
        .args(encode_args(canvas_w, canvas_h, info.fps, output))
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| RenderError::Spawn {
            program: "ffmpeg".into(),
            source,
        })?;

    let mut dec_out = decoder.stdout.take().expect("decoder stdout");
    let mut enc_in = encoder.stdin.take().expect("encoder stdin");
    // Take the encoder's stderr so we can report its real failure cause if the
    // write side breaks (the encoder's stdout is a file, not a pipe we own, so
    // reading stderr after wait() cannot deadlock).
    let mut enc_err = encoder.stderr.take();
    let frame_len = (w as usize) * (h as usize) * 4;
    let mut buf = vec![0u8; frame_len];

    // Drain the encoder's captured stderr into a String.
    let read_encoder_stderr = |enc_err: &mut Option<std::process::ChildStderr>| -> String {
        let mut text = String::new();
        if let Some(stderr) = enc_err.as_mut() {
            let _ = stderr.read_to_string(&mut text);
        }
        text.trim().to_string()
    };

    loop {
        match dec_out.read_exact(&mut buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => {
                let _ = decoder.kill();
                let _ = decoder.wait();
                let _ = encoder.wait();
                return Err(RenderError::Io(e));
            }
        }
        let frame = RgbaImage::from_raw(w, h, buf.clone()).expect("frame dimensions");
        let composed = compose_frame(&frame, style);
        if let Err(e) = enc_in.write_all(composed.as_raw()) {
            // The encoder likely died early; reap both children and surface the
            // encoder's real failure cause rather than the BrokenPipe.
            drop(enc_in);
            let _ = decoder.kill();
            let _ = decoder.wait();
            let _ = encoder.wait();
            let stderr = read_encoder_stderr(&mut enc_err);
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                return Err(RenderError::Failed(format!(
                    "encoder exited before all frames were written: {stderr}"
                )));
            }
            return Err(RenderError::Io(e));
        }
    }
    drop(enc_in); // signal EOF to the encoder

    let dec_status = decoder.wait().map_err(RenderError::Io)?;
    let enc_status = encoder.wait().map_err(RenderError::Io)?;
    if !dec_status.success() || !enc_status.success() {
        let stderr = read_encoder_stderr(&mut enc_err);
        return Err(RenderError::Failed(format!(
            "decoder={dec_status:?} encoder={enc_status:?}: {stderr}"
        )));
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ffprobe_stream_info() {
        let output = "width=1280\nheight=720\nr_frame_rate=30/1\n";
        let info = parse_ffprobe(output).expect("info");
        assert_eq!(info.width, 1280);
        assert_eq!(info.height, 720);
        assert!((info.fps - 30.0).abs() < 1e-6);
    }

    #[test]
    fn parses_fractional_frame_rate() {
        let info = parse_ffprobe("width=640\nheight=480\nr_frame_rate=30000/1001\n").expect("info");
        assert!((info.fps - 29.97).abs() < 0.01);
    }

    #[test]
    fn rejects_missing_fields() {
        assert!(parse_ffprobe("width=640\n").is_none());
    }

    #[test]
    fn rejects_zero_denominator_frame_rate() {
        assert_eq!(parse_frame_rate("30/0"), None);
    }

    #[test]
    fn parses_bare_integer_frame_rate() {
        assert_eq!(parse_frame_rate("25"), Some(25.0));
    }

    #[test]
    fn decode_args_emit_rawvideo_rgba() {
        let args = decode_args("in.mp4");
        assert!(args.windows(2).any(|w| w == ["-i", "in.mp4"]));
        assert!(args.windows(2).any(|w| w == ["-pix_fmt", "rgba"]));
        assert!(args.windows(2).any(|w| w == ["-f", "rawvideo"]));
        assert_eq!(args.last().unwrap(), "-");
    }

    #[test]
    fn encode_args_set_size_fps_and_even_pad() {
        let args = encode_args(412, 333, 30.0, "out.mp4");
        assert!(args.windows(2).any(|w| w == ["-s", "412x333"]));
        assert!(args.windows(2).any(|w| w == ["-r", "30.00000"]));
        assert!(
            args.windows(2)
                .any(|w| w == ["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2"])
        );
        assert!(args.windows(2).any(|w| w == ["-pix_fmt", "yuv420p"]));
        assert_eq!(args.last().unwrap(), "out.mp4");
    }

    #[test]
    #[ignore = "needs ffmpeg/ffprobe"]
    fn frames_a_generated_clip() {
        let dir = std::env::temp_dir();
        let src = dir.join("appreels-render-src.mp4");
        let out = dir.join("appreels-render-out.mp4");
        // Generate a 1s 320x240 test clip.
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=320x240:rate=10",
                "-pix_fmt",
                "yuv420p",
                src.to_str().unwrap(),
            ])
            .status()
            .expect("ffmpeg testsrc");
        assert!(status.success());

        let style = polish_core::style_from_seed(42);
        let info =
            frame_video(src.to_str().unwrap(), out.to_str().unwrap(), &style).expect("render");
        assert_eq!((info.width, info.height), (320, 240));

        // Output should be larger than the input region and valid.
        let probed = probe(out.to_str().unwrap()).expect("probe out");
        assert!(probed.width >= 320 + style.padding * 2);
    }
}
