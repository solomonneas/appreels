# appreels Phase 1 (Foundation) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the appreels Cargo workspace with the shared `polish-core` framing crate (extracted from appshots), the `appreels-script` demo-script format, and an `appreels` CLI skeleton with `doctor` + `schema`, all under green CI.

**Architecture:** A Rust Cargo workspace. `polish-core` is a pure, deterministic image-framing library (gradient backdrop, rounded corners, soft shadow, padding) reused from appshots' `polish.rs`, made frame-oriented (operates on in-memory `RgbaImage`). `appreels-script` defines the serde + JSON-schema demo-script types. `appreels` is the clap CLI that, in this phase, implements `doctor` (dependency probe) and `schema` (emit the script JSON schema), following appshots' JSON conventions (camelCase wire keys, `ok`/`warnings`/`errors`).

**Tech Stack:** Rust 2024, clap (derive), serde + serde_json, schemars, image (png), thiserror. CI on GitHub Actions (ubuntu-latest, latest stable, fmt + clippy `-D warnings` + test).

---

## File Structure

```
appreels/
  Cargo.toml                     # workspace manifest (members)
  LICENSE                        # Apache-2.0
  README.md
  .github/workflows/ci.yml
  crates/
    polish-core/
      Cargo.toml
      src/lib.rs                 # PresentationStyle, palettes, compose_frame, helpers
    appreels-script/
      Cargo.toml
      src/lib.rs                 # Script, Target, Step, StepTarget, schema
    appreels/
      Cargo.toml
      src/main.rs                # clap CLI entry + dispatch
      src/cli.rs                 # args + command impls (doctor, schema)
      src/doctor.rs              # dependency probe + DoctorReport
```

Rationale: each crate has one responsibility and is testable in isolation. `polish-core`
has zero appreels-specific deps so appshots can later depend on it unchanged.

---

### Task 1: Workspace + crate skeletons

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `crates/polish-core/Cargo.toml`, `crates/polish-core/src/lib.rs`
- Create: `crates/appreels-script/Cargo.toml`, `crates/appreels-script/src/lib.rs`
- Create: `crates/appreels/Cargo.toml`, `crates/appreels/src/main.rs`

- [ ] **Step 1: Create the workspace manifest**

`Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["crates/polish-core", "crates/appreels-script", "crates/appreels"]

[workspace.package]
edition = "2024"
license = "Apache-2.0"
repository = "https://github.com/solomonneas/appreels"

[workspace.dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
schemars = "0.8"
image = { version = "0.25", default-features = false, features = ["png"] }
clap = { version = "4.5", features = ["derive"] }
thiserror = "2.0"
```

- [ ] **Step 2: Create polish-core crate manifest**

`crates/polish-core/Cargo.toml`:
```toml
[package]
name = "polish-core"
version = "0.1.0"
edition.workspace = true
license.workspace = true
description = "appshots-style image framing: gradient backdrop, rounded corners, soft shadow, padding"

[dependencies]
image = { workspace = true }
```

- [ ] **Step 3: Create appreels-script crate manifest**

`crates/appreels-script/Cargo.toml`:
```toml
[package]
name = "appreels-script"
version = "0.1.0"
edition.workspace = true
license.workspace = true
description = "appreels demo-script format (serde + JSON schema)"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
schemars = { workspace = true }

[dev-dependencies]
serde_json = { workspace = true }
```

- [ ] **Step 4: Create appreels bin crate manifest**

`crates/appreels/Cargo.toml`:
```toml
[package]
name = "appreels"
version = "0.1.0"
edition.workspace = true
license.workspace = true
description = "Agent-neutral polished demo-video recorder"

[[bin]]
name = "appreels"
path = "src/main.rs"

[dependencies]
appreels-script = { path = "../appreels-script" }
clap = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
schemars = { workspace = true }
```

- [ ] **Step 5: Add placeholder lib/main so the workspace builds**

`crates/polish-core/src/lib.rs`:
```rust
//! appshots-style image framing.
```

`crates/appreels-script/src/lib.rs`:
```rust
//! appreels demo-script format.
```

`crates/appreels/src/main.rs`:
```rust
fn main() {}
```

- [ ] **Step 6: Verify the workspace builds**

Run: `cargo build`
Expected: compiles all three crates, no errors.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/
git commit -m "chore: scaffold appreels cargo workspace"
```

---

### Task 2: polish-core — palettes & style from seed

**Files:**
- Modify: `crates/polish-core/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/polish-core/src/lib.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_from_seed_is_deterministic() {
        let a = style_from_seed(12345);
        let b = style_from_seed(12345);
        assert_eq!(a.palette_name, b.palette_name);
        assert_eq!(a.padding, b.padding);
        assert_eq!(a.start, b.start);
    }

    #[test]
    fn style_uses_a_known_palette() {
        let style = style_from_seed(1);
        assert!(PALETTES.iter().any(|p| p.0 == style.palette_name));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p polish-core`
Expected: FAIL — `style_from_seed`, `PresentationStyle`, `PALETTES` not found.

- [ ] **Step 3: Implement style + palettes (deterministic, no rand dep)**

Prepend to `crates/polish-core/src/lib.rs` (above the test module):
```rust
#[derive(Debug, Clone)]
pub struct PresentationStyle {
    pub seed: u64,
    pub palette_name: String,
    pub start: [u8; 3],
    pub end: [u8; 3],
    pub accent: [u8; 3],
    pub padding: u32,
    pub corner_radius: u32,
    pub shadow_blur: f32,
    pub shadow_offset_y: i32,
}

pub const PALETTES: [(&str, [u8; 3], [u8; 3], [u8; 3]); 5] = [
    ("dusk-berry", [34, 40, 78], [178, 48, 104], [118, 79, 178]),
    ("aurora-teal", [15, 77, 87], [62, 148, 126], [165, 212, 141]),
    ("graphite-rose", [38, 42, 49], [158, 64, 91], [222, 134, 113]),
    ("indigo-copper", [31, 45, 92], [190, 104, 62], [240, 167, 92]),
    ("forest-slate", [23, 65, 55], [73, 88, 103], [129, 160, 126]),
];

// Deterministic splitmix64 so the crate needs no rng dependency.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

fn range_u32(state: &mut u64, lo: u32, hi: u32) -> u32 {
    lo + (splitmix64(state) % u64::from(hi - lo + 1)) as u32
}

pub fn style_from_seed(seed: u64) -> PresentationStyle {
    let mut state = seed;
    let idx = (splitmix64(&mut state) % PALETTES.len() as u64) as usize;
    let (name, start, end, accent) = PALETTES[idx];
    PresentationStyle {
        seed,
        palette_name: name.to_string(),
        start,
        end,
        accent,
        padding: range_u32(&mut state, 56, 88),
        corner_radius: range_u32(&mut state, 14, 22),
        shadow_blur: range_u32(&mut state, 18, 30) as f32,
        shadow_offset_y: range_u32(&mut state, 14, 28) as i32,
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p polish-core`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/polish-core/src/lib.rs
git commit -m "feat(polish-core): deterministic style + palettes"
```

---

### Task 3: polish-core — compose_frame (backdrop + rounded window + shadow)

**Files:**
- Modify: `crates/polish-core/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/polish-core/src/lib.rs`:
```rust
    use image::{Rgba, RgbaImage};

    #[test]
    fn compose_frame_pads_by_style() {
        let style = style_from_seed(42);
        let input = RgbaImage::from_pixel(100, 60, Rgba([10, 20, 30, 255]));
        let out = compose_frame(&input, &style);
        assert_eq!(out.width(), 100 + style.padding * 2);
        assert_eq!(out.height(), 60 + style.padding * 2 + style.shadow_offset_y as u32);
    }

    #[test]
    fn compose_frame_is_opaque() {
        let style = style_from_seed(7);
        let input = RgbaImage::from_pixel(40, 40, Rgba([255, 255, 255, 255]));
        let out = compose_frame(&input, &style);
        assert_eq!(out.get_pixel(0, 0).0[3], 255); // backdrop corner is opaque
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p polish-core`
Expected: FAIL — `compose_frame` not found.

- [ ] **Step 3: Implement compose_frame + helpers (adapted from appshots polish.rs)**

Add to `crates/polish-core/src/lib.rs` (above tests). These are the appshots
algorithms made in-memory and frame-oriented:
```rust
use image::{GenericImageView, ImageBuffer, Rgba, RgbaImage, imageops};

const SHADOW_ALPHA: u8 = 95;

/// Frame a single image with the appshots look: gradient backdrop, rounded
/// corners, soft shadow, padding. Pure and deterministic.
pub fn compose_frame(input: &RgbaImage, style: &PresentationStyle) -> RgbaImage {
    let window = rounded_window(input, style.corner_radius);
    let (w, h) = window.dimensions();
    let canvas_w = w + style.padding * 2;
    let canvas_h = h + style.padding * 2 + style.shadow_offset_y as u32;

    let mut canvas = backdrop(canvas_w, canvas_h, style);
    let shadow = shadow_layer(w, h, canvas_w, canvas_h, style);
    alpha_composite(&mut canvas, &shadow, 0, 0);
    alpha_composite(&mut canvas, &window, style.padding as i32, style.padding as i32);
    canvas
}

fn rounded_window(input: &RgbaImage, radius: u32) -> RgbaImage {
    let mut image = input.clone();
    let (width, height) = image.dimensions();
    for y in 0..height {
        for x in 0..width {
            let alpha = rounded_alpha(x, y, width, height, radius);
            if alpha < 255 {
                let p = image.get_pixel_mut(x, y);
                p.0[3] = ((u16::from(p.0[3]) * u16::from(alpha)) / 255) as u8;
            }
        }
    }
    image
}

fn rounded_alpha(x: u32, y: u32, width: u32, height: u32, radius: u32) -> u8 {
    if radius == 0 || width <= radius * 2 || height <= radius * 2 {
        return 255;
    }
    let cx = if x < radius { Some(radius as i32) }
        else if x >= width - radius { Some((width - radius - 1) as i32) } else { None };
    let cy = if y < radius { Some(radius as i32) }
        else if y >= height - radius { Some((height - radius - 1) as i32) } else { None };
    let (Some(cx), Some(cy)) = (cx, cy) else { return 255 };
    let dx = x as i32 - cx;
    let dy = y as i32 - cy;
    let distance = ((dx * dx + dy * dy) as f32).sqrt();
    let edge = radius as f32;
    if distance <= edge - 1.0 { 255 }
    else if distance >= edge { 0 }
    else { ((edge - distance) * 255.0).round() as u8 }
}

fn shadow_layer(win_w: u32, win_h: u32, canvas_w: u32, canvas_h: u32, style: &PresentationStyle) -> RgbaImage {
    let mut mask = RgbaImage::from_pixel(canvas_w, canvas_h, Rgba([0, 0, 0, 0]));
    let sx = style.padding as i32;
    let sy = style.padding as i32 + style.shadow_offset_y;
    for y in 0..win_h {
        for x in 0..win_w {
            let alpha = rounded_alpha(x, y, win_w, win_h, style.corner_radius);
            if alpha == 0 { continue; }
            let (tx, ty) = (sx + x as i32, sy + y as i32);
            if tx < 0 || ty < 0 { continue; }
            let (tx, ty) = (tx as u32, ty as u32);
            if tx < canvas_w && ty < canvas_h {
                let sa = ((u16::from(alpha) * u16::from(SHADOW_ALPHA)) / 255) as u8;
                mask.put_pixel(tx, ty, Rgba([0, 0, 0, sa]));
            }
        }
    }
    imageops::blur(&mask, style.shadow_blur)
}

fn backdrop(width: u32, height: u32, style: &PresentationStyle) -> RgbaImage {
    ImageBuffer::from_fn(width, height, |x, y| {
        let fx = x as f32 / width.max(1) as f32;
        let fy = y as f32 / height.max(1) as f32;
        let diagonal = fx * 0.62 + fy * 0.38;
        let radial = ((fx - 0.78).powi(2) + (fy - 0.18).powi(2)).sqrt();
        let accent_mix = (1.0 - radial * 1.6).clamp(0.0, 0.45);
        let base = mix_rgb(style.start, style.end, diagonal);
        let mixed = mix_rgb(base, style.accent, accent_mix);
        let vignette = 1.0 - (((fx - 0.5).powi(2) + (fy - 0.5).powi(2)).sqrt() * 0.18);
        Rgba([
            (f32::from(mixed[0]) * vignette).round().clamp(0.0, 255.0) as u8,
            (f32::from(mixed[1]) * vignette).round().clamp(0.0, 255.0) as u8,
            (f32::from(mixed[2]) * vignette).round().clamp(0.0, 255.0) as u8,
            255,
        ])
    })
}

fn mix_rgb(start: [u8; 3], end: [u8; 3], amount: f32) -> [u8; 3] {
    [
        lerp(f32::from(start[0]), f32::from(end[0]), amount) as u8,
        lerp(f32::from(start[1]), f32::from(end[1]), amount) as u8,
        lerp(f32::from(start[2]), f32::from(end[2]), amount) as u8,
    ]
}

fn lerp(start: f32, end: f32, amount: f32) -> f32 {
    start + (end - start) * amount.clamp(0.0, 1.0)
}

fn alpha_composite(base: &mut RgbaImage, overlay: &RgbaImage, offset_x: i32, offset_y: i32) {
    let (bw, bh) = base.dimensions();
    for y in 0..overlay.height() {
        for x in 0..overlay.width() {
            let (tx, ty) = (offset_x + x as i32, offset_y + y as i32);
            if tx < 0 || ty < 0 { continue; }
            let (tx, ty) = (tx as u32, ty as u32);
            if tx >= bw || ty >= bh { continue; }
            let src = overlay.get_pixel(x, y);
            let alpha = f32::from(src.0[3]) / 255.0;
            if alpha == 0.0 { continue; }
            let dst = base.get_pixel(tx, ty);
            let inv = 1.0 - alpha;
            base.put_pixel(tx, ty, Rgba([
                (f32::from(src.0[0]) * alpha + f32::from(dst.0[0]) * inv).round() as u8,
                (f32::from(src.0[1]) * alpha + f32::from(dst.0[1]) * inv).round() as u8,
                (f32::from(src.0[2]) * alpha + f32::from(dst.0[2]) * inv).round() as u8,
                255,
            ]));
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p polish-core`
Expected: PASS (4 tests total).

- [ ] **Step 5: Commit**

```bash
git add crates/polish-core/src/lib.rs
git commit -m "feat(polish-core): compose_frame framing pipeline"
```

---

### Task 4: appreels-script — types + serde round-trip

**Files:**
- Modify: `crates/appreels-script/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/appreels-script/src/lib.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_browser_script() {
        let json = r#"{
            "version": "0.1.0",
            "title": "Create a new project",
            "target": { "kind": "browser", "url": "https://app.example.com" },
            "steps": [
                { "type": "narrate", "text": "Hello." },
                { "type": "click", "target": { "selector": "#new" } },
                { "type": "type", "target": { "selector": "input" }, "text": "Demo" },
                { "type": "zoom", "reset": true }
            ]
        }"#;
        let script: Script = serde_json::from_str(json).expect("parse");
        assert_eq!(script.steps.len(), 4);
        let back = serde_json::to_string(&script).expect("serialize");
        let reparsed: Script = serde_json::from_str(&back).expect("reparse");
        assert_eq!(reparsed.title, "Create a new project");
    }

    #[test]
    fn target_uses_camel_case_tag() {
        let t = Target::Browser { url: "https://x".into() };
        let v = serde_json::to_value(t).unwrap();
        assert_eq!(v["kind"], "browser");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p appreels-script`
Expected: FAIL — `Script`, `Target` not found.

- [ ] **Step 3: Implement the script types**

Prepend to `crates/appreels-script/src/lib.rs`:
```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Script {
    pub version: String,
    pub title: String,
    pub target: Target,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<Viewport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defaults: Option<Defaults>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Target {
    Browser { url: String },
    Desktop {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        app: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window_title: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Viewport { pub width: u32, pub height: u32 }

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Defaults {
    #[serde(default)] pub move_ms: Option<u32>,
    #[serde(default)] pub settle_ms: Option<u32>,
    #[serde(default)] pub easing: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Step {
    Narrate { text: String },
    Caption {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")] anchor: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")] duration_ms: Option<u32>,
    },
    Move { target: StepTarget },
    Click { target: StepTarget },
    Type { target: StepTarget, text: String },
    Key { chord: String },
    Wait { ms: u32 },
    Scroll { target: StepTarget },
    Zoom {
        #[serde(default, skip_serializing_if = "Option::is_none")] to: Option<StepTarget>,
        #[serde(default, skip_serializing_if = "Option::is_none")] scale: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")] hold_ms: Option<u32>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")] reset: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum StepTarget {
    Selector(String),
    Coord { x: i32, y: i32 },
    ImageAnchor(String),
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p appreels-script`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/appreels-script/src/lib.rs
git commit -m "feat(script): demo-script serde types"
```

---

### Task 5: appreels-script — JSON schema export

**Files:**
- Modify: `crates/appreels-script/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module:
```rust
    #[test]
    fn schema_generates() {
        let schema = script_schema();
        let v = serde_json::to_value(&schema).unwrap();
        assert!(v["properties"]["steps"].is_object());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p appreels-script`
Expected: FAIL — `script_schema` not found.

- [ ] **Step 3: Implement the schema accessor**

Add near the top of `crates/appreels-script/src/lib.rs` (after imports):
```rust
/// The JSON schema for a [`Script`], for `appreels schema`.
pub fn script_schema() -> schemars::schema::RootSchema {
    schemars::schema_for!(Script)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p appreels-script`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/appreels-script/src/lib.rs
git commit -m "feat(script): JSON schema export"
```

---

### Task 6: appreels CLI — doctor dependency probe

**Files:**
- Create: `crates/appreels/src/doctor.rs`
- Create: `crates/appreels/src/cli.rs`
- Modify: `crates/appreels/src/main.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/appreels/src/doctor.rs`:
```rust
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub ok: bool,
    pub version: String,
    pub tools: Vec<ToolStatus>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub name: String,
    pub available: bool,
    pub purpose: String,
}

const REQUIRED_TOOLS: &[(&str, &str)] = &[
    ("ffmpeg", "raw capture + render"),
    ("xdotool", "real-cursor input control"),
    ("wmctrl", "window geometry"),
    ("obs-cmd", "OBS live-scene capture (optional)"),
];

pub fn report(version: &str, has_tool: impl Fn(&str) -> bool) -> DoctorReport {
    let tools: Vec<ToolStatus> = REQUIRED_TOOLS
        .iter()
        .map(|(name, purpose)| ToolStatus {
            name: name.to_string(),
            available: has_tool(name),
            purpose: purpose.to_string(),
        })
        .collect();
    let warnings: Vec<String> = tools
        .iter()
        .filter(|t| !t.available)
        .map(|t| format!("{} not found on PATH: {}", t.name, t.purpose))
        .collect();
    // ffmpeg + xdotool are the hard requirements for the v1 recorder path.
    let ok = tools.iter().filter(|t| t.name == "ffmpeg" || t.name == "xdotool").all(|t| t.available);
    DoctorReport { ok, version: version.to_string(), tools, warnings }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_requires_ffmpeg_and_xdotool() {
        let all = report("0.1.0", |_| true);
        assert!(all.ok);
        let none = report("0.1.0", |_| false);
        assert!(!none.ok);
        assert!(!none.warnings.is_empty());
        let partial = report("0.1.0", |t| t == "ffmpeg"); // missing xdotool
        assert!(!partial.ok);
    }

    #[test]
    fn report_serializes_camel_case() {
        let v = serde_json::to_value(report("0.1.0", |_| true)).unwrap();
        assert!(v["tools"][0]["available"].is_boolean());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p appreels`
Expected: FAIL — `doctor` module not wired into the crate yet.

- [ ] **Step 3: Wire the module + a PATH lookup helper**

Create `crates/appreels/src/cli.rs`:
```rust
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::doctor;

#[derive(Debug, Parser)]
#[command(name = "appreels", about = "Agent-neutral polished demo-video recorder")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Report capture/render dependency health as JSON.
    Doctor,
    /// Print the demo-script JSON schema.
    Schema {
        #[arg(long)]
        compact: bool,
    },
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run(cli: Cli) -> Result<ExitCode, Box<dyn std::error::Error>> {
    match cli.command {
        Command::Doctor => {
            let report = doctor::report(VERSION, |name| has_command(name));
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(if report.ok { ExitCode::SUCCESS } else { ExitCode::from(1) })
        }
        Command::Schema { compact } => {
            let schema = appreels_script::script_schema();
            if compact {
                println!("{}", serde_json::to_string(&schema)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&schema)?);
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn has_command(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else { return false };
    std::env::split_paths(&path).any(|dir| dir.join(program).is_file())
}
```

Replace `crates/appreels/src/main.rs`:
```rust
mod cli;
mod doctor;

use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    match cli::run(cli::Cli::parse()) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("appreels: {err}");
            ExitCode::from(1)
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p appreels`
Expected: PASS (2 tests).

- [ ] **Step 5: Verify the CLI runs**

Run: `cargo run -p appreels -- doctor`
Expected: JSON report with a `tools` array; exit 0 if ffmpeg + xdotool are present.
Run: `cargo run -p appreels -- schema --compact`
Expected: one-line JSON schema containing `"steps"`.

- [ ] **Step 6: Commit**

```bash
git add crates/appreels/src/
git commit -m "feat(cli): doctor dependency probe + schema command"
```

---

### Task 7: CI + LICENSE + README

**Files:**
- Create: `.github/workflows/ci.yml`
- Create: `LICENSE`
- Create: `README.md`

- [ ] **Step 1: Add the CI workflow**

`.github/workflows/ci.yml`:
```yaml
name: CI

on:
  push:
    branches: [master]
  pull_request:
  workflow_dispatch:

concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

jobs:
  check:
    name: Check
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - run: cargo fmt --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test
```

- [ ] **Step 2: Add the Apache-2.0 LICENSE**

Copy the standard Apache-2.0 license text into `LICENSE` with `Copyright 2026 Solomon Neas`
(identical to the appshots `LICENSE` file).

- [ ] **Step 3: Add a README**

`README.md`:
```markdown
# appreels

`appreels` is the video sibling of [appshots](https://github.com/solomonneas/appshots):
an agent-neutral CLI that produces polished demo videos. An LLM can drive a browser or
desktop app to perform a demo, which is recorded and wrapped in the same clean framed
look appshots gives screenshots, with cursor emphasis, auto zoom, captions, and optional
voiceover.

Status: early development. See `docs/superpowers/specs/` for the design and
`docs/superpowers/plans/` for implementation plans.

## Commands (phase 1)

```bash
appreels doctor          # report capture/render dependency health as JSON
appreels schema          # print the demo-script JSON schema
```

## License

Apache-2.0.
```

- [ ] **Step 4: Verify the full gate locally (on latest stable)**

Run: `rustup update stable && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: fmt clean, clippy clean, all tests pass.

- [ ] **Step 5: Commit**

```bash
git add .github LICENSE README.md
git commit -m "ci: add workflow, license, and readme"
```

---

## Self-Review

- **Spec coverage (phase 1 scope):** workspace ✓ (Task 1), polish-core extraction ✓
  (Tasks 2-3), demo-script format + schema ✓ (Tasks 4-5), CLI doctor + schema ✓ (Task 6),
  CI conventions incl. latest-stable clippy lesson ✓ (Task 7). Capture/render/driver/voice/
  agent are explicitly later phases, each to get their own plan.
- **Placeholder scan:** none — every code step shows complete code; the only "copy the
  standard Apache-2.0 text" step references the existing appshots LICENSE verbatim.
- **Type consistency:** `PresentationStyle`/`compose_frame`/`PALETTES`/`style_from_seed`
  in polish-core; `Script`/`Target`/`Step`/`StepTarget`/`script_schema` in appreels-script;
  `DoctorReport`/`ToolStatus`/`report` in appreels — all referenced consistently across
  tasks.
```
