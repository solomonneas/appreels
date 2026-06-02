//! appreels post-render: frame recorded video with the polish-core look.

/// Basic properties of a video stream.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

/// Parse `ffprobe -show_entries stream=width,height,r_frame_rate -of default=noprint_wrappers=1`.
pub fn parse_ffprobe(output: &str) -> Option<VideoInfo> {
    let mut width = None;
    let mut height = None;
    let mut fps = None;
    for line in output.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "width" => width = value.trim().parse().ok(),
            "height" => height = value.trim().parse().ok(),
            "r_frame_rate" => fps = parse_frame_rate(value.trim()),
            _ => {}
        }
    }
    Some(VideoInfo {
        width: width?,
        height: height?,
        fps: fps?,
    })
}

fn parse_frame_rate(value: &str) -> Option<f64> {
    match value.split_once('/') {
        Some((num, den)) => {
            let n: f64 = num.parse().ok()?;
            let d: f64 = den.parse().ok()?;
            if d == 0.0 { None } else { Some(n / d) }
        }
        None => value.parse().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ffprobe_stream_info() {
        let output = "width=1280\nheight=720\nr_frame_rate=30/1\n";
        let info = parse_ffprobe(output).expect("info");
        assert_eq!(info.width, 1280);
        assert_eq!(info.height, 720);
        assert!((info.fps - 30.0).abs() < 1e-6);
    }

    #[test]
    fn parses_fractional_frame_rate() {
        let info =
            parse_ffprobe("width=640\nheight=480\nr_frame_rate=30000/1001\n").expect("info");
        assert!((info.fps - 29.97).abs() < 0.01);
    }

    #[test]
    fn rejects_missing_fields() {
        assert!(parse_ffprobe("width=640\n").is_none());
    }
}
