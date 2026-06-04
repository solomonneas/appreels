use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

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
}

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_CARD_MS: u32 = 1500;

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
    }
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
}
