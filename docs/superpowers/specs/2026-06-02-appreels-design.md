# appreels — Design Spec

Date: 2026-06-02
Status: approved (brainstorm), pending implementation plan

## Summary

`appreels` is the video sibling of [appshots](https://github.com/solomonneas/appshots):
an agent-neutral CLI that produces **polished demo videos**. An LLM can autonomously
drive a browser or desktop app to perform a demo, which is recorded and wrapped in the
same clean framed look appshots gives screenshots (gradient backdrop, rounded window,
soft shadow, padding), enriched with cursor emphasis, auto zoom/pan, captions, and an
optional TTS voiceover.

It is a standalone Rust repo (`solomonneas/appreels`, Apache-2.0), structured as a Cargo
workspace. The appshots polish look is factored into a shared, published `polish-core`
crate that both tools depend on (appshots migrates onto it as a fast-follow, out of the
appreels v1 critical path).

## Goals

- Produce a finished, shareable demo `.mp4` from a single command.
- Two control models, selectable: **director** (LLM authors + rehearses a deterministic
  script off-camera, then the recorder performs it cleanly) and **live puppeteer** (LLM
  drives during capture, for quick rough takes).
- Drive both **browser** (Chrome DevTools, reliable DOM/AX targets) and **desktop** Linux
  apps (xdotool + screenshot/AT-SPI vision) behind one driver abstraction.
- Reproduce the appshots aesthetic exactly via post-compositing, plus a live OBS scene
  path (hybrid).
- Stable JSON output and the appshots CLI conventions (`doctor`, `schema`, camelCase wire
  keys, ok/warnings/errors) so any agent can call it as a subprocess.

## Non-goals (v1)

- Webcam / talking-head overlay (deferred; OBS can do it later).
- Wayland capture (X11 first, matching appshots' primary path).
- macOS/Windows backends (Linux X11 first).
- A GUI editor. The demo-script is hand-editable JSON; no timeline UI.

## Target platform

Linux X11 (developed on DISPLAY=:1). Available and assumed: `ffmpeg`, OBS Studio 32 +
`obs-cmd`, `xdotool`, `wmctrl`, `scrot`, Chrome/Chromium with DevTools. `doctor` reports
which are present and degrades features accordingly.

## Architecture

Cargo workspace. Crates, each single-purpose and independently testable:

| Crate | Responsibility |
|-------|----------------|
| `polish-core` | The appshots look on one `RgbaImage`: gradient backdrop, rounded corners, soft shadow, padding. Pure, deterministic. Shared/published. |
| `appreels-script` | Demo-script format: serde types + JSON schema. The artifact the director authors and the performer replays. |
| `appreels-capture` | Recording backends. Hybrid: ffmpeg `x11grab` (raw window/region) and OBS (`obs-cmd`/obsctl) live scene. Emits raw video + a cursor/timing track. |
| `appreels-driver` | One `Driver` trait; `BrowserDriver` (CDP locate) + `DesktopDriver` (AT-SPI/vision locate). Both **act with the real OS cursor via xdotool** for visible motion. |
| `appreels-render` | Post pipeline (ffmpeg): per-frame `polish-core` framing + auto zoom/pan + cursor emphasis + captions + title cards + voiceover mux → final mp4. |
| `appreels-voice` | TTS: synthesize LLM-written narration lines, return audio + timing. Pluggable engine. |
| `appreels-agent` | The LLM director loop (explore → author → rehearse → verify → emit clean script) and live-puppeteer mode. Pluggable LLM client. |
| `appreels` (bin) | Orchestrator CLI: `record` / `author` / `perform` / `render` / `run` / `doctor` / `schema`. |

### Data flow — director mode (`appreels run --goal "..."`)

```
goal
  → agent: perceive via driver (DOM/AX | screenshot+AT-SPI), pick step, execute in
    rehearsal (no recording), verify state changed, append clean step → repeat
  → script.json
  → performer: replay script via driver while capture records
  → raw.mp4 + cursor/timing track
  → voice: synthesize narration → narration.wav + cue timings
  → render: polish-core framing per frame + zoom/pan + cursor FX + captions + cards + mux
  → final demo.mp4 + metadata.json
```

Live mode short-circuits: agent drives during capture, no script emitted, lighter render.

## Demo-script format (`appreels-script`)

JSON, schema-published. Targets are re-resolved at perform time so layout drift doesn't
break a saved script.

```json
{
  "version": "0.1.0",
  "title": "Create a new project",
  "target": { "kind": "browser", "url": "https://app.example.com" },
  "viewport": { "width": 1280, "height": 800 },
  "defaults": { "moveMs": 600, "settleMs": 400, "easing": "easeInOut" },
  "steps": [
    { "type": "narrate", "text": "Let's create a project." },
    { "type": "caption", "text": "Open the menu", "anchor": "target", "durationMs": 1800 },
    { "type": "zoom",  "to": { "selector": "#new" }, "scale": 1.8, "holdMs": 1200 },
    { "type": "click", "target": { "selector": "#new" } },
    { "type": "type",  "target": { "selector": "input[name=title]" }, "text": "Demo" },
    { "type": "wait",  "ms": 800 },
    { "type": "zoom",  "reset": true }
  ]
}
```

- `target.kind`: `browser` (with `url`) | `desktop` (with `app`/`windowTitle`).
- Step targets: `{selector}` | `{coord:{x,y}}` | `{imageAnchor:"path"}` (template match).
- Steps: `narrate`, `caption`, `move`, `click`, `type`, `key`, `wait`, `zoom`, `scroll`.
- Any action step may carry inline `caption`/`narrate` for authoring convenience.

## Driver (`appreels-driver`)

```rust
pub struct Hit { pub x: i32, pub y: i32, pub w: u32, pub h: u32 }   // screen coords
pub trait Driver {
    fn locate(&mut self, target: &Target) -> Result<Hit>;
    fn move_to(&mut self, x: i32, y: i32, opts: MoveOpts) -> Result<()>; // eased, real cursor
    fn click(&mut self, x: i32, y: i32, button: Button) -> Result<()>;
    fn type_text(&mut self, text: &str) -> Result<()>;
    fn key(&mut self, chord: &str) -> Result<()>;
    fn perceive(&mut self) -> Result<Perception>;   // agent's eyes
    fn screen_rect(&self) -> Rect;                   // maps capture region ↔ screen
}
```

- `BrowserDriver`: Chrome DevTools Protocol. `locate` via `DOM.querySelector` +
  `DOM.getBoxModel`, offset by the browser window's screen position (via wmctrl/xdotool)
  → absolute screen coords. `perceive` returns the DOM/accessibility tree.
- `DesktopDriver`: `locate` via AT-SPI (reuse appshots' accessible-text approach) and/or
  template-image matching against a screenshot. `perceive` returns a screenshot + AT-SPI
  text.
- **Cursor visibility rule**: CDP synthetic mouse events do not move the visible OS
  pointer. Both drivers therefore move the **real cursor with xdotool** to the resolved
  screen coords (eased) and click there, so the recording shows genuine cursor motion.
  CDP is used for reliable *locating*, xdotool for visible *acting*.
- A `MockDriver` (scripted hits, recorded actions) backs unit tests for the performer and
  agent loops without a real display.

## Agent (`appreels-agent`)

- **Director loop**: `perceive → LLM proposes next step toward goal → execute in rehearsal
  → verify (re-perceive: did expected state change?) → append clean step → repeat` until
  the goal is satisfied or a step/turn budget is exhausted. Dead ends and retries are
  pruned, so the emitted `script.json` performs flawlessly.
- **Live mode**: same loop, run during capture, emits no script.
- **LLM client**: a `LlmClient` trait (one method: propose next action given goal +
  perception + history). Concrete impls are pluggable; v1 ships one (e.g. a CLI/API-backed
  client) behind a feature so the rest of the system is testable with a scripted fake.

## Capture (`appreels-capture`)

- `FfmpegBackend`: `x11grab` of a window rect (resolved via wmctrl/xdotool) or full
  region; records raw lossless-ish mp4; writes a `cursor.jsonl` track (timestamped pointer
  positions + click events, sourced from the performer/driver, not screen-scraping).
- `ObsBackend`: drives OBS via `obs-cmd` (start/stop recording, scene with background +
  window capture sources) for the live/WYSIWYG path.
- Both produce a `Capture { video_path, cursor_track, started_at, region }`.

## Render (`appreels-render`)

ffmpeg-driven post pipeline. Given `Capture` + optional `script` (for caption/zoom cues)
+ optional narration audio:

1. Per-frame framing via `polish-core` (gradient backdrop, rounded window, soft shadow,
   padding) — exact appshots match.
2. Auto zoom/pan: smooth eased zoom toward the active region (from script `zoom` cues or
   inferred from the cursor track), then back out.
3. Cursor emphasis: accent ring + click ripple, drawn from the cursor track.
4. Captions: lower-third bar (gradient scrim, left accent keyline, bold text) timed to
   `caption` cues.
5. Title/outro cards using the active palette.
6. Mux the voiceover audio track.

Output: `demo.mp4` + `metadata.json` (appshots-style stable contract: ok, version,
createdAt, inputs, palette/preset, durations, warnings, errors).

### Visual default ("clean" preset)

Minimal **A** frame (gradient backdrop from appshots' 5 palettes, generous padding, raw
window — no titlebar — rounded corners, soft drop shadow, accent-ring cursor) with **B**'s
caption: a lower-third bar (bottom, gradient scrim, left accent keyline, bold text).
Later presets: A's center pill caption, C cinematic-dark, and a with-titlebar variant.

## Error handling

Mirrors appshots: a JSON result with `ok`/`warnings`/`errors`. Optional effects degrade to
warnings (TTS engine missing → silent track + warning; zoom/caption render failure →
skipped + warning). `locate` failure during *authoring* → agent re-perceives / tries an
alternate target; during *perform* → hard error naming the failing step so it can be
re-authored. `doctor` reports missing dependencies and which features are unavailable.

## Testing

- `polish-core`: deterministic pixel/structure tests (as appshots' polish does).
- `appreels-script`: serde round-trip + schema generation tests.
- `appreels-driver`: `MockDriver` for logic; real CDP/xdotool behind `--ignored`/feature
  live tests.
- `appreels-render`: filtergraph-construction tests + small fixture-frame renders compared
  structurally; full renders behind live tests (need ffmpeg).
- `appreels-agent`: director loop tested with a scripted fake `LlmClient` + `MockDriver`
  (à la appshots' `mcp::dispatch` mock-runner tests).
- `appreels` (cli): pure arg-building / dispatch tests.
- CI: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` on
  ubuntu-latest, on the **latest stable** toolchain (lesson from appshots: pin CI to the
  newest stable and run clippy locally on it before pushing).

## Build order (bottom-up phases)

1. **Foundation**: `polish-core` (extracted from appshots `polish.rs`, frame-oriented) +
   `appreels-script` (format + schema) + `appreels` CLI skeleton (`doctor`, `schema`) + CI.
2. **Polished recorder MVP**: `appreels-capture` (ffmpeg x11grab + cursor track) +
   `appreels-render` (polish-core framing, default look) → `record` produces a framed mp4.
3. **Effects**: cursor emphasis, auto zoom/pan, captions, title cards in `appreels-render`.
4. **Driver + perform**: `appreels-driver` (browser CDP + desktop xdotool, real-cursor
   acting) + `perform` (replay a script while capturing).
5. **Voice**: `appreels-voice` TTS + voiceover mux.
6. **Agent**: `appreels-agent` director loop + live mode; `author` and `run` end-to-end.

Each phase ships and is verifiable on its own. v1 = through phase 6; phases 1-3 already
deliver the "polished recorder" value even without the LLM.

## Open questions (resolve during planning, non-blocking)

- Exact `polish-core` extraction path: publish first vs. vendor into appreels then extract.
- Which concrete TTS engine and LLM client to wire for the reference build.
- Zoom source of truth when both script cues and cursor-inferred regions exist.
