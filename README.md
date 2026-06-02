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

## Recording (phase 2)

```bash
# Record a window for 5 seconds, then frame it with the appshots look:
appreels record --window "Firefox" --display :0 --seconds 5 --out raw.mp4
appreels render --input raw.mp4 --out demo.mp4 --style-seed 42

# Or capture an explicit region:
appreels record --region 0,0,1280,720 --display :0 --seconds 5 --out raw.mp4
```

## License

Apache-2.0.
