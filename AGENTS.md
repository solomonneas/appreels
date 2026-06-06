# appreels - Agent Guidance

appreels records demos by driving real windows with xdotool: it launches
terminals and browsers, types, clicks, moves the mouse, and activates and
closes windows on whatever X display it targets.

## Hard rule: never drive the live desktop

**Never run `appreels record`, `perform-terminal`, or `perform-browser`
against `:0` or `:1`.** Those are the user's live session. Driving them takes
over the user's keyboard, mouse, and focus in real time. This has happened
before (2026-06-05): an agent loop ran live smoke tests on `:1` and typed
into the user's active windows for 20 minutes.

The CLI enforces this: displays `:0` and `:1` are refused at startup unless
`APPREELS_ALLOW_LIVE_DISPLAY=1` is set. Do NOT set that variable. It exists
for a human deliberately recording their own screen, not for tests, smoke
runs, or verification.

## Where to run live tests instead

This machine keeps persistent Xvfb servers running at `:95`, `:96`, and
`:99` (1600x1000x24). Use one of those:

```bash
appreels perform-terminal --script demo.json --out demo.mp4 --display :99
```

If you need a different geometry or a visible window:

```bash
Xvfb :98 -screen 0 1920x1080x24 &       # headless
Xephyr :98 -screen 1920x1080 &          # visible, contained in a window
appreels perform-terminal --script demo.json --out demo.mp4 --display :98
```

Inspect headless results from the rendered video or extracted frames
(`ffmpeg -i out.mp4 frame-%d.png`), not by watching the live screen.

## Other notes

- Verifying a recording does not require the user's desktop. If a test seems
  to need the real session, stop and ask the user first.
- Clean up demo leftovers in `/tmp` (`appreels-*` files) after test runs.
- Smoke loops that retry on failure must cap retries; do not loop a failing
  live capture.
