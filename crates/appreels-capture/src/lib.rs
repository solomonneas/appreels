//! appreels screen/window capture.

use std::process::Command;

use thiserror::Error;

/// A rectangle on the X11 screen, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Parse the output of `xdotool getwindowgeometry --shell <id>`.
pub fn parse_xdotool_geometry(output: &str) -> Option<Region> {
    let mut x = None;
    let mut y = None;
    let mut width = None;
    let mut height = None;
    for line in output.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "X" => x = value.trim().parse().ok(),
            "Y" => y = value.trim().parse().ok(),
            "WIDTH" => width = value.trim().parse().ok(),
            "HEIGHT" => height = value.trim().parse().ok(),
            _ => {}
        }
    }
    Some(Region {
        x: x?,
        y: y?,
        width: width?,
        height: height?,
    })
}

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("failed to run `{program}`: {source}")]
    Spawn {
        program: String,
        source: std::io::Error,
    },
    #[error("`{program}` exited with status {status}: {stderr}")]
    Failed {
        program: String,
        status: std::process::ExitStatus,
        stderr: String,
    },
    #[error("could not resolve window geometry for {0:?}")]
    WindowNotFound(String),
}

/// Build the ffmpeg argument vector for an x11grab recording.
pub fn x11grab_args(
    display: &str,
    region: Region,
    fps: u32,
    seconds: f64,
    output: &str,
) -> Vec<String> {
    vec![
        "-y".to_string(),
        "-f".to_string(),
        "x11grab".to_string(),
        "-framerate".to_string(),
        fps.to_string(),
        "-video_size".to_string(),
        format!("{}x{}", region.width, region.height),
        "-i".to_string(),
        format!("{display}+{},{}", region.x, region.y),
        "-t".to_string(),
        format!("{seconds:.3}"),
        "-c:v".to_string(),
        "libx264".to_string(),
        "-preset".to_string(),
        "ultrafast".to_string(),
        "-pix_fmt".to_string(),
        "yuv420p".to_string(),
        output.to_string(),
    ]
}

/// Resolve a window's screen rect by title via xdotool.
pub fn resolve_window(title: &str) -> Result<Region, CaptureError> {
    let search = run("xdotool", &["search", "--name", title])?;
    let id = search
        .lines()
        .next()
        .ok_or_else(|| CaptureError::WindowNotFound(title.to_string()))?;
    let geom = run("xdotool", &["getwindowgeometry", "--shell", id])?;
    parse_xdotool_geometry(&geom).ok_or_else(|| CaptureError::WindowNotFound(title.to_string()))
}

/// Record a region of the X display to `output` for `seconds`, via ffmpeg x11grab.
pub fn record(
    display: &str,
    region: Region,
    fps: u32,
    seconds: f64,
    output: &str,
) -> Result<(), CaptureError> {
    let args = x11grab_args(display, region, fps, seconds, output);
    run_status(
        "ffmpeg",
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    )
}

fn run(program: &str, args: &[&str]) -> Result<String, CaptureError> {
    let out = Command::new(program)
        .args(args)
        .output()
        .map_err(|source| CaptureError::Spawn {
            program: program.to_string(),
            source,
        })?;
    if !out.status.success() {
        return Err(CaptureError::Failed {
            program: program.to_string(),
            status: out.status,
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn run_status(program: &str, args: &[&str]) -> Result<(), CaptureError> {
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|source| CaptureError::Spawn {
            program: program.to_string(),
            source,
        })?;
    if !status.success() {
        return Err(CaptureError::Failed {
            program: program.to_string(),
            status,
            stderr: String::new(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_xdotool_shell_geometry() {
        let output = "WINDOW=12345\nX=100\nY=64\nWIDTH=1280\nHEIGHT=720\nSCREEN=0\n";
        let region = parse_xdotool_geometry(output).expect("geometry");
        assert_eq!(
            region,
            Region {
                x: 100,
                y: 64,
                width: 1280,
                height: 720
            }
        );
    }

    #[test]
    fn rejects_incomplete_geometry() {
        assert!(parse_xdotool_geometry("X=1\nY=2\n").is_none());
    }

    #[test]
    fn x11grab_args_include_geometry_and_input() {
        let region = Region {
            x: 10,
            y: 20,
            width: 640,
            height: 480,
        };
        let args = x11grab_args(":1", region, 30, 2.5, "/tmp/out.mp4");
        assert!(args.windows(2).any(|w| w == ["-f", "x11grab"]));
        assert!(args.windows(2).any(|w| w == ["-video_size", "640x480"]));
        assert!(args.windows(2).any(|w| w == ["-i", ":1+10,20"]));
        assert!(args.windows(2).any(|w| w == ["-t", "2.500"]));
        assert!(args.windows(2).any(|w| w == ["-framerate", "30"]));
        assert_eq!(args.last().unwrap(), "/tmp/out.mp4");
    }

    #[test]
    #[ignore = "needs ffmpeg and an X display"]
    fn records_a_short_clip() {
        let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
        let out = std::env::temp_dir().join("appreels-capture-test.mp4");
        let region = Region {
            x: 0,
            y: 0,
            width: 320,
            height: 240,
        };
        record(&display, region, 10, 1.0, out.to_str().unwrap()).expect("record");
        assert!(out.metadata().expect("file").len() > 0);
    }
}
