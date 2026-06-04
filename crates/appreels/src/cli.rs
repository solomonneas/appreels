use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use serde::Deserialize;

use crate::doctor;

#[derive(Debug, Parser)]
#[command(
    name = "appreels",
    about = "Agent-neutral polished demo-video recorder"
)]
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
    /// Record a window or screen region to a raw video via ffmpeg x11grab.
    Record {
        /// Window title to capture (resolved via xdotool).
        #[arg(long, conflicts_with = "region")]
        window: Option<String>,
        /// Explicit region as "x,y,width,height".
        #[arg(long, conflicts_with = "window")]
        region: Option<String>,
        /// X display to capture.
        #[arg(long, default_value = ":0")]
        display: String,
        #[arg(long, default_value_t = 30)]
        fps: u32,
        /// Recording duration in seconds.
        #[arg(long)]
        seconds: f64,
        #[arg(long)]
        out: PathBuf,
        /// Cursor track output path (JSONL). Defaults to <out>.cursor.jsonl.
        #[arg(long)]
        cursor_track: Option<PathBuf>,
    },
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
    /// Replay a terminal demo script, record it, and render a polished video.
    PerformTerminal {
        /// Terminal demo script JSON.
        #[arg(long)]
        script: PathBuf,
        /// Final rendered video output path.
        #[arg(long)]
        out: PathBuf,
        /// Raw terminal recording path. Defaults to <out>.raw.mp4.
        #[arg(long)]
        raw_out: Option<PathBuf>,
        /// X display to use for launching, acting, and recording.
        #[arg(long, default_value = ":0")]
        display: String,
        #[arg(long, default_value_t = 30)]
        fps: u32,
        /// Style seed (omit for the default style).
        #[arg(long)]
        style_seed: Option<u64>,
        /// Keep the launched terminal window open after recording.
        #[arg(long)]
        keep_open: bool,
    },
}

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_CARD_MS: u32 = 1500;
const DEFAULT_TERMINAL_STARTUP_MS: u32 = 800;
const DEFAULT_TERMINAL_TAIL_MS: u32 = 700;
const DEFAULT_TYPE_DELAY_MS: u32 = 28;
const DEFAULT_STEP_SETTLE_MS: u32 = 250;

pub fn run(cli: Cli) -> Result<ExitCode, Box<dyn std::error::Error>> {
    match cli.command {
        Command::Doctor => {
            let report = doctor::report(VERSION, has_command);
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(if report.ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
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
            let track_str = track
                .to_str()
                .ok_or("cursor track path must be valid UTF-8")?;
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
            let mut timeline = match &cues {
                Some(path) => {
                    let mut timeline =
                        appreels_render::Timeline::from_json(&std::fs::read_to_string(path)?)?;
                    if let Some(track) = timeline.cursor_track.as_ref() {
                        let track_path = std::path::Path::new(track);
                        if track_path.is_relative()
                            && let Some(parent) = path.parent()
                        {
                            timeline.cursor_track =
                                Some(parent.join(track_path).to_string_lossy().to_string());
                        }
                    }
                    timeline
                }
                None => appreels_render::Timeline::default(),
            };
            if let Some(text) = title {
                timeline.title_card = Some(appreels_render::Card {
                    text,
                    ms: DEFAULT_CARD_MS,
                });
            }
            if let Some(text) = outro {
                timeline.outro_card = Some(appreels_render::Card {
                    text,
                    ms: DEFAULT_CARD_MS,
                });
            }
            for spec in &caption {
                timeline.captions.push(parse_caption_flag(spec)?);
            }
            let auto_track = default_cursor_track(&input);
            let cursor_track_path = cursor_track.or_else(|| {
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
        Command::PerformTerminal {
            script,
            out,
            raw_out,
            display,
            fps,
            style_seed,
            keep_open,
        } => {
            let style = match style_seed {
                Some(seed) => polish_core::style_from_seed(seed),
                None => polish_core::style_from_seed(default_seed()),
            };
            let terminal_script = TerminalDemo::from_json(&std::fs::read_to_string(&script)?)?;
            let raw = raw_out.unwrap_or_else(|| raw_demo_path(&out));
            let cursor_track = default_cursor_track(&raw);
            let cues = raw.with_extension("cues.json");

            let terminal = launch_terminal(&display, &terminal_script)?;
            let region = appreels_capture::resolve_window_id(&terminal.window_id)?;
            activate_window(&display, &terminal.window_id)?;

            let timeline = terminal_script.to_timeline(region);
            let duration_ms = terminal_script.estimated_source_ms();
            let record_seconds = f64::from(duration_ms) / 1000.0;
            let display_for_record = display.clone();
            let raw_for_record = raw.clone();
            let track_for_record = cursor_track.clone();
            let raw_for_record_str = raw_for_record
                .to_str()
                .ok_or("raw output path must be valid UTF-8")?
                .to_string();
            let track_for_record_str = track_for_record
                .to_str()
                .ok_or("cursor track path must be valid UTF-8")?
                .to_string();
            let record_handle = std::thread::spawn(move || {
                appreels_capture::record_with_cursor(
                    &display_for_record,
                    region,
                    fps,
                    record_seconds,
                    &raw_for_record_str,
                    &track_for_record_str,
                )
            });

            std::thread::sleep(Duration::from_millis(u64::from(
                terminal_script.startup_ms().max(150),
            )));
            perform_terminal_steps(&display, &terminal.window_id, &terminal_script)?;
            record_handle
                .join()
                .map_err(|_| "recording thread panicked")??;

            std::fs::write(&cues, serde_json::to_string_pretty(&timeline)?)?;
            if !keep_open {
                let _ = close_window(&display, &terminal.window_id);
            }

            let outcome = appreels_render::render_video(
                raw.to_str().ok_or("raw output path must be valid UTF-8")?,
                out.to_str().ok_or("output path must be valid UTF-8")?,
                &style,
                &timeline,
                Some(
                    cursor_track
                        .to_str()
                        .ok_or("cursor track path must be valid UTF-8")?,
                ),
            )?;
            let report = serde_json::json!({
                "ok": true,
                "command": "performTerminal",
                "output": out,
                "rawOutput": raw,
                "cursorTrack": cursor_track,
                "cues": cues,
                "terminalTitle": terminal.title,
                "region": { "x": region.x, "y": region.y, "width": region.width, "height": region.height },
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
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TerminalDemo {
    title: Option<String>,
    cwd: Option<PathBuf>,
    shell: Option<String>,
    cols: Option<u32>,
    rows: Option<u32>,
    startup_ms: Option<u32>,
    tail_ms: Option<u32>,
    type_delay_ms: Option<u32>,
    settle_ms: Option<u32>,
    title_card_ms: Option<u32>,
    outro_card_ms: Option<u32>,
    outro: Option<String>,
    steps: Vec<TerminalStep>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum TerminalStep {
    Caption {
        text: String,
        duration_ms: Option<u32>,
        focus: Option<TerminalFocus>,
    },
    Type {
        text: String,
        ms_per_char: Option<u32>,
        caption: Option<String>,
        focus: Option<TerminalFocus>,
    },
    Run {
        command: String,
        ms_per_char: Option<u32>,
        wait_ms: Option<u32>,
        caption: Option<String>,
        focus: Option<TerminalFocus>,
    },
    Key {
        chord: String,
        caption: Option<String>,
        hold_ms: Option<u32>,
        focus: Option<TerminalFocus>,
    },
    Wait {
        ms: u32,
        caption: Option<String>,
        focus: Option<TerminalFocus>,
    },
    Zoom {
        focus: TerminalFocus,
        scale: Option<f64>,
        duration_ms: Option<u32>,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
enum TerminalFocus {
    Input,
    Output,
    Full,
    Center,
    Coord { x: f64, y: f64 },
}

#[derive(Debug, Clone)]
struct LaunchedTerminal {
    title: String,
    window_id: String,
}

impl TerminalDemo {
    fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }

    fn title(&self) -> String {
        self.title
            .clone()
            .unwrap_or_else(|| format!("appreels-terminal-{}", std::process::id()))
    }

    fn startup_ms(&self) -> u32 {
        self.startup_ms.unwrap_or(DEFAULT_TERMINAL_STARTUP_MS)
    }

    fn tail_ms(&self) -> u32 {
        self.tail_ms.unwrap_or(DEFAULT_TERMINAL_TAIL_MS)
    }

    fn type_delay_ms(&self) -> u32 {
        self.type_delay_ms.unwrap_or(DEFAULT_TYPE_DELAY_MS)
    }

    fn settle_ms(&self) -> u32 {
        self.settle_ms.unwrap_or(DEFAULT_STEP_SETTLE_MS)
    }

    fn estimated_source_ms(&self) -> u32 {
        self.startup_ms()
            + self.steps.iter().map(|s| s.estimated_ms(self)).sum::<u32>()
            + self.tail_ms()
    }

    fn to_timeline(&self, region: appreels_capture::Region) -> appreels_render::Timeline {
        let mut timeline = appreels_render::Timeline {
            title_card: Some(appreels_render::Card {
                text: self.title(),
                ms: self.title_card_ms.unwrap_or(900),
            }),
            outro_card: self.outro.as_ref().map(|text| appreels_render::Card {
                text: text.clone(),
                ms: self.outro_card_ms.unwrap_or(900),
            }),
            ..Default::default()
        };
        let mut t = self.startup_ms();
        for step in &self.steps {
            step.add_cues(&mut timeline, t, self, region);
            t += step.estimated_ms(self);
        }
        timeline
    }
}

impl TerminalStep {
    fn estimated_ms(&self, demo: &TerminalDemo) -> u32 {
        match self {
            TerminalStep::Caption { duration_ms, .. } => {
                duration_ms.unwrap_or(1400) + demo.settle_ms()
            }
            TerminalStep::Type {
                text, ms_per_char, ..
            } => {
                type_duration_ms(text, ms_per_char.unwrap_or_else(|| demo.type_delay_ms()))
                    + demo.settle_ms()
            }
            TerminalStep::Run {
                command,
                ms_per_char,
                wait_ms,
                ..
            } => {
                type_duration_ms(command, ms_per_char.unwrap_or_else(|| demo.type_delay_ms()))
                    + wait_ms.unwrap_or(1800)
                    + demo.settle_ms()
            }
            TerminalStep::Key { hold_ms, .. } => hold_ms.unwrap_or(350) + demo.settle_ms(),
            TerminalStep::Wait { ms, .. } => *ms,
            TerminalStep::Zoom { duration_ms, .. } => duration_ms.unwrap_or(1200),
        }
    }

    fn add_cues(
        &self,
        timeline: &mut appreels_render::Timeline,
        t: u32,
        demo: &TerminalDemo,
        region: appreels_capture::Region,
    ) {
        match self {
            TerminalStep::Caption {
                text,
                duration_ms,
                focus,
            } => {
                let duration = duration_ms.unwrap_or(1400);
                add_caption(timeline, t, duration, text);
                add_focus_zoom(timeline, t, duration, focus.as_ref(), region, 1.16);
            }
            TerminalStep::Type {
                text,
                ms_per_char,
                caption,
                focus,
            } => {
                let duration =
                    type_duration_ms(text, ms_per_char.unwrap_or_else(|| demo.type_delay_ms()))
                        + demo.settle_ms();
                if let Some(text) = caption {
                    add_caption(timeline, t, duration, text);
                }
                add_focus_zoom(
                    timeline,
                    t,
                    duration,
                    focus.as_ref().or(Some(&TerminalFocus::Input)),
                    region,
                    1.22,
                );
            }
            TerminalStep::Run {
                command,
                ms_per_char,
                wait_ms,
                caption,
                focus,
            } => {
                let type_ms =
                    type_duration_ms(command, ms_per_char.unwrap_or_else(|| demo.type_delay_ms()));
                let wait = wait_ms.unwrap_or(1800);
                let duration = type_ms + wait + demo.settle_ms();
                if let Some(text) = caption {
                    add_caption(timeline, t, duration, text);
                }
                add_focus_zoom(
                    timeline,
                    t,
                    type_ms + demo.settle_ms(),
                    Some(&TerminalFocus::Input),
                    region,
                    1.22,
                );
                add_focus_zoom(
                    timeline,
                    t + type_ms + demo.settle_ms(),
                    wait,
                    focus.as_ref().or(Some(&TerminalFocus::Output)),
                    region,
                    1.18,
                );
            }
            TerminalStep::Key {
                caption,
                hold_ms,
                focus,
                ..
            } => {
                let duration = hold_ms.unwrap_or(350) + demo.settle_ms();
                if let Some(text) = caption {
                    add_caption(timeline, t, duration, text);
                }
                add_focus_zoom(timeline, t, duration, focus.as_ref(), region, 1.12);
            }
            TerminalStep::Wait { ms, caption, focus } => {
                if let Some(text) = caption {
                    add_caption(timeline, t, *ms, text);
                }
                add_focus_zoom(
                    timeline,
                    t,
                    *ms,
                    focus.as_ref().or(Some(&TerminalFocus::Output)),
                    region,
                    1.16,
                );
            }
            TerminalStep::Zoom {
                focus,
                scale,
                duration_ms,
            } => {
                add_focus_zoom(
                    timeline,
                    t,
                    duration_ms.unwrap_or(1200),
                    Some(focus),
                    region,
                    scale.unwrap_or(1.2),
                );
            }
        }
    }
}

fn add_caption(
    timeline: &mut appreels_render::Timeline,
    start_ms: u32,
    duration_ms: u32,
    text: &str,
) {
    if text.trim().is_empty() || duration_ms == 0 {
        return;
    }
    timeline.captions.push(appreels_render::Caption {
        start_ms,
        end_ms: start_ms.saturating_add(duration_ms),
        text: text.to_string(),
    });
}

fn add_focus_zoom(
    timeline: &mut appreels_render::Timeline,
    start_ms: u32,
    duration_ms: u32,
    focus: Option<&TerminalFocus>,
    region: appreels_capture::Region,
    default_scale: f64,
) {
    let Some(focus) = focus else {
        return;
    };
    if matches!(focus, TerminalFocus::Full) || duration_ms == 0 {
        return;
    }
    let (x, y) = focus_point(focus, region);
    timeline.zooms.push(appreels_render::ZoomCue {
        start_ms,
        end_ms: start_ms.saturating_add(duration_ms),
        x,
        y,
        scale: default_scale,
    });
}

fn focus_point(focus: &TerminalFocus, region: appreels_capture::Region) -> (f64, f64) {
    let w = f64::from(region.width);
    let h = f64::from(region.height);
    match focus {
        TerminalFocus::Input => (w * 0.48, h * 0.82),
        TerminalFocus::Output => (w * 0.50, h * 0.42),
        TerminalFocus::Full | TerminalFocus::Center => (w * 0.50, h * 0.50),
        TerminalFocus::Coord { x, y } => (*x, *y),
    }
}

fn type_duration_ms(text: &str, ms_per_char: u32) -> u32 {
    (text.chars().count() as u32)
        .saturating_mul(ms_per_char)
        .max(ms_per_char)
}

fn launch_terminal(
    display: &str,
    demo: &TerminalDemo,
) -> Result<LaunchedTerminal, Box<dyn std::error::Error>> {
    let title = unique_terminal_title(&demo.title());
    let shell = demo
        .shell
        .clone()
        .unwrap_or_else(|| "bash --noprofile --norc".to_string());
    let geometry = format!("{}x{}", demo.cols.unwrap_or(100), demo.rows.unwrap_or(28));
    let cwd = demo.cwd.clone().unwrap_or(std::env::current_dir()?);

    if has_command("gnome-terminal") {
        let mut cmd = std::process::Command::new("gnome-terminal");
        cmd.env("DISPLAY", display)
            .arg("--hide-menubar")
            .arg(format!("--geometry={geometry}"))
            .arg(format!("--title={title}"))
            .arg(format!("--working-directory={}", cwd.to_string_lossy()))
            .arg("--")
            .arg("bash")
            .arg("-lc")
            .arg(shell)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
    } else if has_command("xfce4-terminal") {
        let mut cmd = std::process::Command::new("xfce4-terminal");
        cmd.env("DISPLAY", display)
            .arg("--disable-server")
            .arg("--title")
            .arg(&title)
            .arg("--initial-title")
            .arg(&title)
            .arg("--dynamic-title-mode")
            .arg("none")
            .arg("--geometry")
            .arg(&geometry)
            .arg("--working-directory")
            .arg(&cwd)
            .arg("--hide-menubar")
            .arg("--hide-toolbar")
            .arg("--hide-scrollbar")
            .arg("--color-bg")
            .arg("#000000")
            .arg("--color-text")
            .arg("#f2f2f2")
            .arg("--command")
            .arg(&shell)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
    } else {
        return Err("no supported terminal found (need xfce4-terminal or gnome-terminal)".into());
    }

    let window_id = wait_for_window(display, &title, Duration::from_secs(5))?;
    Ok(LaunchedTerminal { title, window_id })
}

fn unique_terminal_title(base: &str) -> String {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{base} - appreels-{}-{stamp}", std::process::id())
}

fn wait_for_window(
    display: &str,
    title: &str,
    timeout: Duration,
) -> Result<String, Box<dyn std::error::Error>> {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Ok(out) = std::process::Command::new("xdotool")
            .env("DISPLAY", display)
            .args(["search", "--name", title])
            .output()
            && out.status.success()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            if let Some(id) = text.lines().next().filter(|s| !s.trim().is_empty()) {
                return Ok(id.trim().to_string());
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!("timed out waiting for terminal window {title:?}").into())
}

fn activate_window(display: &str, window_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    run_xdotool(display, &["windowactivate", "--sync", window_id])?;
    Ok(())
}

fn close_window(display: &str, window_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    run_xdotool(display, &["windowclose", window_id])?;
    Ok(())
}

fn perform_terminal_steps(
    display: &str,
    window_id: &str,
    demo: &TerminalDemo,
) -> Result<(), Box<dyn std::error::Error>> {
    activate_window(display, window_id)?;
    for step in &demo.steps {
        match step {
            TerminalStep::Caption { duration_ms, .. } => {
                std::thread::sleep(Duration::from_millis(u64::from(
                    duration_ms.unwrap_or(1400),
                )));
            }
            TerminalStep::Type {
                text, ms_per_char, ..
            } => {
                type_into_terminal(
                    display,
                    text,
                    ms_per_char.unwrap_or_else(|| demo.type_delay_ms()),
                )?;
                std::thread::sleep(Duration::from_millis(u64::from(demo.settle_ms())));
            }
            TerminalStep::Run {
                command,
                ms_per_char,
                wait_ms,
                ..
            } => {
                type_into_terminal(
                    display,
                    command,
                    ms_per_char.unwrap_or_else(|| demo.type_delay_ms()),
                )?;
                run_xdotool(display, &["key", "--clearmodifiers", "Return"])?;
                std::thread::sleep(Duration::from_millis(u64::from(
                    wait_ms.unwrap_or(1800) + demo.settle_ms(),
                )));
            }
            TerminalStep::Key { chord, hold_ms, .. } => {
                run_xdotool(display, &["key", "--clearmodifiers", chord])?;
                std::thread::sleep(Duration::from_millis(u64::from(
                    hold_ms.unwrap_or(350) + demo.settle_ms(),
                )));
            }
            TerminalStep::Wait { ms, .. } => {
                std::thread::sleep(Duration::from_millis(u64::from(*ms)));
            }
            TerminalStep::Zoom { duration_ms, .. } => {
                std::thread::sleep(Duration::from_millis(u64::from(
                    duration_ms.unwrap_or(1200),
                )));
            }
        }
    }
    std::thread::sleep(Duration::from_millis(u64::from(demo.tail_ms())));
    Ok(())
}

fn type_into_terminal(
    display: &str,
    text: &str,
    ms_per_char: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    run_xdotool(
        display,
        &[
            "type",
            "--clearmodifiers",
            "--delay",
            &ms_per_char.to_string(),
            "--",
            text,
        ],
    )?;
    Ok(())
}

fn run_xdotool(display: &str, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let out = std::process::Command::new("xdotool")
        .env("DISPLAY", display)
        .args(args)
        .output()?;
    if !out.status.success() {
        return Err(format!(
            "xdotool {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr).trim()
        )
        .into());
    }
    Ok(())
}

fn raw_demo_path(out: &Path) -> PathBuf {
    let stem = out
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("demo");
    out.with_file_name(format!("{stem}.raw.mp4"))
}

fn parse_region(spec: &str) -> Result<appreels_capture::Region, Box<dyn std::error::Error>> {
    let parts: Vec<&str> = spec.split(',').map(str::trim).collect();
    if parts.len() != 4 {
        return Err("region must be \"x,y,width,height\"".into());
    }
    Ok(appreels_capture::Region {
        x: parts[0].parse()?,
        y: parts[1].parse()?,
        width: parts[2].parse()?,
        height: parts[3].parse()?,
    })
}

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
    Ok(appreels_render::Caption {
        start_ms,
        end_ms,
        text,
    })
}

/// `<out>.cursor.jsonl` next to a recording or source video.
fn default_cursor_track(out: &std::path::Path) -> PathBuf {
    out.with_extension("cursor.jsonl")
}

// A fixed default seed keeps `render` without --style-seed deterministic.
fn default_seed() -> u64 {
    0x617070_7265656c // "appreel" bytes; arbitrary but stable
}

fn has_command(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(program).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_region_spec() {
        let r = parse_region("10, 20, 640, 480").expect("region");
        assert_eq!((r.x, r.y, r.width, r.height), (10, 20, 640, 480));
    }

    #[test]
    fn rejects_bad_region_spec() {
        assert!(parse_region("1,2,3").is_err());
    }

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

    #[test]
    fn derives_raw_demo_path_from_output() {
        let p = raw_demo_path(std::path::Path::new("/tmp/demo.mp4"));
        assert_eq!(p, std::path::PathBuf::from("/tmp/demo.raw.mp4"));
    }

    #[test]
    fn parses_terminal_demo_script() {
        let json = r#"{
            "title": "Terminal showcase",
            "startupMs": 100,
            "tailMs": 200,
            "typeDelayMs": 10,
            "steps": [
                { "type": "caption", "text": "Start", "durationMs": 500 },
                { "type": "run", "command": "echo hello", "waitMs": 700, "caption": "Run it" },
                { "type": "wait", "ms": 900, "focus": "output", "caption": "Generating" }
            ]
        }"#;
        let demo = TerminalDemo::from_json(json).expect("script");
        assert_eq!(demo.title(), "Terminal showcase");
        assert_eq!(demo.steps.len(), 3);
        assert!(demo.estimated_source_ms() >= 2400);
    }

    #[test]
    fn terminal_script_generates_captions_and_zooms() {
        let json = r#"{
            "title": "Terminal showcase",
            "startupMs": 100,
            "tailMs": 200,
            "steps": [
                { "type": "type", "text": "cargo run", "caption": "Type the command" },
                { "type": "wait", "ms": 1000, "focus": "output", "caption": "Watch output stream in" }
            ]
        }"#;
        let demo = TerminalDemo::from_json(json).expect("script");
        let timeline = demo.to_timeline(appreels_capture::Region {
            x: 10,
            y: 20,
            width: 800,
            height: 500,
        });
        assert_eq!(timeline.title_card.unwrap().text, "Terminal showcase");
        assert_eq!(timeline.captions.len(), 2);
        assert_eq!(timeline.zooms.len(), 2);
        assert!(timeline.zooms[0].y > timeline.zooms[1].y);
    }
}
