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
    },
    /// Frame a recorded video with the appshots look.
    Render {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Style seed (omit for the default style).
        #[arg(long)]
        style_seed: Option<u64>,
    },
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

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
        } => {
            let resolved = match (window, region) {
                (Some(title), _) => appreels_capture::resolve_window(&title)?,
                (_, Some(spec)) => parse_region(&spec)?,
                (None, None) => return Err("provide --window or --region".into()),
            };
            let out_str = out.to_str().ok_or("output path must be valid UTF-8")?;
            appreels_capture::record(&display, resolved, fps, seconds, out_str)?;
            let report = serde_json::json!({
                "ok": true,
                "command": "record",
                "output": out,
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
        } => {
            let style = match style_seed {
                Some(seed) => polish_core::style_from_seed(seed),
                None => polish_core::style_from_seed(default_seed()),
            };
            let input_str = input.to_str().ok_or("input path must be valid UTF-8")?;
            let out_str = out.to_str().ok_or("output path must be valid UTF-8")?;
            let info = appreels_render::frame_video(input_str, out_str, &style)?;
            let report = serde_json::json!({
                "ok": true,
                "command": "render",
                "output": out,
                "styleSeed": style.seed,
                "palette": style.palette_name,
                "source": { "width": info.width, "height": info.height, "fps": info.fps },
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
}
