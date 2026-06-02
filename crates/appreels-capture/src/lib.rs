//! appreels screen/window capture.

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
}
