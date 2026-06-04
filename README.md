# appreels

`appreels` is the video sibling of [appshots](https://github.com/solomonneas/appshots):
an agent-neutral CLI that produces polished demo videos. An LLM can drive a browser or
desktop app to perform a demo, which is recorded and wrapped in the same clean framed
look appshots gives screenshots, with cursor emphasis, auto zoom, captions, and optional
voiceover.

Status: early development. The phase 1-3 core CLI works: dependency checks, script
schema output, X11 region/window recording with cursor tracks, and appshots-style
video rendering with title/outro cards, captions, zoom cues, and cursor emphasis.
See `docs/superpowers/specs/` for the design and `docs/superpowers/plans/` for
implementation plans.

## Commands (phase 1)

```bash
appreels doctor          # report capture/render dependency health as JSON
appreels schema          # print the demo-script JSON schema
```

## Recording and rendering

```bash
# Record a window for 5 seconds. This also writes raw.cursor.jsonl:
appreels record --window "Firefox" --display :0 --seconds 5 --out raw.mp4

# Frame it with the appshots look and quick effect flags:
appreels render --input raw.mp4 --out demo.mp4 --style-seed 42 \
  --title "Create a project" \
  --caption "0:1800:Open the menu" \
  --outro "Thanks"

# Or capture an explicit region:
appreels record --region 0,0,1280,720 --display :0 --seconds 5 --out raw.mp4
```

`render` also accepts a cue file:

```bash
appreels render --input raw.mp4 --out demo.mp4 --cues cues.json
```

Cue file shape, with all fields optional:

```json
{
  "cursorTrack": "raw.cursor.jsonl",
  "titleCard": { "text": "Create a project", "ms": 1500 },
  "captions": [{ "startMs": 0, "endMs": 1800, "text": "Open the menu" }],
  "zooms": [{ "startMs": 2000, "endMs": 5000, "x": 420, "y": 300, "scale": 1.8 }],
  "outroCard": { "text": "Thanks!", "ms": 1500 }
}
```

Cursor tracks are JSONL files of `{ "tMs": 0, "x": 100, "y": 120 }` samples
relative to the captured region. `render` uses `--cursor-track` when supplied,
then `cursorTrack` from the cue file, then auto-discovers `<input>.cursor.jsonl`
when present.

## Terminal demos

`perform-terminal` launches a real terminal, replays a JSON plan with `xdotool`,
records the exact terminal window, generates render cues, and writes a polished
demo video:

```bash
appreels perform-terminal --script terminal-demo.json --out demo.mp4 --display :0
```

Example `terminal-demo.json`:

```json
{
  "title": "Terminal showcase",
  "cols": 100,
  "rows": 28,
  "position": { "x": 120, "y": 110 },
  "stage": true,
  "stageMargin": 72,
  "startupMs": 1500,
  "tailMs": 1800,
  "typeDelayMs": 55,
  "settleMs": 600,
  "zoomScale": 1.04,
  "inputZoomScale": 1.06,
  "outputZoomScale": 1.045,
  "outro": "Done",
  "outroCardMs": 1600,
  "steps": [
    {
      "type": "run",
      "command": "echo hello",
      "waitMs": 2200,
      "caption": "Type and run the command",
      "focus": "output"
    },
    {
      "type": "wait",
      "ms": 1500,
      "caption": "Watch the generated output",
      "focus": "output"
    }
  ]
}
```

Supported terminal steps are `caption`, `type`, `run`, `key`, `wait`, and `zoom`.
Focus values are `input`, `output`, `center`, `full`, or explicit coordinates as
`{ "coord": { "x": 420, "y": 240 } }`. `run` automatically zooms toward the input
while typing and toward output while waiting. By default, `perform-terminal` launches
a black stage window behind the terminal and places the terminal at a stable screen
position, so semi-transparent terminal profiles do not leak whatever was behind the
window into the recording.

The repo includes a ready-to-run multi-command CLI showcase:

```bash
appreels perform-terminal \
  --script examples/terminal-cli-showcase.json \
  --out terminal-cli-showcase.mp4 \
  --display :0
```

## Browser demos

`perform-browser` launches a fresh Chrome app window, records that exact browser
window, replays paced real-cursor actions, and renders the final video with the
same cue pipeline:

```bash
appreels perform-browser --script browser-demo.json --out browser-demo.mp4 --display :0
```

Example `browser-demo.json`:

```json
{
  "title": "Browser workflow demo",
  "windowTitle": "Browser action showcase",
  "url": "examples/browser-actions-showcase.html",
  "width": 1180,
  "height": 760,
  "position": { "x": 95, "y": 75 },
  "startupMs": 1800,
  "tailMs": 1600,
  "moveMs": 680,
  "zoomScale": 1.07,
  "steps": [
    { "type": "click", "x": 465, "y": 175, "caption": "Click into the project field" },
    { "type": "type", "text": "Launch Kit", "caption": "Type the project name" },
    { "type": "click", "x": 980, "y": 175, "caption": "Create the project" }
  ]
}
```

Supported browser steps are `caption`, `click`, `type`, `key`, `scroll`, `wait`,
and `zoom`. Browser focus values are `center`, `full`, or explicit coordinates as
`{ "coord": { "x": 420, "y": 240 } }`. The bundled example page and script can be
run directly:

```bash
appreels perform-browser \
  --script examples/browser-actions-showcase.json \
  --out browser-actions-showcase.mp4 \
  --display :0
```

## License

Apache-2.0.
