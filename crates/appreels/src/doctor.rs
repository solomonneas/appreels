use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub ok: bool,
    pub version: String,
    pub tools: Vec<ToolStatus>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub name: String,
    pub available: bool,
    pub purpose: String,
}

const REQUIRED_TOOLS: &[(&str, &str)] = &[
    ("ffmpeg", "raw capture + render"),
    ("ffprobe", "video probing for render"),
    ("xdotool", "real-cursor input control"),
    ("wmctrl", "window geometry"),
    ("obs-cmd", "OBS live-scene capture (optional)"),
];

pub fn report(version: &str, has_tool: impl Fn(&str) -> bool) -> DoctorReport {
    let tools: Vec<ToolStatus> = REQUIRED_TOOLS
        .iter()
        .map(|(name, purpose)| ToolStatus {
            name: name.to_string(),
            available: has_tool(name),
            purpose: purpose.to_string(),
        })
        .collect();
    let warnings: Vec<String> = tools
        .iter()
        .filter(|t| !t.available)
        .map(|t| format!("{} not found on PATH: {}", t.name, t.purpose))
        .collect();
    // ffmpeg + xdotool are the hard requirements for the v1 recorder path.
    let ok = tools
        .iter()
        .filter(|t| t.name == "ffmpeg" || t.name == "xdotool")
        .all(|t| t.available);
    DoctorReport {
        ok,
        version: version.to_string(),
        tools,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_requires_ffmpeg_and_xdotool() {
        let all = report("0.1.0", |_| true);
        assert!(all.ok);
        let none = report("0.1.0", |_| false);
        assert!(!none.ok);
        assert!(!none.warnings.is_empty());
        let partial = report("0.1.0", |t| t == "ffmpeg"); // missing xdotool
        assert!(!partial.ok);
    }

    #[test]
    fn probes_ffprobe() {
        let r = report("0.1.0", |_| true);
        assert!(r.tools.iter().any(|t| t.name == "ffprobe"));
    }

    #[test]
    fn report_serializes_camel_case() {
        let v = serde_json::to_value(report("0.1.0", |_| true)).unwrap();
        assert!(v["tools"][0]["available"].is_boolean());
    }
}
