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

## License

Apache-2.0.
