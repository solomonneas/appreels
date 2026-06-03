# appreels Phase 3 (Effects) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn `appreels render` into an effects pipeline (title/outro cards, lower-third captions, eased zoom/pan, cursor accent ring) driven by a sidecar cue file + CLI flags, and give `appreels record` a cursor-track poller.

**Architecture:** Pure-Rust per-frame compositing. ffmpeg only decodes to raw RGBA and re-encodes (unchanged). New `appreels-render` modules (`timeline`, `text`, `cards`, `effects`) provide pure, unit-tested cue parsing and pixel operations; `render_video` orchestrates them per frame. `appreels-capture` polls `xdotool getmouselocation` during recording to write a `cursor.jsonl` track. One `polish-core` addition (`gradient_backdrop`) lets cards reuse the exact backdrop.

**Tech Stack:** Rust 2024, `image` 0.25 (`imageops`), `ab_glyph` 0.2 (glyph rasterization, bundled DejaVu Sans Bold), `serde`/`serde_json` (cue + track parsing), existing `polish-core`. ffmpeg/ffprobe/xdotool as subprocesses.

---

## File Structure

```
crates/
  polish-core/src/lib.rs              # + pub fn gradient_backdrop()
  appreels-render/
    Cargo.toml                        # + serde, serde_json, ab_glyph
    assets/DejaVuSans-Bold.ttf        # bundled font (include_bytes!)
    assets/DejaVuSans-Bold.LICENSE.txt
    src/lib.rs                        # + mod decls, render_video, card_frame_count, frame_video wrapper
    src/timeline.rs                   # Timeline/Card/Caption/ZoomCue/CursorSample, parse, merge, interpolation
    src/text.rs                       # font(), text_width, draw_text, draw_text_centered, blend_pixel
    src/cards.rs                      # render_card()
    src/effects.rs                    # apply_zoom + ZoomTransform, draw_cursor_ring, draw_caption
  appreels-capture/src/lib.rs         # + parse_mouse_location, record_with_cursor
  appreels/src/cli.rs                 # + render cue/flag plumbing, record --cursor-track
```

Rationale: the render crate is one ~320-line file today; phase 3 adds substantial pixel
and parsing logic, so it splits by responsibility (cue model vs text vs cards vs effects).
Each new module is pure and independently testable; `lib.rs` keeps the ffmpeg orchestration.

---

### Task 1: polish-core — extract `gradient_backdrop`

**Files:**
- Modify: `crates/polish-core/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/polish-core/src/lib.rs`:
```rust
    #[test]
    fn gradient_backdrop_is_sized_and_opaque() {
        let style = style_from_seed(42);
        let bg = gradient_backdrop(120, 80, &style);
        assert_eq!(bg.dimensions(), (120, 80));
        assert_eq!(bg.get_pixel(0, 0).0[3], 255);
        assert_eq!(bg.get_pixel(119, 79).0[3], 255);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p polish-core gradient_backdrop_is_sized_and_opaque`
Expected: FAIL — `gradient_backdrop` not found.

- [ ] **Step 3: Make `backdrop` public and rename to `gradient_backdrop`**

In `crates/polish-core/src/lib.rs`, change the private `backdrop` fn signature to a public
`gradient_backdrop` (body unchanged):
```rust
/// Render the appshots gradient backdrop at the given size. Pure, opaque.
pub fn gradient_backdrop(width: u32, height: u32, style: &PresentationStyle) -> RgbaImage {
```
Then update the one caller inside `compose_frame`:
```rust
    let mut canvas = gradient_backdrop(canvas_w, canvas_h, style);
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p polish-core`
Expected: PASS (existing tests + the new one).

- [ ] **Step 5: Commit**

```bash
git add crates/polish-core/src/lib.rs
git commit -m "feat(polish-core): expose gradient_backdrop for reuse"
```

---

### Task 2: appreels-render — deps + timeline cue model, parsing, cursor interpolation

**Files:**
- Modify: `crates/appreels-render/Cargo.toml`
- Create: `crates/appreels-render/src/timeline.rs`
- Modify: `crates/appreels-render/src/lib.rs`

- [ ] **Step 1: Add dependencies**

In `crates/appreels-render/Cargo.toml`, under `[dependencies]`, add:
```toml
serde = { workspace = true }
serde_json = { workspace = true }
ab_glyph = "0.2"
```

- [ ] **Step 2: Write the failing test**

Create `crates/appreels-render/src/timeline.rs`:
```rust
//! appreels render cue model: cards, captions, zooms, cursor track.

use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_json_round_trips() {
        let json = r#"{
            "cursorTrack": "raw.cursor.jsonl",
            "titleCard": { "text": "Hi", "ms": 1500 },
            "captions": [ { "startMs": 0, "endMs": 1800, "text": "Open the menu" } ],
            "zooms": [ { "startMs": 2000, "endMs": 5000, "x": 420, "y": 300, "scale": 1.8 } ]
        }"#;
        let tl = Timeline::from_json(json).expect("parse");
        assert_eq!(tl.cursor_track.as_deref(), Some("raw.cursor.jsonl"));
        assert_eq!(tl.title_card.as_ref().unwrap().ms, 1500);
        assert_eq!(tl.captions[0].text, "Open the menu");
        assert_eq!(tl.zooms[0].scale, 1.8);
        assert!(tl.outro_card.is_none());
    }

    #[test]
    fn empty_timeline_is_default() {
        let tl = Timeline::from_json("{}").expect("parse");
        assert!(tl.captions.is_empty());
        assert!(tl.zooms.is_empty());
        assert!(tl.title_card.is_none());
    }

    #[test]
    fn parses_cursor_track_skipping_blank_lines() {
        let s = "{\"tMs\":0,\"x\":10,\"y\":20}\n\n{\"tMs\":100,\"x\":30,\"y\":40}\n";
        let samples = parse_cursor_track(s);
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[1].t_ms, 100);
        assert_eq!(samples[1].x, 30.0);
    }

    #[test]
    fn cursor_at_interpolates_between_samples() {
        let samples = vec![
            CursorSample { t_ms: 0, x: 0.0, y: 0.0 },
            CursorSample { t_ms: 100, x: 100.0, y: 200.0 },
        ];
        let (x, y) = cursor_at(&samples, 50.0).expect("interp");
        assert!((x - 50.0).abs() < 1e-6);
        assert!((y - 100.0).abs() < 1e-6);
    }

    #[test]
    fn cursor_at_clamps_past_the_ends() {
        let samples = vec![
            CursorSample { t_ms: 10, x: 5.0, y: 5.0 },
            CursorSample { t_ms: 20, x: 9.0, y: 9.0 },
        ];
        assert_eq!(cursor_at(&samples, 0.0), Some((5.0, 5.0)));
        assert_eq!(cursor_at(&samples, 999.0), Some((9.0, 9.0)));
        assert_eq!(cursor_at(&[], 5.0), None);
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p appreels-render`
Expected: FAIL — `Timeline`, `parse_cursor_track`, `cursor_at`, `CursorSample` not found.

- [ ] **Step 4: Implement the cue types + parsing + interpolation**

Prepend to `crates/appreels-render/src/timeline.rs` (above the `#[cfg(test)]` module):
```rust
/// The full set of render cues for one clip.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Timeline {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_track: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_card: Option<Card>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outro_card: Option<Card>,
    #[serde(default)]
    pub captions: Vec<Caption>,
    #[serde(default)]
    pub zooms: Vec<ZoomCue>,
}

/// A full-canvas title or outro card.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub text: String,
    pub ms: u32,
}

/// A lower-third caption active over `[start_ms, end_ms)`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Caption {
    pub start_ms: u32,
    pub end_ms: u32,
    pub text: String,
}

/// A zoom toward `(x, y)` (source-window px) at `scale`, eased in/hold/out.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ZoomCue {
    pub start_ms: u32,
    pub end_ms: u32,
    pub x: f64,
    pub y: f64,
    pub scale: f64,
}

/// A single pointer sample, window-relative px, `t_ms` from capture start.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CursorSample {
    pub t_ms: u32,
    pub x: f64,
    pub y: f64,
}

impl Timeline {
    /// Parse a cue file's JSON.
    pub fn from_json(s: &str) -> Result<Timeline, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// Parse a `cursor.jsonl` track (one `CursorSample` JSON object per line).
/// Blank and unparseable lines are skipped.
pub fn parse_cursor_track(s: &str) -> Vec<CursorSample> {
    s.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<CursorSample>(l).ok())
        .collect()
}

/// Linearly interpolate the cursor position at `t_ms`. Clamps to the first/last
/// sample outside the track's range. `None` if there are no samples.
/// Assumes `samples` is sorted ascending by `t_ms` (as written by capture).
pub fn cursor_at(samples: &[CursorSample], t_ms: f64) -> Option<(f64, f64)> {
    let first = samples.first()?;
    if t_ms <= first.t_ms as f64 {
        return Some((first.x, first.y));
    }
    let last = samples.last()?;
    if t_ms >= last.t_ms as f64 {
        return Some((last.x, last.y));
    }
    for w in samples.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        let (ta, tb) = (a.t_ms as f64, b.t_ms as f64);
        if t_ms >= ta && t_ms <= tb {
            let f = if tb > ta { (t_ms - ta) / (tb - ta) } else { 0.0 };
            return Some((a.x + (b.x - a.x) * f, a.y + (b.y - a.y) * f));
        }
    }
    None
}
```

- [ ] **Step 5: Declare the module + re-export from lib.rs**

At the top of `crates/appreels-render/src/lib.rs` (just under the `//!` doc comment), add:
```rust
mod timeline;

pub use timeline::{Caption, Card, CursorSample, Timeline, ZoomCue, cursor_at, parse_cursor_track};
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p appreels-render`
Expected: PASS (existing + 5 new timeline tests).

- [ ] **Step 7: Commit**

```bash
git add crates/appreels-render/Cargo.toml crates/appreels-render/src/timeline.rs crates/appreels-render/src/lib.rs
git commit -m "feat(render): timeline cue model, parsing, and cursor interpolation"
```

---

### Task 3: appreels-render — caption + zoom lookup with easing

**Files:**
- Modify: `crates/appreels-render/src/timeline.rs`
- Modify: `crates/appreels-render/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/appreels-render/src/timeline.rs`:
```rust
    #[test]
    fn caption_at_selects_the_active_caption() {
        let caps = vec![
            Caption { start_ms: 0, end_ms: 1000, text: "a".into() },
            Caption { start_ms: 1000, end_ms: 2000, text: "b".into() },
        ];
        assert_eq!(caption_at(&caps, 500.0).unwrap().text, "a");
        assert_eq!(caption_at(&caps, 1000.0).unwrap().text, "b"); // end is exclusive
        assert!(caption_at(&caps, 5000.0).is_none());
    }

    #[test]
    fn zoom_at_ramps_up_holds_and_ramps_down() {
        let zooms = vec![ZoomCue { start_ms: 0, end_ms: 1000, x: 50.0, y: 60.0, scale: 2.0 }];
        // Middle (hold): full scale.
        let mid = zoom_at(&zooms, 500.0).unwrap();
        assert!((mid.scale - 2.0).abs() < 1e-6);
        assert_eq!((mid.cx, mid.cy), (50.0, 60.0));
        // Very start: barely zoomed (close to 1.0).
        let start = zoom_at(&zooms, 1.0).unwrap();
        assert!(start.scale >= 1.0 && start.scale < 1.2);
        // Outside the cue: none.
        assert!(zoom_at(&zooms, 2000.0).is_none());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p appreels-render`
Expected: FAIL — `caption_at`, `zoom_at`, `ZoomState` not found.

- [ ] **Step 3: Implement caption/zoom lookup**

Add to `crates/appreels-render/src/timeline.rs` (above the `#[cfg(test)]` module):
```rust
/// The active caption at `t_ms`, if any. `end_ms` is exclusive.
pub fn caption_at(captions: &[Caption], t_ms: f64) -> Option<&Caption> {
    captions
        .iter()
        .find(|c| t_ms >= c.start_ms as f64 && t_ms < c.end_ms as f64)
}

/// The active zoom transform at a moment: center (source px) + eased scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomState {
    pub cx: f64,
    pub cy: f64,
    pub scale: f64,
}

/// The eased zoom state at `t_ms`. Overlapping cues: last one wins. Scale ramps
/// up over the first 30% of the cue, holds, and ramps down over the last 30%.
pub fn zoom_at(zooms: &[ZoomCue], t_ms: f64) -> Option<ZoomState> {
    let cue = zooms
        .iter()
        .rev()
        .find(|z| t_ms >= z.start_ms as f64 && t_ms < z.end_ms as f64)?;
    let span = (cue.end_ms.saturating_sub(cue.start_ms)).max(1) as f64;
    let p = ((t_ms - cue.start_ms as f64) / span).clamp(0.0, 1.0);
    let ramp = 0.3_f64;
    let factor = if p < ramp {
        ease_in_out(p / ramp)
    } else if p > 1.0 - ramp {
        ease_in_out((1.0 - p) / ramp)
    } else {
        1.0
    };
    Some(ZoomState {
        cx: cue.x,
        cy: cue.y,
        scale: 1.0 + (cue.scale - 1.0) * factor,
    })
}

fn ease_in_out(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        2.0 * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
    }
}
```

- [ ] **Step 4: Re-export the new items**

In `crates/appreels-render/src/lib.rs`, extend the timeline `pub use` line to:
```rust
pub use timeline::{
    Caption, Card, CursorSample, Timeline, ZoomCue, ZoomState, caption_at, cursor_at,
    parse_cursor_track, zoom_at,
};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p appreels-render`
Expected: PASS (caption + zoom tests included).

- [ ] **Step 6: Commit**

```bash
git add crates/appreels-render/src/timeline.rs crates/appreels-render/src/lib.rs
git commit -m "feat(render): caption lookup and eased zoom state"
```

---

### Task 4: appreels-render — bundled font + text drawing

**Files:**
- Create: `crates/appreels-render/assets/DejaVuSans-Bold.ttf` (copied from the system)
- Create: `crates/appreels-render/assets/DejaVuSans-Bold.LICENSE.txt`
- Create: `crates/appreels-render/src/text.rs`
- Modify: `crates/appreels-render/src/lib.rs`

- [ ] **Step 1: Bundle the font + its license**

```bash
mkdir -p crates/appreels-render/assets
cp /usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf crates/appreels-render/assets/DejaVuSans-Bold.ttf
```

Create `crates/appreels-render/assets/DejaVuSans-Bold.LICENSE.txt`:
```text
DejaVu Sans Bold is bundled for caption and title-card text rendering.

DejaVu fonts are based on the Bitstream Vera fonts and are released under a
permissive license. Fonts are (c) 2003 Bitstream, Inc. (Bitstream Vera) and
the DejaVu changes are in the public domain. "Bitstream Vera" and "DejaVu" are
trademarks of their respective owners.

Bitstream Vera Fonts Copyright: Copyright (c) 2003 by Bitstream, Inc. All
Rights Reserved. Permission is hereby granted, free of charge, to any person
obtaining a copy of the fonts accompanying this license ("Fonts") and
associated documentation files (the "Font Software"), to reproduce and
distribute the Font Software, including without limitation the rights to use,
copy, merge, publish, distribute, and/or sell copies of the Font Software,
subject to the conditions in the full license text:
https://dejavu-fonts.github.io/License.html
```

- [ ] **Step 2: Write the failing test**

Create `crates/appreels-render/src/text.rs`:
```rust
//! Text rendering for captions and cards, using a bundled font.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use image::RgbaImage;

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn font_loads() {
        let _ = font();
    }

    #[test]
    fn text_width_grows_with_length() {
        let f = font();
        let one = text_width(&f, "W", 32.0);
        let two = text_width(&f, "WW", 32.0);
        assert!(two > one);
        assert!(one > 0.0);
    }

    #[test]
    fn draw_text_writes_pixels() {
        let f = font();
        let mut img = RgbaImage::from_pixel(200, 60, Rgba([0, 0, 0, 255]));
        draw_text(&mut img, &f, "Hi", 40.0, 10.0, 5.0, [255, 255, 255]);
        let any_lit = img.pixels().any(|p| p.0[0] > 100);
        assert!(any_lit, "expected some white text pixels");
    }

    #[test]
    fn draw_text_centered_stays_in_bounds() {
        let f = font();
        let mut img = RgbaImage::from_pixel(300, 80, Rgba([0, 0, 0, 255]));
        draw_text_centered(&mut img, &f, "Centered", 36.0, 20.0, [255, 255, 255]);
        assert!(img.pixels().any(|p| p.0[0] > 100));
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p appreels-render`
Expected: FAIL — `font`, `text_width`, `draw_text`, `draw_text_centered` not found.

- [ ] **Step 4: Implement font loading + drawing**

Prepend to `crates/appreels-render/src/text.rs` (above the `#[cfg(test)]` module):
```rust
const FONT_BYTES: &[u8] = include_bytes!("../assets/DejaVuSans-Bold.ttf");

/// The bundled bold font.
pub fn font() -> FontRef<'static> {
    FontRef::try_from_slice(FONT_BYTES).expect("bundled font is valid")
}

/// Width in px of `text` rendered at `px` scale (includes kerning).
pub fn text_width(font: &FontRef<'static>, text: &str, px: f32) -> f32 {
    let scaled = font.as_scaled(PxScale::from(px));
    let mut width = 0.0;
    let mut prev = None;
    for c in text.chars() {
        let gid = font.glyph_id(c);
        if let Some(p) = prev {
            width += scaled.kern(p, gid);
        }
        width += scaled.h_advance(gid);
        prev = Some(gid);
    }
    width
}

/// Alpha-composite `color` onto a pixel at `coverage` (0..1). Keeps it opaque.
pub(crate) fn blend_pixel(img: &mut RgbaImage, x: u32, y: u32, color: [u8; 3], coverage: f32) {
    let a = coverage.clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    let p = img.get_pixel_mut(x, y);
    for i in 0..3 {
        p.0[i] = (f32::from(color[i]) * a + f32::from(p.0[i]) * (1.0 - a)).round() as u8;
    }
    p.0[3] = 255;
}

/// Draw `text` with its top-left at `(x, y)` in `color`.
pub fn draw_text(
    img: &mut RgbaImage,
    font: &FontRef<'static>,
    text: &str,
    px: f32,
    x: f32,
    y: f32,
    color: [u8; 3],
) {
    let scaled = font.as_scaled(PxScale::from(px));
    let ascent = scaled.ascent();
    let (iw, ih) = img.dimensions();
    let mut caret = x;
    let mut prev = None;
    for c in text.chars() {
        let gid = font.glyph_id(c);
        if let Some(p) = prev {
            caret += scaled.kern(p, gid);
        }
        let glyph = gid.with_scale_and_position(PxScale::from(px), point(caret, y + ascent));
        if let Some(outline) = font.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, cov| {
                let px_x = bounds.min.x as i32 + gx as i32;
                let px_y = bounds.min.y as i32 + gy as i32;
                if px_x < 0 || px_y < 0 {
                    return;
                }
                let (px_x, px_y) = (px_x as u32, px_y as u32);
                if px_x < iw && px_y < ih {
                    blend_pixel(img, px_x, px_y, color, cov);
                }
            });
        }
        caret += scaled.h_advance(gid);
        prev = Some(gid);
    }
}

/// Draw `text` horizontally centered, with its top at `y`.
pub fn draw_text_centered(
    img: &mut RgbaImage,
    font: &FontRef<'static>,
    text: &str,
    px: f32,
    y: f32,
    color: [u8; 3],
) {
    let width = text_width(font, text, px);
    let x = (img.width() as f32 - width) / 2.0;
    draw_text(img, font, text, px, x.max(0.0), y, color);
}
```

- [ ] **Step 5: Declare the module in lib.rs**

In `crates/appreels-render/src/lib.rs`, under the `mod timeline;` line, add:
```rust
mod text;
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p appreels-render`
Expected: PASS (4 text tests).

- [ ] **Step 7: Commit**

```bash
git add crates/appreels-render/assets crates/appreels-render/src/text.rs crates/appreels-render/src/lib.rs
git commit -m "feat(render): bundled font and text drawing"
```

---

### Task 5: appreels-render — title/outro cards

**Files:**
- Create: `crates/appreels-render/src/cards.rs`
- Modify: `crates/appreels-render/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/appreels-render/src/cards.rs`:
```rust
//! Full-canvas title and outro cards.

use image::RgbaImage;
use polish_core::PresentationStyle;

use crate::text::{draw_text_centered, font};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_is_canvas_sized_and_has_content() {
        let style = polish_core::style_from_seed(42);
        let card = render_card(400, 300, "Create a project", &style);
        assert_eq!(card.dimensions(), (400, 300));
        // Opaque backdrop.
        assert_eq!(card.get_pixel(0, 0).0[3], 255);
        // The text band near the vertical center should differ from the bare
        // backdrop at the same row near the edge.
        let mid_y = 150;
        let edge = card.get_pixel(2, mid_y).0;
        let has_text = (0..400).any(|x| card.get_pixel(x, mid_y).0 != edge);
        assert!(has_text, "expected text pixels in the center band");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p appreels-render`
Expected: FAIL — `render_card` not found.

- [ ] **Step 3: Implement `render_card`**

Prepend to `crates/appreels-render/src/cards.rs` (above the `#[cfg(test)]` module):
```rust
/// Render a full-canvas card: gradient backdrop + centered bold title text.
pub fn render_card(width: u32, height: u32, text: &str, style: &PresentationStyle) -> RgbaImage {
    let mut canvas = polish_core::gradient_backdrop(width, height, style);
    let f = font();
    let px = (width as f32 * 0.06).clamp(24.0, 96.0);
    let y = height as f32 / 2.0 - px / 2.0;
    draw_text_centered(&mut canvas, &f, text, px, y, [245, 245, 245]);
    canvas
}
```

- [ ] **Step 4: Declare the module + re-export**

In `crates/appreels-render/src/lib.rs`, add under the `mod text;` line:
```rust
mod cards;

pub use cards::render_card;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p appreels-render`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/appreels-render/src/cards.rs crates/appreels-render/src/lib.rs
git commit -m "feat(render): title/outro card frames"
```

---

### Task 6: appreels-render — zoom/pan effect

**Files:**
- Create: `crates/appreels-render/src/effects.rs`
- Modify: `crates/appreels-render/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/appreels-render/src/effects.rs`:
```rust
//! Per-frame pixel effects: zoom/pan, cursor ring, caption bar.

use ab_glyph::FontRef;
use image::{RgbaImage, imageops};

use crate::text::{blend_pixel, draw_text};
use crate::timeline::ZoomState;

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn identity_zoom_is_a_noop() {
        let img = RgbaImage::from_pixel(20, 16, Rgba([1, 2, 3, 255]));
        let (out, xf) = apply_zoom(&img, ZoomState { cx: 10.0, cy: 8.0, scale: 1.0 });
        assert_eq!(out.dimensions(), (20, 16));
        assert_eq!((xf.sx, xf.sy), (1.0, 1.0));
        assert_eq!(xf.map(5.0, 4.0), (5.0, 4.0));
    }

    #[test]
    fn zoom_preserves_size_and_magnifies() {
        // Left half red, right half blue.
        let mut img = RgbaImage::from_pixel(40, 40, Rgba([255, 0, 0, 255]));
        for y in 0..40 {
            for x in 20..40 {
                img.put_pixel(x, y, Rgba([0, 0, 255, 255]));
            }
        }
        let (out, xf) = apply_zoom(&img, ZoomState { cx: 30.0, cy: 20.0, scale: 2.0 });
        assert_eq!(out.dimensions(), (40, 40));
        assert!(xf.sx > 1.0);
        // Centered on the blue half, the output should be predominantly blue.
        assert!(out.get_pixel(20, 20).0[2] > 200);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p appreels-render`
Expected: FAIL — `apply_zoom`, `ZoomTransform` not found.

- [ ] **Step 3: Implement `apply_zoom` + `ZoomTransform`**

Prepend to `crates/appreels-render/src/effects.rs` (above the `#[cfg(test)]` module):
```rust
/// Maps a source-frame point into the zoomed image produced by [`apply_zoom`].
#[derive(Debug, Clone, Copy)]
pub struct ZoomTransform {
    pub left: f64,
    pub top: f64,
    pub sx: f64,
    pub sy: f64,
}

impl ZoomTransform {
    pub fn identity() -> Self {
        ZoomTransform { left: 0.0, top: 0.0, sx: 1.0, sy: 1.0 }
    }

    /// Map `(x, y)` from source-frame px to zoomed-image px.
    pub fn map(&self, x: f64, y: f64) -> (f64, f64) {
        ((x - self.left) * self.sx, (y - self.top) * self.sy)
    }
}

/// Crop a sub-rect centered on the zoom and scale it back to the frame size, so
/// the framed window outline stays constant while content magnifies. A no-op
/// (identity transform) when `scale <= 1.0`. The crop is clamped to the frame.
pub fn apply_zoom(frame: &RgbaImage, zoom: ZoomState) -> (RgbaImage, ZoomTransform) {
    let (w, h) = frame.dimensions();
    if zoom.scale <= 1.0 {
        return (frame.clone(), ZoomTransform::identity());
    }
    let crop_w = (f64::from(w) / zoom.scale).round().clamp(1.0, f64::from(w));
    let crop_h = (f64::from(h) / zoom.scale).round().clamp(1.0, f64::from(h));
    let left = (zoom.cx - crop_w / 2.0)
        .round()
        .clamp(0.0, f64::from(w) - crop_w);
    let top = (zoom.cy - crop_h / 2.0)
        .round()
        .clamp(0.0, f64::from(h) - crop_h);
    let sub = imageops::crop_imm(frame, left as u32, top as u32, crop_w as u32, crop_h as u32)
        .to_image();
    let scaled = imageops::resize(&sub, w, h, imageops::FilterType::Triangle);
    (
        scaled,
        ZoomTransform {
            left,
            top,
            sx: f64::from(w) / crop_w,
            sy: f64::from(h) / crop_h,
        },
    )
}
```

- [ ] **Step 4: Declare the module + re-export**

In `crates/appreels-render/src/lib.rs`, add under the `mod cards;` line:
```rust
mod effects;

pub use effects::{ZoomTransform, apply_zoom};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p appreels-render`
Expected: PASS (2 zoom tests).

- [ ] **Step 6: Commit**

```bash
git add crates/appreels-render/src/effects.rs crates/appreels-render/src/lib.rs
git commit -m "feat(render): eased zoom/pan effect"
```

---

### Task 7: appreels-render — cursor accent ring

**Files:**
- Modify: `crates/appreels-render/src/effects.rs`
- Modify: `crates/appreels-render/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/appreels-render/src/effects.rs`:
```rust
    #[test]
    fn cursor_ring_draws_an_accent_circle() {
        let mut img = RgbaImage::from_pixel(80, 80, Rgba([0, 0, 0, 255]));
        draw_cursor_ring(&mut img, 40.0, 40.0, [240, 167, 92]);
        // Center stays dark (the ring is hollow).
        assert_eq!(img.get_pixel(40, 40).0[0], 0);
        // A point ~14px to the right (on the ring) is tinted with the accent.
        let on_ring = img.get_pixel(54, 40).0;
        assert!(on_ring[0] > 100 && on_ring[1] > 60, "expected accent on the ring");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p appreels-render`
Expected: FAIL — `draw_cursor_ring` not found.

- [ ] **Step 3: Implement `draw_cursor_ring`**

Add to `crates/appreels-render/src/effects.rs` (above the `#[cfg(test)]` module):
```rust
const RING_RADIUS: f64 = 14.0;
const RING_THICKNESS: f64 = 3.0;

/// Draw a soft accent ring centered at `(cx, cy)` in image space. Constant size.
pub fn draw_cursor_ring(img: &mut RgbaImage, cx: f64, cy: f64, accent: [u8; 3]) {
    let (w, h) = img.dimensions();
    let reach = RING_RADIUS + RING_THICKNESS;
    let x0 = (cx - reach).floor().max(0.0) as u32;
    let y0 = (cy - reach).floor().max(0.0) as u32;
    let x1 = ((cx + reach).ceil().max(0.0) as u32).min(w);
    let y1 = ((cy + reach).ceil().max(0.0) as u32).min(h);
    for y in y0..y1 {
        for x in x0..x1 {
            let dist = ((f64::from(x) - cx).powi(2) + (f64::from(y) - cy).powi(2)).sqrt();
            let edge = (dist - RING_RADIUS).abs();
            if edge <= RING_THICKNESS {
                let coverage = (1.0 - edge / RING_THICKNESS).clamp(0.0, 1.0);
                blend_pixel(img, x, y, accent, coverage as f32);
            }
        }
    }
}
```

- [ ] **Step 4: Re-export**

In `crates/appreels-render/src/lib.rs`, extend the effects `pub use` line:
```rust
pub use effects::{ZoomTransform, apply_zoom, draw_cursor_ring};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p appreels-render`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/appreels-render/src/effects.rs crates/appreels-render/src/lib.rs
git commit -m "feat(render): cursor accent ring"
```

---

### Task 8: appreels-render — caption bar

**Files:**
- Modify: `crates/appreels-render/src/effects.rs`
- Modify: `crates/appreels-render/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/appreels-render/src/effects.rs`:
```rust
    #[test]
    fn caption_bar_darkens_lower_third_and_draws_keyline() {
        let f = crate::text::font();
        let mut img = RgbaImage::from_pixel(320, 240, Rgba([255, 255, 255, 255]));
        draw_caption(&mut img, &f, "Open the menu", [240, 167, 92]);
        // Top of the frame is untouched (still white).
        assert_eq!(img.get_pixel(10, 10).0, [255, 255, 255, 255]);
        // Somewhere in the lower third the scrim darkened the background.
        let darkened = (160..230)
            .any(|y| img.get_pixel(160, y).0[0] < 200);
        assert!(darkened, "expected a darkened caption band in the lower third");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p appreels-render`
Expected: FAIL — `draw_caption` not found.

- [ ] **Step 3: Implement `draw_caption`**

Add to `crates/appreels-render/src/effects.rs` (above the `#[cfg(test)]` module):
```rust
/// Draw the default "clean" lower-third caption: gradient scrim, left accent
/// keyline, bold text. Drawn onto the framed canvas.
pub fn draw_caption(img: &mut RgbaImage, font: &FontRef<'static>, text: &str, accent: [u8; 3]) {
    let (w, h) = img.dimensions();
    let bar_h = ((h as f32) * 0.14) as u32;
    let margin = ((h as f32) * 0.06) as u32;
    let bar_top = h.saturating_sub(bar_h + margin);
    let bar_bottom = (bar_top + bar_h).min(h);

    // Gradient scrim, strongest in the middle of the bar.
    for y in bar_top..bar_bottom {
        let fy = (y - bar_top) as f32 / bar_h.max(1) as f32;
        let alpha = (0.80 * (1.0 - (fy - 0.5).abs() * 0.5)).clamp(0.0, 0.85);
        for x in 0..w {
            blend_pixel(img, x, y, [12, 14, 20], alpha);
        }
    }

    // Left accent keyline.
    let key_x = ((w as f32) * 0.08) as u32;
    for y in bar_top..bar_bottom {
        for x in key_x..(key_x + 5).min(w) {
            blend_pixel(img, x, y, accent, 1.0);
        }
    }

    // Caption text, vertically centered in the bar.
    let px = ((bar_h as f32) * 0.42).clamp(18.0, 60.0);
    let text_y = bar_top as f32 + (bar_h as f32 - px) / 2.0;
    draw_text(img, font, text, px, (key_x + 16) as f32, text_y, [240, 240, 240]);
}
```

- [ ] **Step 4: Re-export**

In `crates/appreels-render/src/lib.rs`, extend the effects `pub use` line:
```rust
pub use effects::{ZoomTransform, apply_zoom, draw_caption, draw_cursor_ring};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p appreels-render`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/appreels-render/src/effects.rs crates/appreels-render/src/lib.rs
git commit -m "feat(render): lower-third caption bar"
```

---

### Task 9: appreels-render — timeline-aware render orchestration

**Files:**
- Modify: `crates/appreels-render/src/lib.rs`

- [ ] **Step 1: Write the failing test (pure helper)**

Add to the `tests` module in `crates/appreels-render/src/lib.rs`:
```rust
    #[test]
    fn card_frame_count_rounds_to_fps() {
        assert_eq!(card_frame_count(1000, 30.0), 30);
        assert_eq!(card_frame_count(1500, 30.0), 45);
        assert_eq!(card_frame_count(0, 30.0), 0);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p appreels-render card_frame_count_rounds_to_fps`
Expected: FAIL — `card_frame_count` not found.

- [ ] **Step 3: Add imports, the outcome type, the helper, and `render_video`**

In `crates/appreels-render/src/lib.rs`, update the `use` block near the top to also pull in
the effects/text/timeline helpers and `style.accent`:
```rust
use image::RgbaImage;
use polish_core::{PresentationStyle, compose_frame};
use thiserror::Error;

use effects::{ZoomTransform, apply_zoom, draw_caption, draw_cursor_ring};
use text::font as load_font;
use timeline::{caption_at, cursor_at, parse_cursor_track, zoom_at};
```
(Keep the existing `use std::io::{Read, Write};` and `use std::process::{Command, Stdio};`.)

Add this helper and types near the other `pub fn`s (e.g. just above `frame_video`):
```rust
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
```

- [ ] **Step 4: Implement `render_video` and re-point `frame_video` at it**

Replace the existing `frame_video` function body in `crates/appreels-render/src/lib.rs` with
a thin wrapper, and add `render_video` below it:
```rust
/// Decode `input`, frame each frame through `compose_frame`, and re-encode to
/// `output`. Backwards-compatible no-effects render.
pub fn frame_video(
    input: &str,
    output: &str,
    style: &PresentationStyle,
) -> Result<VideoInfo, RenderError> {
    render_video(input, output, style, &Timeline::default(), None).map(|o| o.info)
}

/// Decode `input`, apply the `timeline` effects per frame (zoom, cursor ring,
/// caption) plus optional title/outro cards, and re-encode to `output`.
/// `cursor_track_path` overrides `timeline.cursor_track` when `Some`.
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
    let accent = style.accent;
    let font = load_font();
    let mut warnings = Vec::new();

    // Load the cursor track from the explicit path or the timeline reference.
    let track_path = cursor_track_path
        .map(str::to_string)
        .or_else(|| timeline.cursor_track.clone());
    let cursor_samples = match &track_path {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(text) => parse_cursor_track(&text),
            Err(e) => {
                warnings.push(format!("cursor track {path:?} unreadable: {e}; ring skipped"));
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
        .map_err(|source| RenderError::Spawn { program: "ffmpeg".into(), source })?;
    let mut encoder = Command::new("ffmpeg")
        .args(encode_args(canvas_w, canvas_h, info.fps, output))
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| RenderError::Spawn { program: "ffmpeg".into(), source })?;

    let mut dec_out = decoder.stdout.take().expect("decoder stdout");
    let mut enc_in = encoder.stdin.take().expect("encoder stdin");
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

    // Helper closure: write one canvas frame, reaping children on a broken pipe.
    // Returns Err to bail out of the whole render.
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

    // Title card.
    let has_title = timeline.title_card.is_some();
    if let Some(card) = &timeline.title_card {
        let frame = cards::render_card(canvas_w, canvas_h, &card.text, style);
        for _ in 0..card_frame_count(card.ms, info.fps) {
            write_frame!(frame.clone());
        }
    }

    // Source frames with effects.
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
        let frame = RgbaImage::from_raw(w, h, buf.clone()).expect("frame dimensions");

        // Zoom/pan, tracking the transform for the cursor.
        let (mut working, xform) = match zoom_at(&timeline.zooms, t_ms) {
            Some(z) => apply_zoom(&frame, z),
            None => (frame, ZoomTransform::identity()),
        };

        // Cursor ring at the (possibly zoomed) pointer position.
        if let Some((cx, cy)) = cursor_at(&cursor_samples, t_ms) {
            let (rx, ry) = xform.map(cx, cy);
            draw_cursor_ring(&mut working, rx, ry, accent);
        }

        // Polish-core framing.
        let mut composed = compose_frame(&working, style);

        // Caption bar.
        if let Some(caption) = caption_at(&timeline.captions, t_ms) {
            draw_caption(&mut composed, &font, &caption.text, accent);
        }

        write_frame!(composed);
        frame_index += 1;
    }

    // Outro card.
    let has_outro = timeline.outro_card.is_some();
    if let Some(card) = &timeline.outro_card {
        let frame = cards::render_card(canvas_w, canvas_h, &card.text, style);
        for _ in 0..card_frame_count(card.ms, info.fps) {
            write_frame!(frame.clone());
        }
    }

    drop(enc_in);
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
```

- [ ] **Step 5: Re-export `render_video`, `RenderOutcome`, `card_frame_count`**

In `crates/appreels-render/src/lib.rs`, add a re-export line near the others:
```rust
pub use cards::render_card;
```
is already present from Task 5; add nothing new for `render_video`/`RenderOutcome`/
`card_frame_count` because they are defined directly in `lib.rs` and already `pub`.

- [ ] **Step 6: Replace the old `#[ignore]` live test with an effects-aware one**

In `crates/appreels-render/src/lib.rs`, replace the existing `frames_a_generated_clip`
test with:
```rust
    #[test]
    #[ignore = "needs ffmpeg/ffprobe"]
    fn renders_a_clip_with_effects() {
        let dir = std::env::temp_dir();
        let src = dir.join("appreels-render-src.mp4");
        let out = dir.join("appreels-render-out.mp4");
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
        let timeline = Timeline {
            title_card: Some(Card { text: "Demo".into(), ms: 500 }),
            outro_card: Some(Card { text: "Thanks".into(), ms: 500 }),
            captions: vec![Caption { start_ms: 0, end_ms: 600, text: "hello".into() }],
            zooms: vec![ZoomCue { start_ms: 200, end_ms: 800, x: 160.0, y: 120.0, scale: 1.6 }],
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
```

- [ ] **Step 7: Run tests + the live test**

Run: `cargo test -p appreels-render`
Expected: PASS (all non-ignored tests, including `card_frame_count_rounds_to_fps`).
Run: `cargo test -p appreels-render -- --ignored renders_a_clip_with_effects`
Expected: PASS (writes a framed mp4 with cards; re-probes to canvas width).

- [ ] **Step 8: Commit**

```bash
git add crates/appreels-render/src/lib.rs
git commit -m "feat(render): timeline-aware render with cards, zoom, cursor, captions"
```

---

### Task 10: appreels-capture — cursor-track poller

**Files:**
- Modify: `crates/appreels-capture/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/appreels-capture/src/lib.rs`:
```rust
    #[test]
    fn parses_mouse_location_shell() {
        let out = "X=512\nY=384\nSCREEN=0\nWINDOW=12345\n";
        assert_eq!(parse_mouse_location(out), Some((512, 384)));
    }

    #[test]
    fn rejects_incomplete_mouse_location() {
        assert!(parse_mouse_location("X=1\n").is_none());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p appreels-capture`
Expected: FAIL — `parse_mouse_location` not found.

- [ ] **Step 3: Implement `parse_mouse_location`**

Add to `crates/appreels-capture/src/lib.rs` (e.g. just below `parse_xdotool_geometry`):
```rust
/// Parse the output of `xdotool getmouselocation --shell` into `(x, y)` screen px.
pub fn parse_mouse_location(output: &str) -> Option<(i32, i32)> {
    let mut x = None;
    let mut y = None;
    for line in output.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "X" => x = value.trim().parse().ok(),
            "Y" => y = value.trim().parse().ok(),
            _ => {}
        }
    }
    Some((x?, y?))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p appreels-capture`
Expected: PASS.

- [ ] **Step 5: Implement `record_with_cursor`**

At the top of `crates/appreels-capture/src/lib.rs`, replace
`use std::process::Command;` with:
```rust
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
```

Add this function below `record`:
```rust
/// Record like [`record`], while concurrently polling the pointer into a JSONL
/// cursor track at `cursor_output`. Cursor positions are stored relative to
/// `region` (window-relative px), with `tMs` measured from capture start.
pub fn record_with_cursor(
    display: &str,
    region: Region,
    fps: u32,
    seconds: f64,
    output: &str,
    cursor_output: &str,
) -> Result<(), CaptureError> {
    let args = x11grab_args(display, region, fps, seconds, output);
    let mut child = Command::new("ffmpeg")
        .args(args.iter().map(String::as_str).collect::<Vec<_>>())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|source| CaptureError::Spawn {
            program: "ffmpeg".to_string(),
            source,
        })?;

    let mut file = std::fs::File::create(cursor_output).map_err(|source| CaptureError::Spawn {
        program: cursor_output.to_string(),
        source,
    })?;
    let start = Instant::now();

    loop {
        // Stop as soon as ffmpeg finishes the timed recording.
        let exited = child
            .try_wait()
            .map_err(|source| CaptureError::Spawn {
                program: "ffmpeg".to_string(),
                source,
            })?
            .is_some();
        if exited {
            break;
        }
        if let Ok(out) = Command::new("xdotool")
            .args(["getmouselocation", "--shell"])
            .env("DISPLAY", display)
            .output()
        {
            if let Some((sx, sy)) = parse_mouse_location(&String::from_utf8_lossy(&out.stdout)) {
                let t_ms = start.elapsed().as_millis();
                let (rx, ry) = (sx - region.x, sy - region.y);
                let _ = writeln!(file, "{{\"tMs\":{t_ms},\"x\":{rx},\"y\":{ry}}}");
            }
        }
        std::thread::sleep(Duration::from_millis(16));
    }

    let status = child.wait().map_err(|source| CaptureError::Spawn {
        program: "ffmpeg".to_string(),
        source,
    })?;
    if !status.success() {
        return Err(CaptureError::Failed {
            program: "ffmpeg".to_string(),
            status,
            stderr: String::new(),
        });
    }
    Ok(())
}
```

- [ ] **Step 6: Add an `#[ignore]`d live test**

Add to the `tests` module in `crates/appreels-capture/src/lib.rs`:
```rust
    #[test]
    #[ignore = "needs ffmpeg, xdotool, and an X display"]
    fn records_a_clip_with_cursor_track() {
        let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
        let dir = std::env::temp_dir();
        let video = dir.join("appreels-capture-cursor.mp4");
        let track = dir.join("appreels-capture-cursor.jsonl");
        let region = Region { x: 0, y: 0, width: 320, height: 240 };
        record_with_cursor(
            &display,
            region,
            10,
            1.0,
            video.to_str().unwrap(),
            track.to_str().unwrap(),
        )
        .expect("record");
        assert!(video.metadata().expect("video").len() > 0);
        let text = std::fs::read_to_string(&track).expect("track");
        assert!(text.lines().any(|l| l.contains("\"tMs\"")), "expected cursor samples");
    }
```

- [ ] **Step 7: Run unit tests + the live test**

Run: `cargo test -p appreels-capture`
Expected: PASS (mouse-location parse tests).
Run: `DISPLAY="${DISPLAY:-:0}" cargo test -p appreels-capture -- --ignored records_a_clip_with_cursor_track`
Expected: PASS (writes a non-empty mp4 and a cursor track with samples).

- [ ] **Step 8: Commit**

```bash
git add crates/appreels-capture/src/lib.rs
git commit -m "feat(capture): cursor-track poller via xdotool getmouselocation"
```

---

### Task 11: appreels CLI — cue/flag plumbing for record + render

**Files:**
- Modify: `crates/appreels/src/cli.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/appreels/src/cli.rs`:
```rust
    #[test]
    fn parses_caption_flag_keeping_colons_in_text() {
        let c = parse_caption_flag("0:1800:Open: the menu").expect("caption");
        assert_eq!((c.start_ms, c.end_ms), (0, 1800));
        assert_eq!(c.text, "Open: the menu");
    }

    #[test]
    fn rejects_caption_flag_without_text() {
        assert!(parse_caption_flag("0:1800").is_err());
    }

    #[test]
    fn derives_cursor_track_path_from_output() {
        let p = default_cursor_track(std::path::Path::new("/tmp/raw.mp4"));
        assert_eq!(p, std::path::PathBuf::from("/tmp/raw.cursor.jsonl"));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p appreels`
Expected: FAIL — `parse_caption_flag`, `default_cursor_track` not found.

- [ ] **Step 3: Add the `Record` cursor-track option + `Render` effect options**

In `crates/appreels/src/cli.rs`, in `enum Command`, add to the `Record` variant (after `out`):
```rust
        /// Cursor track output path (JSONL). Defaults to <out>.cursor.jsonl.
        #[arg(long)]
        cursor_track: Option<PathBuf>,
```

Replace the entire `Render` variant with:
```rust
    /// Frame a recorded video with the appshots look + effects.
    Render {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Style seed (omit for the default style).
        #[arg(long)]
        style_seed: Option<u64>,
        /// Sidecar cue file (JSON) with cards/captions/zooms/cursorTrack.
        #[arg(long)]
        cues: Option<PathBuf>,
        /// Title card text (uses the default duration).
        #[arg(long)]
        title: Option<String>,
        /// Outro card text (uses the default duration).
        #[arg(long)]
        outro: Option<String>,
        /// Caption as "startMs:endMs:text" (repeatable).
        #[arg(long)]
        caption: Vec<String>,
        /// Cursor track (JSONL) override; defaults to auto-discovery next to --input.
        #[arg(long)]
        cursor_track: Option<PathBuf>,
    },
```

- [ ] **Step 4: Add the helpers**

In `crates/appreels/src/cli.rs`, add near `parse_region`:
```rust
const DEFAULT_CARD_MS: u32 = 1500;

fn parse_caption_flag(spec: &str) -> Result<appreels_render::Caption, Box<dyn std::error::Error>> {
    let mut parts = spec.splitn(3, ':');
    let start_ms = parts
        .next()
        .ok_or("caption must be \"startMs:endMs:text\"")?
        .trim()
        .parse()?;
    let end_ms = parts
        .next()
        .ok_or("caption must be \"startMs:endMs:text\"")?
        .trim()
        .parse()?;
    let text = parts
        .next()
        .ok_or("caption must be \"startMs:endMs:text\"")?
        .to_string();
    Ok(appreels_render::Caption { start_ms, end_ms, text })
}

/// `<out>.cursor.jsonl` next to the recording.
fn default_cursor_track(out: &std::path::Path) -> PathBuf {
    out.with_extension("cursor.jsonl")
}
```

- [ ] **Step 5: Update the `Record` handler to write a cursor track**

In `crates/appreels/src/cli.rs`, replace the `Command::Record { .. }` match arm with:
```rust
        Command::Record {
            window,
            region,
            display,
            fps,
            seconds,
            out,
            cursor_track,
        } => {
            let resolved = match (window, region) {
                (Some(title), _) => appreels_capture::resolve_window(&title)?,
                (_, Some(spec)) => parse_region(&spec)?,
                (None, None) => return Err("provide --window or --region".into()),
            };
            let out_str = out.to_str().ok_or("output path must be valid UTF-8")?;
            let track = cursor_track.unwrap_or_else(|| default_cursor_track(&out));
            let track_str = track.to_str().ok_or("cursor track path must be valid UTF-8")?;
            appreels_capture::record_with_cursor(
                &display, resolved, fps, seconds, out_str, track_str,
            )?;
            let report = serde_json::json!({
                "ok": true,
                "command": "record",
                "output": out,
                "cursorTrack": track,
                "region": { "x": resolved.x, "y": resolved.y, "width": resolved.width, "height": resolved.height },
                "fps": fps,
                "seconds": seconds,
            });
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(ExitCode::SUCCESS)
        }
```

- [ ] **Step 6: Update the `Render` handler to build a timeline + call `render_video`**

In `crates/appreels/src/cli.rs`, replace the `Command::Render { .. }` match arm with:
```rust
        Command::Render {
            input,
            out,
            style_seed,
            cues,
            title,
            outro,
            caption,
            cursor_track,
        } => {
            let style = match style_seed {
                Some(seed) => polish_core::style_from_seed(seed),
                None => polish_core::style_from_seed(default_seed()),
            };
            let input_str = input.to_str().ok_or("input path must be valid UTF-8")?;
            let out_str = out.to_str().ok_or("output path must be valid UTF-8")?;

            // Start from the cue file (if any), then merge CLI flags on top.
            let mut timeline = match &cues {
                Some(path) => appreels_render::Timeline::from_json(&std::fs::read_to_string(path)?)?,
                None => appreels_render::Timeline::default(),
            };
            if let Some(text) = title {
                timeline.title_card = Some(appreels_render::Card { text, ms: DEFAULT_CARD_MS });
            }
            if let Some(text) = outro {
                timeline.outro_card = Some(appreels_render::Card { text, ms: DEFAULT_CARD_MS });
            }
            for spec in &caption {
                timeline.captions.push(parse_caption_flag(spec)?);
            }

            // Cursor track: explicit flag > cue file reference > auto-discovery.
            let auto_track = default_cursor_track(&input);
            let cursor_track_path = cursor_track
                .or_else(|| {
                    if timeline.cursor_track.is_some() {
                        None
                    } else if auto_track.exists() {
                        Some(auto_track)
                    } else {
                        None
                    }
                });
            let cursor_track_str = cursor_track_path
                .as_ref()
                .map(|p| p.to_str().ok_or("cursor track path must be valid UTF-8"))
                .transpose()?;

            let outcome = appreels_render::render_video(
                input_str,
                out_str,
                &style,
                &timeline,
                cursor_track_str,
            )?;
            let report = serde_json::json!({
                "ok": true,
                "command": "render",
                "output": out,
                "styleSeed": style.seed,
                "palette": style.palette_name,
                "source": {
                    "width": outcome.info.width,
                    "height": outcome.info.height,
                    "fps": outcome.info.fps,
                },
                "effects": {
                    "captions": outcome.captions,
                    "zooms": outcome.zooms,
                    "cursorTrackUsed": outcome.cursor_track_used,
                    "titleCard": outcome.title_card,
                    "outroCard": outcome.outro_card,
                },
                "warnings": outcome.warnings,
            });
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(ExitCode::SUCCESS)
        }
```

- [ ] **Step 7: Run tests + build**

Run: `cargo test -p appreels`
Expected: PASS (caption-flag + cursor-track-path tests + existing region tests).
Run: `cargo build`
Expected: clean.

- [ ] **Step 8: Verify the CLI end-to-end manually (needs display + ffmpeg)**

Run:
```bash
cargo run -p appreels -- record --region 0,0,640,480 --display "$DISPLAY" --seconds 3 --out /tmp/raw.mp4
cargo run -p appreels -- render --input /tmp/raw.mp4 --out /tmp/demo.mp4 \
  --style-seed 42 --title "Demo" --outro "Thanks" --caption "0:2000:Hello there"
ffprobe -v error -select_streams v:0 -show_entries stream=width,height,nb_frames \
  -of default=noprint_wrappers=1 /tmp/demo.mp4
ffmpeg -y -v error -i /tmp/demo.mp4 -ss 0.2 -frames:v 1 /tmp/demo_frame.png
```
Expected: `record` prints `ok: true` with a `cursorTrack` path and writes
`/tmp/raw.cursor.jsonl`; `render` prints `ok: true` with an `effects` block and the cursor
track auto-discovered (`cursorTrackUsed: true`). `/tmp/demo.mp4` is longer than the source
(cards) and `/tmp/demo_frame.png` shows the framed look. Open a mid-clip frame to confirm
the caption bar, accent keyline, and cursor ring are present.

- [ ] **Step 9: Commit**

```bash
git add crates/appreels/src/cli.rs
git commit -m "feat(cli): cue file + effect flags for render, cursor track for record"
```

---

### Task 12: README + workspace verification

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Document the effects**

Add a section to `README.md` (after the existing recording section):
```markdown
## Effects (phase 3)

`render` adds title/outro cards, lower-third captions, eased zoom/pan, and a cursor accent
ring. `record` writes a cursor track (`<out>.cursor.jsonl`) that `render` auto-discovers
next to the input.

```bash
# Record (also writes raw.cursor.jsonl), then render with quick flags:
appreels record --region 0,0,1280,720 --display :0 --seconds 6 --out raw.mp4
appreels render --input raw.mp4 --out demo.mp4 --style-seed 42 \
  --title "Create a project" --outro "Thanks!" --caption "0:2000:Open the menu"

# Or drive effects from a cue file:
appreels render --input raw.mp4 --out demo.mp4 --cues cues.json
```

Cue file shape (`cues.json`, all fields optional):

```json
{
  "cursorTrack": "raw.cursor.jsonl",
  "titleCard": { "text": "Create a project", "ms": 1500 },
  "captions":  [ { "startMs": 0,    "endMs": 1800, "text": "Open the menu" } ],
  "zooms":     [ { "startMs": 2000, "endMs": 5000, "x": 420, "y": 300, "scale": 1.8 } ],
  "outroCard": { "text": "Thanks!", "ms": 1500 }
}
```
```

- [ ] **Step 2: Verify the full gate on latest stable**

Run: `rustup update stable && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: fmt clean, clippy clean, all (non-ignored) tests pass.

If `cargo fmt --check` reports diffs, run `cargo fmt` and re-stage. If clippy flags the
`write_frame!` macro or unused imports, address them (e.g. `#[allow(unused)]` is NOT
acceptable; fix the root cause).

- [ ] **Step 3: Run the live tests once more**

Run: `cargo test -p appreels-render -- --ignored renders_a_clip_with_effects`
Run: `DISPLAY="${DISPLAY:-:0}" cargo test -p appreels-capture -- --ignored records_a_clip_with_cursor_track`
Expected: both PASS.

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs: document phase 3 effects (cards, captions, zoom, cursor)"
```

---

## Self-Review

- **Spec coverage:**
  - Cue file + CLI flags (sidecar timeline JSON + flags) → Tasks 2, 3, 11. ✓
  - Cursor track from `record` (poller) → Task 10; window-relative coords ✓.
  - Frame-time-aware pipeline with the documented compositing order → Task 9. ✓
  - Zoom/pan (crop+scale, eased in/hold/out) → Tasks 3 (easing) + 6 (pixels) + 9 (wiring). ✓
  - Cursor ring (constant size, zoom-transformed position) → Task 7 + 9. ✓
  - Caption bar (scrim + accent keyline + bold text) → Task 8. ✓
  - Title/outro cards (gradient backdrop + centered text) → Task 5 + 9. ✓
  - Text via `ab_glyph` + bundled font → Task 4. ✓
  - `polish-core::gradient_backdrop` extraction → Task 1. ✓
  - Module split (`timeline`/`text`/`cards`/`effects`) → Tasks 2,4,5,6. ✓
  - Degrade-to-warning error handling (missing cursor track, OOB zoom clamp) → Task 9
    (warning on unreadable track) + Task 6 (clamp). ✓
  - Stable JSON with `warnings`/`effects` → Task 11. ✓
  - Click ripple deferred → not in plan (intentional, per spec non-goals). ✓
- **Placeholder scan:** none — every code step shows complete code; commands have expected
  output. The `#[allow(unused)]` mention in Task 12 is a prohibition, not a placeholder.
- **Type consistency:** `Timeline`/`Card`/`Caption`/`ZoomCue`/`CursorSample`/`ZoomState`
  (timeline.rs, re-exported); `ZoomTransform`/`apply_zoom`/`draw_cursor_ring`/`draw_caption`
  (effects.rs); `font`/`text_width`/`draw_text`/`draw_text_centered`/`blend_pixel`
  (text.rs); `render_card` (cards.rs); `render_video`/`RenderOutcome`/`card_frame_count`/
  `frame_video` (lib.rs); `parse_mouse_location`/`record_with_cursor` (capture);
  `parse_caption_flag`/`default_cursor_track`/`DEFAULT_CARD_MS` (cli). `render_video`
  signature `(input, output, style, &Timeline, Option<&str>)` is used identically in Task 9
  (definition + test) and Task 11 (caller). `gradient_backdrop(w,h,&style)` defined in
  Task 1 and called in Tasks 5 + 9 (via `render_card`).
- **Even-dimension safety:** unchanged — `encode_args` keeps `-vf pad=ceil(iw/2)*2:ceil(ih/2)*2`,
  and all emitted frames (cards + framed) share the canvas size, so the single `-s` is valid.
