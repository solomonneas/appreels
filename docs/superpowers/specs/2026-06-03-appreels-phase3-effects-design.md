# appreels Phase 3 (Effects) — Design Spec

Date: 2026-06-03
Status: approved (brainstorm), pending implementation plan

## Summary

Phase 3 turns `appreels render` from a per-frame framer into an effects pipeline: a
recorded clip becomes a polished demo with a **title/outro card**, **lower-third
captions**, **eased zoom/pan**, and a **cursor accent ring**. Effects are driven by a
sidecar **cue file** (absolute-timed) plus CLI convenience flags. `appreels record` gains a
**cursor-track poller** so the ring has real position data now, ahead of the phase 4
driver.

This realizes the "Effects" phase of the [appreels design spec](2026-06-02-appreels-design.md)
build order (item 3). Click-ripple emphasis is intentionally deferred to phase 4, where the
driver produces an authoritative cursor track that includes click events.

## Goals

- `render` composites zoom/pan, cursor ring, captions, and title/outro cards into the
  existing `polish-core` framed look, producing a finished demo `.mp4`.
- Effects are supplied by an absolute-timed **cue file** (the same artifact phase 4's
  performer will emit) plus CLI flags for simple one-off cases.
- `record` writes a `cursor.jsonl` track alongside the raw video.
- Every piece stays unit-testable: pure cue parsing/merge/interpolation + pure pixel
  operations on fixture frames; subprocess/live behaviors behind `#[ignore]` tests.
- Stable JSON output in the appshots convention (`ok`/`warnings`/`errors`, camelCase).

## Non-goals (phase 3)

- **Cursor click ripple** — needs real click events; deferred to phase 4 (driver).
- **Auto/inferred zoom** from the cursor track — phase 3 zoom is explicit cue regions
  only. Cursor-inferred zoom is a later refinement.
- Driving effects from the authoring `appreels-script` format — that format carries only
  relative authoring hints (`moveMs`/`durationMs`), no absolute timeline. Mapping it to a
  timeline is the performer's job (phase 4). Phase 3 consumes the absolute cue file
  directly.
- Voiceover / narration audio (phase 5).

## Rendering approach

**Pure-Rust per-frame compositing.** ffmpeg is used only for decode (→ raw RGBA frames)
and encode (raw RGBA → mp4), exactly as `frame_video` already does. All effects are
in-Rust pixel operations using `image` + `imageproc`. Rationale: matches `polish-core`'s
pure/deterministic model, gives exact pixel control over the accent look, and makes every
effect unit-testable on fixture frames. ffmpeg filtergraphs (`zoompan`/`drawtext`) were
rejected: they can't reproduce the exact `polish-core` look without round-tripping frames
anyway, and filter strings are awkward to test.

## Data contracts

Two files, both camelCase, both optional (no cues → today's plain framing).

### Cue file (`--cues cues.json`)

```json
{
  "cursorTrack": "raw.cursor.jsonl",
  "titleCard": { "text": "Create a project", "ms": 1500 },
  "captions":  [ { "startMs": 0,    "endMs": 1800, "text": "Open the menu" } ],
  "zooms":     [ { "startMs": 2000, "endMs": 5000, "x": 420, "y": 300, "scale": 1.8 } ],
  "outroCard": { "text": "Thanks!", "ms": 1500 }
}
```

- All times are absolute milliseconds from the start of the **source** video (cards add
  time outside this range; see below).
- `cursorTrack` is a path (relative paths resolve against the cue file's directory). May be
  overridden by `--cursor-track`.
- `titleCard`/`outroCard`/`captions`/`zooms` are all optional.

### Cursor track (`cursor.jsonl`)

One JSON object per line, written by `record`:

```json
{"tMs":16,"x":120,"y":88}
```

- `tMs` is milliseconds from capture start; `x`/`y` are **window-relative** pixels
  (`record` subtracts the capture region origin), so render never deals with screen
  offsets.

### Coordinate spaces

- **Source-window pixels** (the recorded W×H region): cursor samples and zoom `x`/`y`.
- **Canvas pixels** (the framed output, `(W+2·padding) × (H+2·padding+shadowOffsetY)`):
  caption bar and cards. Constant for every emitted frame so the encoder stays single-size.

## `record` — cursor-track capture (`appreels-capture`)

Today `record()` blocks on `ffmpeg.status()`. New behavior:

1. Spawn `ffmpeg` x11grab as a child (not `.status()`).
2. Spawn a poller thread: loop every ~16 ms running `xdotool getmouselocation --shell`,
   parse it, write `{tMs,x,y}` (window-relative) as a JSON line to the cursor track, until
   the ffmpeg child exits.
3. Join the poller, then check the ffmpeg exit status (existing error semantics preserved).

New pure helper `parse_mouse_location(&str) -> Option<(i32, i32)>` (parses the `X=…\nY=…`
shell output; unit-tested). New entry point e.g. `record_with_cursor(display, region, fps,
seconds, video_out, cursor_out)`; the existing `record` is kept (or becomes a thin wrapper
with no cursor track) so the no-cursor path still works.

CLI `record` gains `--cursor-track PATH` (default derived from `--out`, e.g.
`out.cursor.jsonl`). The record report JSON gains `cursorTrack`.

## `render` — frame-time-aware pipeline (`appreels-render`)

`frame_video` becomes timeline-aware. For each **output** frame, `tMs = frame_index *
1000 / fps`. Pipeline for a source frame:

```
source RGBA frame (W×H)
  → apply zoom/pan      crop a sub-rect, scale back to W×H; eased in/hold/out
  → draw cursor ring    interpolated track position, transformed by active zoom,
                        constant on-screen radius
  → compose_frame       polish-core: backdrop + rounded window + shadow + padding → canvas
  → draw caption bar     lower-third scrim + accent keyline + bold text, if tMs in a window
  → emit canvas frame
```

**Cards** are full-canvas generated frames: `titleCard` emitted before the first source
frame, `outroCard` after the last, each `round(ms * fps / 1000)` frames. Total output
duration = title + source + outro. All frames are canvas-sized.

### Effect details

- **Zoom/pan:** crop a sub-rect centered on `(x,y)` sized `(W/scale, H/scale)` (clamped to
  the frame; out-of-bounds → clamp + warning), resized to W×H so the framed window outline
  stays constant while content magnifies. Scale and center are eased (easeInOut): ramp up
  over the first ~30% of `[startMs,endMs]`, hold, ramp down over the last ~30%. Overlapping
  zoom cues are not supported in phase 3 (last-wins + warning).
- **Cursor ring:** accent-colored stroked circle with soft alpha, constant on-screen
  radius. Position linearly interpolated between the two nearest track samples by `tMs`;
  before the first / after the last sample it clamps to the nearest. The ring is drawn in
  post-zoom space at the cursor's transformed position, so it tracks the real pointer and
  keeps a constant size regardless of zoom.
- **Caption bar:** the spec's default "clean" lower-third — a bottom gradient scrim, a left
  vertical accent keyline, and bold text. Active while `tMs ∈ [startMs,endMs]`. Optional
  short fade in/out is a nice-to-have, not required.
- **Cards:** palette-gradient background (see below) with centered bold text.

### Text rendering

`ab_glyph` (`FontRef`) + `imageproc::drawing::draw_text_mut` onto `RgbaImage`. One open
font (OFL or Apache licensed) is bundled into the crate via `include_bytes!`, with its
license recorded in the repo. A measure helper (`text_width`) sums glyph advances for
centering and bar sizing. Shared by captions and cards.

### polish-core addition

`compose_frame` already renders the gradient backdrop internally. Extract a public
`polish_core::gradient_backdrop(width, height, &PresentationStyle) -> RgbaImage` and have
`compose_frame` call it, so cards reuse the exact same backdrop. This is the only change to
`polish-core`; its existing behavior and tests are unaffected.

## Module structure (`appreels-render`)

The crate is one ~320-line file today; phase 3 grows it, so split by responsibility:

| File | Responsibility |
|------|----------------|
| `lib.rs` | `VideoInfo`, `parse_ffprobe`, `probe`, ffprobe/decode/encode arg builders, `frame_video` orchestration, `RenderError`, re-exports. |
| `timeline.rs` | `Timeline`, `Caption`, `ZoomCue`, `Card`, `CursorSample`; JSON parse; CLI-flag merge; cursor interpolation; caption/zoom lookup by time. |
| `effects.rs` | `apply_zoom`, `draw_cursor_ring`, `draw_caption` — pure pixel ops. |
| `cards.rs` | `render_card` — full-canvas title/outro frame. |
| `text.rs` | bundled font, `draw_text_centered`, `text_width`. |

## CLI surface (`appreels`)

`render` gains:

- `--cues PATH` — sidecar cue file.
- `--title TEXT`, `--outro TEXT` — convenience for cards (default `ms`).
- `--caption "startMs:endMs:text"` — repeatable; merged with cue-file captions.
- `--cursor-track PATH` — overrides `cursorTrack`; defaults to auto-discovery next to
  `--input` (e.g. `input.cursor.jsonl`) if present.

CLI flags merge **onto** whatever the cue file provides (flags add/override; neither is
required). `--style-seed` is unchanged. The render report JSON gains `warnings` and an
`effects` summary (counts of captions/zooms applied, whether a cursor track was used,
cards rendered).

## Error handling

Mirrors appshots — a JSON result with `ok`/`warnings`/`errors`; optional effects **degrade
to warnings** rather than failing the render:

- Missing/unreadable cursor track → skip the ring, add a warning.
- Zoom region outside the frame → clamp to bounds, add a warning.
- Overlapping zoom cues → last-wins, add a warning.
- Empty captions/cards → silently skipped (not an error).

Hard errors remain: unreadable input video, ffmpeg decode/encode failure, malformed cue
JSON (parse error names the field).

## Testing

**Unit (pure, no subprocess/display):**

- `timeline.rs`: `Timeline` serde round-trip; CLI-flag → timeline merge; cursor
  interpolation between two samples (and clamping past the ends); caption-active-by-time;
  zoom-active-by-time + ease factor at the in/hold/out boundaries.
- `effects.rs`: `apply_zoom` preserves W×H and magnifies a known fixture (center pixel
  matches the cropped region); `draw_cursor_ring` writes accent-colored pixels at the
  expected radius; `draw_caption` fills the lower-third band with non-background pixels.
- `cards.rs`: `render_card` returns a canvas-sized image with gradient + text pixels.
- `text.rs`: `text_width` is monotonic in string length; `draw_text_centered` writes
  pixels within the canvas.
- `appreels-capture`: `parse_mouse_location` parses `xdotool getmouselocation --shell`.
- `appreels` (cli): `--caption "start:end:text"` parsing; merge dispatch.

**`#[ignore]` live (need ffmpeg/xdotool/display):**

- `record_with_cursor` writes a non-empty `cursor.jsonl` whose samples are within the
  region bounds.
- A generated test clip rendered with a caption + zoom + title/outro card re-probes to the
  canvas size and a duration longer than the source by the card durations.

**CI gate (unchanged policy):** `cargo fmt --check && cargo clippy --all-targets -- -D
warnings && cargo test` on the latest stable toolchain.

## Build order (within phase 3)

1. `polish-core::gradient_backdrop` extraction (no behavior change).
2. `appreels-render` module split + `timeline.rs` cue model and parsing/merge/interpolation.
3. `text.rs` (bundled font) + `cards.rs`.
4. `effects.rs` (zoom, cursor ring, caption) + `frame_video` timeline orchestration.
5. `appreels-capture` cursor-track poller + `parse_mouse_location`.
6. CLI wiring (`render` cue/flag plumbing, `record --cursor-track`) + docs + full gate.

## Open questions (resolve during planning, non-blocking)

- Which bundled font (DejaVu Sans Bold vs Inter vs another OFL/Apache face) and at what
  default caption/title point sizes relative to canvas width.
- Default card durations and caption fade timing.
- Cursor poll interval (16 ms vs 33 ms) vs xdotool spawn overhead per sample.
