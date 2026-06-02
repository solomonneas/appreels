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
