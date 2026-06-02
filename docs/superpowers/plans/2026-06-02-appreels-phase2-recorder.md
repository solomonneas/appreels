# appreels Phase 2 (Polished Recorder MVP) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Record a window/region of the X11 desktop to a raw video, then render it into a framed clip using the `polish-core` look, exposed via `appreels record` and `appreels render`.

**Architecture:** Two new crates. `appreels-capture` resolves a target window's screen rect (xdotool) and records it with `ffmpeg -f x11grab`. `appreels-render` decodes the raw video to RGBA frames, frames each frame through `polish-core::compose_frame`, and re-encodes with ffmpeg. All subprocess command-line construction and all text parsing are pure, unit-tested functions; the actual subprocess orchestration is covered by `#[ignore]`d integration tests that need ffmpeg + an X display.

**Tech Stack:** Rust 2024, the existing `polish-core` + `image` crates, ffmpeg/ffprobe/xdotool as external subprocesses, serde_json for output, thiserror for errors.

---

## File Structure

```
crates/
  appreels-capture/
    Cargo.toml
    src/lib.rs            # Region, CaptureError, x11grab_args, parse_xdotool_geometry, record()
  appreels-render/
    Cargo.toml
    src/lib.rs            # VideoInfo, RenderError, ffprobe/decode/encode arg builders, parse_ffprobe, frame_video()
  appreels/
    Cargo.toml            # + capture/render deps
    src/cli.rs            # + Record, Render subcommands
    src/doctor.rs         # + ffprobe in the probe list
```

Rationale: capture and render are independent subsystems with separate external-tool
surfaces; keeping them in separate crates keeps each lib focused and unit-testable. The
CLI wires them together.

---

### Task 1: appreels-capture — Region + xdotool geometry parsing

**Files:**
- Create: `crates/appreels-capture/Cargo.toml`
- Create: `crates/appreels-capture/src/lib.rs`
- Modify: `Cargo.toml` (workspace members)

- [ ] **Step 1: Add the crate to the workspace + manifest**

In root `Cargo.toml`, add to `members`: `"crates/appreels-capture"`.

`crates/appreels-capture/Cargo.toml`:
```toml
[package]
name = "appreels-capture"
version = "0.1.0"
edition.workspace = true
license.workspace = true
description = "appreels screen/window capture (ffmpeg x11grab)"

[dependencies]
thiserror = { workspace = true }
```

- [ ] **Step 2: Write the failing test**

`crates/appreels-capture/src/lib.rs`:
```rust
//! appreels screen/window capture.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_xdotool_shell_geometry() {
        let output = "WINDOW=12345\nX=100\nY=64\nWIDTH=1280\nHEIGHT=720\nSCREEN=0\n";
        let region = parse_xdotool_geometry(output).expect("geometry");
        assert_eq!(region, Region { x: 100, y: 64, width: 1280, height: 720 });
    }

    #[test]
    fn rejects_incomplete_geometry() {
        assert!(parse_xdotool_geometry("X=1\nY=2\n").is_none());
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p appreels-capture`
Expected: FAIL — `Region`, `parse_xdotool_geometry` not found.

- [ ] **Step 4: Implement Region + parser**

Prepend to `crates/appreels-capture/src/lib.rs`:
```rust
/// A rectangle on the X11 screen, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Parse the output of `xdotool getwindowgeometry --shell <id>`.
pub fn parse_xdotool_geometry(output: &str) -> Option<Region> {
    let mut x = None;
    let mut y = None;
    let mut width = None;
    let mut height = None;
    for line in output.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        match key.trim() {
            "X" => x = value.trim().parse().ok(),
            "Y" => y = value.trim().parse().ok(),
            "WIDTH" => width = value.trim().parse().ok(),
            "HEIGHT" => height = value.trim().parse().ok(),
            _ => {}
        }
    }
    Some(Region { x: x?, y: y?, width: width?, height: height? })
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p appreels-capture`
Expected: PASS (2 tests).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/appreels-capture/
git commit -m "feat(capture): region type and xdotool geometry parsing"
```

---

### Task 2: appreels-capture — x11grab arg builder + record()

**Files:**
- Modify: `crates/appreels-capture/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module:
```rust
    #[test]
    fn x11grab_args_include_geometry_and_input() {
        let region = Region { x: 10, y: 20, width: 640, height: 480 };
        let args = x11grab_args(":1", region, 30, 2.5, "/tmp/out.mp4");
        assert!(args.windows(2).any(|w| w == ["-f", "x11grab"]));
        assert!(args.windows(2).any(|w| w == ["-video_size", "640x480"]));
        assert!(args.windows(2).any(|w| w == ["-i", ":1+10,20"]));
        assert!(args.windows(2).any(|w| w == ["-t", "2.500"]));
        assert!(args.windows(2).any(|w| w == ["-framerate", "30"]));
        assert_eq!(args.last().unwrap(), "/tmp/out.mp4");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p appreels-capture`
Expected: FAIL — `x11grab_args` not found.

- [ ] **Step 3: Implement the arg builder + error type + record()**

Add to `crates/appreels-capture/src/lib.rs`:
```rust
use std::process::Command;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("failed to run `{program}`: {source}")]
    Spawn { program: String, source: std::io::Error },
    #[error("`{program}` exited with status {status}: {stderr}")]
    Failed { program: String, status: std::process::ExitStatus, stderr: String },
    #[error("could not resolve window geometry for {0:?}")]
    WindowNotFound(String),
}

/// Build the ffmpeg argument vector for an x11grab recording.
pub fn x11grab_args(display: &str, region: Region, fps: u32, seconds: f64, output: &str) -> Vec<String> {
    vec![
        "-y".to_string(),
        "-f".to_string(), "x11grab".to_string(),
        "-framerate".to_string(), fps.to_string(),
        "-video_size".to_string(), format!("{}x{}", region.width, region.height),
        "-i".to_string(), format!("{display}+{},{}", region.x, region.y),
        "-t".to_string(), format!("{seconds:.3}"),
        "-c:v".to_string(), "libx264".to_string(),
        "-preset".to_string(), "ultrafast".to_string(),
        "-pix_fmt".to_string(), "yuv420p".to_string(),
        output.to_string(),
    ]
}

/// Resolve a window's screen rect by title via xdotool.
pub fn resolve_window(title: &str) -> Result<Region, CaptureError> {
    let search = run("xdotool", &["search", "--name", title])?;
    let id = search.lines().next().ok_or_else(|| CaptureError::WindowNotFound(title.to_string()))?;
    let geom = run("xdotool", &["getwindowgeometry", "--shell", id])?;
    parse_xdotool_geometry(&geom).ok_or_else(|| CaptureError::WindowNotFound(title.to_string()))
}

/// Record a region of the X display to `output` for `seconds`, via ffmpeg x11grab.
pub fn record(display: &str, region: Region, fps: u32, seconds: f64, output: &str) -> Result<(), CaptureError> {
    let args = x11grab_args(display, region, fps, seconds, output);
    run_status("ffmpeg", &args.iter().map(String::as_str).collect::<Vec<_>>())
}

fn run(program: &str, args: &[&str]) -> Result<String, CaptureError> {
    let out = Command::new(program).args(args).output()
        .map_err(|source| CaptureError::Spawn { program: program.to_string(), source })?;
    if !out.status.success() {
        return Err(CaptureError::Failed {
            program: program.to_string(),
            status: out.status,
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn run_status(program: &str, args: &[&str]) -> Result<(), CaptureError> {
    let status = Command::new(program).args(args).status()
        .map_err(|source| CaptureError::Spawn { program: program.to_string(), source })?;
    if !status.success() {
        return Err(CaptureError::Failed { program: program.to_string(), status, stderr: String::new() });
    }
    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p appreels-capture`
Expected: PASS (3 tests).

- [ ] **Step 5: Add an `#[ignore]`d live recording test**

Add to the `tests` module (needs ffmpeg + DISPLAY; run manually):
```rust
    #[test]
    #[ignore = "needs ffmpeg and an X display"]
    fn records_a_short_clip() {
        let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
        let out = std::env::temp_dir().join("appreels-capture-test.mp4");
        let region = Region { x: 0, y: 0, width: 320, height: 240 };
        record(&display, region, 10, 1.0, out.to_str().unwrap()).expect("record");
        assert!(out.metadata().expect("file").len() > 0);
    }
```

- [ ] **Step 6: Run the live test manually to verify capture works**

Run: `cargo test -p appreels-capture -- --ignored records_a_short_clip`
Expected: PASS (writes a non-empty mp4). If it fails because the display name differs,
set `DISPLAY` appropriately and retry.

- [ ] **Step 7: Commit**

```bash
git add crates/appreels-capture/src/lib.rs
git commit -m "feat(capture): x11grab recording and window resolution"
```

---

### Task 3: appreels-render — VideoInfo + ffprobe parsing

**Files:**
- Create: `crates/appreels-render/Cargo.toml`
- Create: `crates/appreels-render/src/lib.rs`
- Modify: `Cargo.toml` (workspace members)

- [ ] **Step 1: Add the crate to the workspace + manifest**

In root `Cargo.toml`, add to `members`: `"crates/appreels-render"`.

`crates/appreels-render/Cargo.toml`:
```toml
[package]
name = "appreels-render"
version = "0.1.0"
edition.workspace = true
license.workspace = true
description = "appreels post-render: polish-core framing of recorded video"

[dependencies]
polish-core = { path = "../polish-core" }
image = { workspace = true }
thiserror = { workspace = true }
```

- [ ] **Step 2: Write the failing test**

`crates/appreels-render/src/lib.rs`:
```rust
//! appreels post-render: frame recorded video with the polish-core look.

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
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p appreels-render`
Expected: FAIL — `parse_ffprobe`, `VideoInfo` not found.

- [ ] **Step 4: Implement VideoInfo + parser**

Prepend to `crates/appreels-render/src/lib.rs`:
```rust
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
        let Some((key, value)) = line.split_once('=') else { continue };
        match key.trim() {
            "width" => width = value.trim().parse().ok(),
            "height" => height = value.trim().parse().ok(),
            "r_frame_rate" => fps = parse_frame_rate(value.trim()),
            _ => {}
        }
    }
    Some(VideoInfo { width: width?, height: height?, fps: fps? })
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
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p appreels-render`
Expected: PASS (3 tests).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/appreels-render/
git commit -m "feat(render): video info and ffprobe parsing"
```

---

### Task 4: appreels-render — decode/encode arg builders + frame_video()

**Files:**
- Modify: `crates/appreels-render/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module:
```rust
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
        assert!(args.windows(2).any(|w| w == ["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2"]));
        assert!(args.windows(2).any(|w| w == ["-pix_fmt", "yuv420p"]));
        assert_eq!(args.last().unwrap(), "out.mp4");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p appreels-render`
Expected: FAIL — `decode_args`, `encode_args` not found.

- [ ] **Step 3: Implement the arg builders, error type, and frame_video orchestration**

Add to `crates/appreels-render/src/lib.rs`:
```rust
use std::io::{Read, Write};
use std::process::{Command, Stdio};

use image::RgbaImage;
use polish_core::{PresentationStyle, compose_frame};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("failed to run `{program}`: {source}")]
    Spawn { program: String, source: std::io::Error },
    #[error("`{program}` failed: {0}", program = "ffmpeg/ffprobe")]
    Failed(String),
    #[error("could not probe input video: {0}")]
    Probe(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// ffprobe args that print width/height/r_frame_rate for the first video stream.
pub fn ffprobe_args(input: &str) -> Vec<String> {
    vec![
        "-v".into(), "error".into(),
        "-select_streams".into(), "v:0".into(),
        "-show_entries".into(), "stream=width,height,r_frame_rate".into(),
        "-of".into(), "default=noprint_wrappers=1".into(),
        input.into(),
    ]
}

/// ffmpeg args to decode `input` to a raw rgba framestream on stdout.
pub fn decode_args(input: &str) -> Vec<String> {
    vec![
        "-v".into(), "error".into(),
        "-i".into(), input.into(),
        "-f".into(), "rawvideo".into(),
        "-pix_fmt".into(), "rgba".into(),
        "-".into(),
    ]
}

/// ffmpeg args to encode a raw rgba framestream (`canvas_w`x`canvas_h` @ `fps`) on stdin to `output`.
pub fn encode_args(canvas_w: u32, canvas_h: u32, fps: f64, output: &str) -> Vec<String> {
    vec![
        "-y".into(),
        "-v".into(), "error".into(),
        "-f".into(), "rawvideo".into(),
        "-pix_fmt".into(), "rgba".into(),
        "-s".into(), format!("{canvas_w}x{canvas_h}"),
        "-r".into(), format!("{fps:.5}"),
        "-i".into(), "-".into(),
        "-vf".into(), "pad=ceil(iw/2)*2:ceil(ih/2)*2".into(),
        "-pix_fmt".into(), "yuv420p".into(),
        output.into(),
    ]
}

/// Probe a video's dimensions and frame rate.
pub fn probe(input: &str) -> Result<VideoInfo, RenderError> {
    let out = Command::new("ffprobe")
        .args(ffprobe_args(input))
        .output()
        .map_err(|source| RenderError::Spawn { program: "ffprobe".into(), source })?;
    if !out.status.success() {
        return Err(RenderError::Probe(String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    parse_ffprobe(&String::from_utf8_lossy(&out.stdout))
        .ok_or_else(|| RenderError::Probe("missing stream fields".into()))
}

/// Decode `input`, frame each frame through `compose_frame`, and re-encode to `output`.
pub fn frame_video(input: &str, output: &str, style: &PresentationStyle) -> Result<VideoInfo, RenderError> {
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
        .map_err(|source| RenderError::Spawn { program: "ffmpeg".into(), source })?;
    let mut encoder = Command::new("ffmpeg")
        .args(encode_args(canvas_w, canvas_h, info.fps, output))
        .stdin(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|source| RenderError::Spawn { program: "ffmpeg".into(), source })?;

    let mut dec_out = decoder.stdout.take().expect("decoder stdout");
    let mut enc_in = encoder.stdin.take().expect("encoder stdin");
    let frame_len = (w as usize) * (h as usize) * 4;
    let mut buf = vec![0u8; frame_len];

    loop {
        match dec_out.read_exact(&mut buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(RenderError::Io(e)),
        }
        let frame = RgbaImage::from_raw(w, h, buf.clone()).expect("frame dimensions");
        let composed = compose_frame(&frame, style);
        enc_in.write_all(composed.as_raw())?;
    }
    drop(enc_in); // signal EOF to the encoder

    let dec_status = decoder.wait().map_err(RenderError::Io)?;
    let enc_status = encoder.wait().map_err(RenderError::Io)?;
    if !dec_status.success() || !enc_status.success() {
        return Err(RenderError::Failed(format!(
            "decoder={dec_status:?} encoder={enc_status:?}"
        )));
    }
    Ok(info)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p appreels-render`
Expected: PASS (5 tests).

- [ ] **Step 5: Add an `#[ignore]`d live render test**

Add to the `tests` module (needs ffmpeg/ffprobe; run manually):
```rust
    #[test]
    #[ignore = "needs ffmpeg/ffprobe"]
    fn frames_a_generated_clip() {
        let dir = std::env::temp_dir();
        let src = dir.join("appreels-render-src.mp4");
        let out = dir.join("appreels-render-out.mp4");
        // Generate a 1s 320x240 test clip.
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-y", "-v", "error", "-f", "lavfi",
                "-i", "testsrc=duration=1:size=320x240:rate=10",
                "-pix_fmt", "yuv420p", src.to_str().unwrap(),
            ])
            .status()
            .expect("ffmpeg testsrc");
        assert!(status.success());

        let style = polish_core::style_from_seed(42);
        let info = frame_video(src.to_str().unwrap(), out.to_str().unwrap(), &style).expect("render");
        assert_eq!((info.width, info.height), (320, 240));

        // Output should be larger than the input region and valid.
        let probed = probe(out.to_str().unwrap()).expect("probe out");
        assert!(probed.width >= 320 + style.padding * 2);
    }
```

- [ ] **Step 6: Run the live render test manually**

Run: `cargo test -p appreels-render -- --ignored frames_a_generated_clip`
Expected: PASS (renders a framed mp4 and re-probes it).

- [ ] **Step 7: Commit**

```bash
git add crates/appreels-render/src/lib.rs
git commit -m "feat(render): frame-by-frame polish-core video framing"
```

---

### Task 5: appreels CLI — record + render subcommands, doctor + ffprobe

**Files:**
- Modify: `crates/appreels/Cargo.toml`
- Modify: `crates/appreels/src/cli.rs`
- Modify: `crates/appreels/src/doctor.rs`

- [ ] **Step 1: Add deps**

In `crates/appreels/Cargo.toml` `[dependencies]`, add:
```toml
appreels-capture = { path = "../appreels-capture" }
appreels-render = { path = "../appreels-render" }
polish-core = { path = "../polish-core" }
```

- [ ] **Step 2: Write the failing test (doctor includes ffprobe)**

In `crates/appreels/src/doctor.rs`, add to the `REQUIRED_TOOLS` test expectations by adding this test to its `tests` module:
```rust
    #[test]
    fn probes_ffprobe() {
        let r = report("0.1.0", |_| true);
        assert!(r.tools.iter().any(|t| t.name == "ffprobe"));
    }
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p appreels`
Expected: FAIL — no `ffprobe` entry.

- [ ] **Step 4: Add ffprobe to the probe list**

In `crates/appreels/src/doctor.rs`, add to `REQUIRED_TOOLS`:
```rust
    ("ffprobe", "video probing for render"),
```
(Place it right after the `ffmpeg` entry.)

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p appreels`
Expected: PASS.

- [ ] **Step 6: Add the record + render subcommands**

In `crates/appreels/src/cli.rs`, add variants to `enum Command`:
```rust
    /// Record a window or screen region to a raw video via ffmpeg x11grab.
    Record {
        /// Window title to capture (resolved via xdotool).
        #[arg(long, conflicts_with = "region")]
        window: Option<String>,
        /// Explicit region as "x,y,width,height".
        #[arg(long, conflicts_with = "window")]
        region: Option<String>,
        /// X display to capture.
        #[arg(long, default_value = ":0")]
        display: String,
        #[arg(long, default_value_t = 30)]
        fps: u32,
        /// Recording duration in seconds.
        #[arg(long)]
        seconds: f64,
        #[arg(long)]
        out: PathBuf,
    },
    /// Frame a recorded video with the appshots look.
    Render {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Style seed (omit for a random style).
        #[arg(long)]
        style_seed: Option<u64>,
    },
```

Re-add the `use std::path::PathBuf;` import at the top of `cli.rs`.

Add handler arms in `run`'s `match`:
```rust
        Command::Record { window, region, display, fps, seconds, out } => {
            let resolved = match (window, region) {
                (Some(title), _) => appreels_capture::resolve_window(&title)?,
                (_, Some(spec)) => parse_region(&spec)?,
                (None, None) => return Err("provide --window or --region".into()),
            };
            appreels_capture::record(&display, resolved, fps, seconds, out.to_str().unwrap())?;
            let report = serde_json::json!({
                "ok": true,
                "command": "record",
                "output": out,
                "region": { "x": resolved.x, "y": resolved.y, "width": resolved.width, "height": resolved.height },
                "fps": fps,
                "seconds": seconds,
            });
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(ExitCode::SUCCESS)
        }
        Command::Render { input, out, style_seed } => {
            let style = match style_seed {
                Some(seed) => polish_core::style_from_seed(seed),
                None => polish_core::style_from_seed(default_seed()),
            };
            let info = appreels_render::frame_video(input.to_str().unwrap(), out.to_str().unwrap(), &style)?;
            let report = serde_json::json!({
                "ok": true,
                "command": "render",
                "output": out,
                "styleSeed": style.seed,
                "palette": style.palette_name,
                "source": { "width": info.width, "height": info.height, "fps": info.fps },
            });
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(ExitCode::SUCCESS)
        }
```

Add these helpers at the bottom of `cli.rs`:
```rust
fn parse_region(spec: &str) -> Result<appreels_capture::Region, Box<dyn std::error::Error>> {
    let parts: Vec<&str> = spec.split(',').map(str::trim).collect();
    if parts.len() != 4 {
        return Err("region must be \"x,y,width,height\"".into());
    }
    Ok(appreels_capture::Region {
        x: parts[0].parse()?,
        y: parts[1].parse()?,
        width: parts[2].parse()?,
        height: parts[3].parse()?,
    })
}

// A fixed default seed keeps `render` without --style-seed deterministic.
fn default_seed() -> u64 {
    0x617070_7265656c // "appreel" bytes; arbitrary but stable
}
```

- [ ] **Step 7: Write a failing test for parse_region**

Add a test module section at the bottom of `cli.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_region_spec() {
        let r = parse_region("10, 20, 640, 480").expect("region");
        assert_eq!((r.x, r.y, r.width, r.height), (10, 20, 640, 480));
    }

    #[test]
    fn rejects_bad_region_spec() {
        assert!(parse_region("1,2,3").is_err());
    }
}
```

- [ ] **Step 8: Run tests + build**

Run: `cargo test -p appreels`
Expected: PASS (doctor tests + parse_region tests).
Run: `cargo build`
Expected: clean.

- [ ] **Step 9: Verify the CLI end-to-end manually (needs display + ffmpeg)**

Run:
```bash
cargo run -p appreels -- record --region 0,0,320,240 --display "$DISPLAY" --seconds 1 --out /tmp/raw.mp4
cargo run -p appreels -- render --input /tmp/raw.mp4 --out /tmp/framed.mp4 --style-seed 42
```
Expected: each prints an `ok: true` JSON report; `/tmp/framed.mp4` exists and is a valid,
larger-than-source video (the framed look). Open it to confirm the appshots backdrop +
rounded corners + shadow are present.

- [ ] **Step 10: Commit**

```bash
git add crates/appreels/
git commit -m "feat(cli): record and render subcommands"
```

---

### Task 6: README + workspace verification

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Document the new commands**

Add a section to `README.md`:
```markdown
## Recording (phase 2)

```bash
# Record a window for 5 seconds, then frame it with the appshots look:
appreels record --window "Firefox" --display :0 --seconds 5 --out raw.mp4
appreels render --input raw.mp4 --out demo.mp4 --style-seed 42

# Or capture an explicit region:
appreels record --region 0,0,1280,720 --display :0 --seconds 5 --out raw.mp4
```
```

- [ ] **Step 2: Verify the full gate on latest stable**

Run: `rustup update stable && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: fmt clean, clippy clean, all (non-ignored) tests pass.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: document record and render commands"
```

---

## Self-Review

- **Spec coverage (phase 2 scope):** ffmpeg x11grab capture ✓ (Tasks 1-2), window
  resolution ✓ (Task 2), frame-by-frame polish-core render ✓ (Tasks 3-4), CLI record +
  render ✓ (Task 5), doctor probes ffprobe ✓ (Task 5), docs ✓ (Task 6). Cursor track,
  zoom, captions, voiceover are later phases per the spec.
- **Placeholder scan:** none — all code is complete, including the orchestration in
  `frame_video`. Live behaviors that cannot run in CI are `#[ignore]`d with explicit run
  commands.
- **Type consistency:** `Region`/`x11grab_args`/`resolve_window`/`record` in
  appreels-capture; `VideoInfo`/`parse_ffprobe`/`probe`/`frame_video`/`ffprobe_args`/
  `decode_args`/`encode_args` in appreels-render; `compose_frame`/`style_from_seed`/
  `PresentationStyle` reused from polish-core with their existing signatures; CLI uses
  `parse_region` + `frame_video` consistently.
- **Even-dimension safety:** the encoder's `-vf pad=ceil(iw/2)*2:ceil(ih/2)*2` guarantees
  yuv420p-legal even dimensions regardless of style padding parity.
```
