//! appreels render cue model: cards, captions, zooms, cursor track.

use serde::{Deserialize, Serialize};

/// The full set of render cues for one clip.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Timeline {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_track: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_card: Option<Card>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outro_card: Option<Card>,
    #[serde(default)]
    pub captions: Vec<Caption>,
    #[serde(default)]
    pub zooms: Vec<ZoomCue>,
}

/// A full-canvas title or outro card.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub text: String,
    pub ms: u32,
}

/// A lower-third caption active over `[start_ms, end_ms)`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Caption {
    pub start_ms: u32,
    pub end_ms: u32,
    pub text: String,
}

/// A zoom toward `(x, y)` (source-window px) at `scale`, eased in/hold/out.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ZoomCue {
    pub start_ms: u32,
    pub end_ms: u32,
    pub x: f64,
    pub y: f64,
    pub scale: f64,
}

/// A single pointer sample, window-relative px, `t_ms` from capture start.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CursorSample {
    pub t_ms: u32,
    pub x: f64,
    pub y: f64,
}

impl Timeline {
    /// Parse a cue file's JSON.
    pub fn from_json(s: &str) -> Result<Timeline, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// Parse a `cursor.jsonl` track (one `CursorSample` JSON object per line).
/// Blank and unparseable lines are skipped.
pub fn parse_cursor_track(s: &str) -> Vec<CursorSample> {
    s.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<CursorSample>(l).ok())
        .collect()
}

/// Linearly interpolate the cursor position at `t_ms`. Clamps to the first/last
/// sample outside the track's range. `None` if there are no samples.
/// Assumes `samples` is sorted ascending by `t_ms` (as written by capture).
pub fn cursor_at(samples: &[CursorSample], t_ms: f64) -> Option<(f64, f64)> {
    let first = samples.first()?;
    if t_ms <= first.t_ms as f64 {
        return Some((first.x, first.y));
    }
    let last = samples.last()?;
    if t_ms >= last.t_ms as f64 {
        return Some((last.x, last.y));
    }
    for w in samples.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        let (ta, tb) = (a.t_ms as f64, b.t_ms as f64);
        if t_ms >= ta && t_ms <= tb {
            let f = if tb > ta {
                (t_ms - ta) / (tb - ta)
            } else {
                0.0
            };
            return Some((a.x + (b.x - a.x) * f, a.y + (b.y - a.y) * f));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_json_round_trips() {
        let json = r#"{
            "cursorTrack": "raw.cursor.jsonl",
            "titleCard": { "text": "Hi", "ms": 1500 },
            "captions": [ { "startMs": 0, "endMs": 1800, "text": "Open the menu" } ],
            "zooms": [ { "startMs": 2000, "endMs": 5000, "x": 420, "y": 300, "scale": 1.8 } ]
        }"#;
        let tl = Timeline::from_json(json).expect("parse");
        assert_eq!(tl.cursor_track.as_deref(), Some("raw.cursor.jsonl"));
        assert_eq!(tl.title_card.as_ref().unwrap().ms, 1500);
        assert_eq!(tl.captions[0].text, "Open the menu");
        assert_eq!(tl.zooms[0].scale, 1.8);
        assert!(tl.outro_card.is_none());
    }

    #[test]
    fn empty_timeline_is_default() {
        let tl = Timeline::from_json("{}").expect("parse");
        assert!(tl.captions.is_empty());
        assert!(tl.zooms.is_empty());
        assert!(tl.title_card.is_none());
    }

    #[test]
    fn parses_cursor_track_skipping_blank_lines() {
        let s = "{\"tMs\":0,\"x\":10,\"y\":20}\n\n{\"tMs\":100,\"x\":30,\"y\":40}\n";
        let samples = parse_cursor_track(s);
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[1].t_ms, 100);
        assert_eq!(samples[1].x, 30.0);
    }

    #[test]
    fn cursor_at_interpolates_between_samples() {
        let samples = vec![
            CursorSample {
                t_ms: 0,
                x: 0.0,
                y: 0.0,
            },
            CursorSample {
                t_ms: 100,
                x: 100.0,
                y: 200.0,
            },
        ];
        let (x, y) = cursor_at(&samples, 50.0).expect("interp");
        assert!((x - 50.0).abs() < 1e-6);
        assert!((y - 100.0).abs() < 1e-6);
    }

    #[test]
    fn cursor_at_clamps_past_the_ends() {
        let samples = vec![
            CursorSample {
                t_ms: 10,
                x: 5.0,
                y: 5.0,
            },
            CursorSample {
                t_ms: 20,
                x: 9.0,
                y: 9.0,
            },
        ];
        assert_eq!(cursor_at(&samples, 0.0), Some((5.0, 5.0)));
        assert_eq!(cursor_at(&samples, 999.0), Some((9.0, 9.0)));
        assert_eq!(cursor_at(&[], 5.0), None);
    }
}
