//! appreels post-render: frame recorded video with the polish-core look.

mod cards;
mod effects;
mod text;
mod timeline;

pub use cards::render_card;
pub use effects::{ZoomTransform, apply_zoom, draw_caption, draw_cursor_ring};
pub use timeline::{
    Caption, CaptionPosition, Card, CursorSample, Timeline, ZoomCue, ZoomState, caption_at,
    cursor_at, parse_cursor_track, zoom_at,
};

use std::io::{Read, Write};
use std::process::{Command, Stdio};

use image::RgbaImage;
use polish_core::{FrameComposer, PresentationStyle};
use thiserror::Error;

use text::font as load_font;

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
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "ultrafast".into(),
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

/// Number of frames a card of `ms` lasts at `fps`.
pub fn card_frame_count(ms: u32, fps: f64) -> u32 {
    ((f64::from(ms) * fps) / 1000.0).round() as u32
}

/// Summary of a timeline-aware render.
#[derive(Debug, Clone)]
pub struct RenderOutcome {
    pub info: VideoInfo,
    pub warnings: Vec<String>,
    pub captions: usize,
    pub zooms: usize,
    pub cursor_track_used: bool,
    pub title_card: bool,
    pub outro_card: bool,
}

/// Decode `input`, frame each frame through `compose_frame`, and re-encode to `output`.
/// This is the backwards-compatible no-effects render.
pub fn frame_video(
    input: &str,
    output: &str,
    style: &PresentationStyle,
) -> Result<VideoInfo, RenderError> {
    render_video(input, output, style, &Timeline::default(), None).map(|outcome| outcome.info)
}

/// Decode `input`, apply timeline effects, and re-encode to `output`.
pub fn render_video(
    input: &str,
    output: &str,
    style: &PresentationStyle,
    timeline: &Timeline,
    cursor_track_path: Option<&str>,
) -> Result<RenderOutcome, RenderError> {
    let info = probe(input)?;
    let (w, h) = (info.width, info.height);
    let canvas_w = w + style.padding * 2;
    let canvas_h = h + style.padding * 2 + style.shadow_offset_y as u32;
    let composer = FrameComposer::new(w, h, style);
    let font = load_font();
    let mut warnings = Vec::new();
    let track_path = cursor_track_path
        .map(str::to_string)
        .or_else(|| timeline.cursor_track.clone());
    let cursor_samples = match &track_path {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(text) => parse_cursor_track(&text),
            Err(e) => {
                warnings.push(format!(
                    "cursor track {path:?} unreadable: {e}; ring skipped"
                ));
                Vec::new()
            }
        },
        None => Vec::new(),
    };
    let cursor_track_used = !cursor_samples.is_empty();

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

    let read_encoder_stderr = |enc_err: &mut Option<std::process::ChildStderr>| -> String {
        let mut text = String::new();
        if let Some(stderr) = enc_err.as_mut() {
            let _ = stderr.read_to_string(&mut text);
        }
        text.trim().to_string()
    };

    macro_rules! write_frame {
        ($canvas:expr) => {{
            let canvas: RgbaImage = $canvas;
            if let Err(e) = enc_in.write_all(canvas.as_raw()) {
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
        }};
    }

    let has_title = timeline.title_card.is_some();
    if let Some(card) = &timeline.title_card {
        let frame = cards::render_card(canvas_w, canvas_h, &card.text, style);
        for _ in 0..card_frame_count(card.ms, info.fps) {
            write_frame!(frame.clone());
        }
    }

    let mut frame_index: u64 = 0;
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
        let t_ms = (frame_index as f64) * 1000.0 / info.fps;
        let frame = RgbaImage::from_vec(w, h, buf.clone()).expect("frame dimensions");
        let (mut working, xform) = match zoom_at(&timeline.zooms, t_ms) {
            Some(z) => apply_zoom(&frame, z),
            None => (frame, ZoomTransform::identity()),
        };
        if let Some((cx, cy)) = cursor_at(&cursor_samples, t_ms) {
            let (rx, ry) = xform.map(cx, cy);
            draw_cursor_ring(&mut working, rx, ry, style.accent);
        }
        let mut composed = composer.compose(&working);
        if let Some(caption) = caption_at(&timeline.captions, t_ms) {
            draw_caption(
                &mut composed,
                &font,
                &caption.text,
                style.accent,
                caption.position,
            );
        }
        write_frame!(composed);
        frame_index += 1;
    }

    let has_outro = timeline.outro_card.is_some();
    if let Some(card) = &timeline.outro_card {
        let frame = cards::render_card(canvas_w, canvas_h, &card.text, style);
        for _ in 0..card_frame_count(card.ms, info.fps) {
            write_frame!(frame.clone());
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
    Ok(RenderOutcome {
        info,
        warnings,
        captions: timeline.captions.len(),
        zooms: timeline.zooms.len(),
        cursor_track_used,
        title_card: has_title,
        outro_card: has_outro,
    })
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
    fn card_frame_count_rounds_to_fps() {
        assert_eq!(card_frame_count(1000, 30.0), 30);
        assert_eq!(card_frame_count(1500, 30.0), 45);
        assert_eq!(card_frame_count(0, 30.0), 0);
    }

    #[test]
    #[ignore = "needs ffmpeg/ffprobe"]
    fn renders_a_clip_with_effects() {
        let dir = std::env::temp_dir();
        let src = dir.join("appreels-render-src.mp4");
        let out = dir.join("appreels-render-out.mp4");
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
        let timeline = Timeline {
            title_card: Some(Card {
                text: "Demo".into(),
                ms: 500,
            }),
            outro_card: Some(Card {
                text: "Thanks".into(),
                ms: 500,
            }),
            captions: vec![Caption {
                start_ms: 0,
                end_ms: 600,
                text: "hello".into(),
                position: CaptionPosition::Bottom,
            }],
            zooms: vec![ZoomCue {
                start_ms: 200,
                end_ms: 800,
                x: 160.0,
                y: 120.0,
                scale: 1.6,
            }],
            ..Default::default()
        };
        let outcome = render_video(
            src.to_str().unwrap(),
            out.to_str().unwrap(),
            &style,
            &timeline,
            None,
        )
        .expect("render");
        assert_eq!((outcome.info.width, outcome.info.height), (320, 240));
        assert_eq!(outcome.captions, 1);

        let probed = probe(out.to_str().unwrap()).expect("probe out");
        assert!(probed.width >= 320 + style.padding * 2);
    }
}
