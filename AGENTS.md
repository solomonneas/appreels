# appreels - Agent Guidance

appreels is a Rust workspace (5 crates) that records demo videos by driving
real windows with xdotool: it launches terminals and browsers, types, clicks,
moves the mouse, and activates and closes windows on whatever X display it
targets.

## Definition of Done

```bash
./scripts/verify
```

It runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
`cargo test`, the same gates as CI. Run it before reporting any code change as
complete. Report the actual result, paste failures verbatim, and never claim a
pass you did not observe.

## Hard prohibitions

- Never push with `--no-verify` if a pre-push hook exists.
- Never weaken, skip, or delete a failing test to get to green. Fix the code
  or report the failure.
- Never invent commands. If a command is not in this file, the README, or a
  manifest, confirm it exists before running it.
- When blocked, report the exact blocker (command, full error output). Do not
  work around it silently.

## Hard rule: never drive the live desktop

Trigger: any run of `appreels record`, `perform-terminal`, or
`perform-browser`.

Rule: never target display `:0` or `:1`. Those are the user's live session.
Driving them takes over the user's keyboard, mouse, and focus in real time.
This happened on 2026-06-05: an agent loop ran live smoke tests on `:1` and
typed into the user's active windows for 20 minutes.

The CLI refuses `:0` and `:1` at startup unless `APPREELS_ALLOW_LIVE_DISPLAY=1`
is set. Do NOT set that variable. It exists for a human deliberately recording
their own screen, never for tests, smoke runs, or verification.

Instead: use a headless display (next section).

## Headless displays for live tests

Persistent Xvfb servers run at `:95`, `:96`, and `:99` (1600x1000x24):

```bash
appreels perform-terminal --script demo.json --out demo.mp4 --display :99
```

If you need a different geometry or a visible window:

```bash
Xvfb :98 -screen 0 1920x1080x24 &       # headless
Xephyr :98 -screen 1920x1080 &          # visible, contained in a window
```

Inspect headless results from the rendered video or extracted frames
(`ffmpeg -i out.mp4 frame-%d.png`), not by watching the live screen.

## Test discipline

- Verifying a recording never requires the user's desktop. If a test seems to
  need the real session, stop and ask the user first.
- Cap retries in smoke loops; do not loop a failing live capture.
- Clean up demo leftovers in `/tmp` (`appreels-*` files) after test runs.

## Memory Handoff

At the end of any task that produces durable knowledge (root causes, gotchas,
workflow changes), write a handoff to `.claude/memory-handoffs/` using its
`TEMPLATE.md`. The memory owner ingests these into canonical memory.
