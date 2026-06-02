//! appreels demo-script format.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The JSON schema for a [`Script`], for `appreels schema`.
pub fn script_schema() -> schemars::schema::RootSchema {
    schemars::schema_for!(Script)
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Script {
    pub version: String,
    pub title: String,
    pub target: Target,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<Viewport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defaults: Option<Defaults>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Target {
    Browser {
        url: String,
    },
    Desktop {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        app: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window_title: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Defaults {
    #[serde(default)]
    pub move_ms: Option<u32>,
    #[serde(default)]
    pub settle_ms: Option<u32>,
    #[serde(default)]
    pub easing: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Step {
    Narrate {
        text: String,
    },
    Caption {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u32>,
    },
    Move {
        target: StepTarget,
    },
    Click {
        target: StepTarget,
    },
    Type {
        target: StepTarget,
        text: String,
    },
    Key {
        chord: String,
    },
    Wait {
        ms: u32,
    },
    Scroll {
        target: StepTarget,
    },
    Zoom {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<StepTarget>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hold_ms: Option<u32>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        reset: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum StepTarget {
    Selector(String),
    Coord { x: i32, y: i32 },
    ImageAnchor(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_browser_script() {
        let json = r##"{
            "version": "0.1.0",
            "title": "Create a new project",
            "target": { "kind": "browser", "url": "https://app.example.com" },
            "steps": [
                { "type": "narrate", "text": "Hello." },
                { "type": "click", "target": { "selector": "#new" } },
                { "type": "type", "target": { "selector": "input" }, "text": "Demo" },
                { "type": "zoom", "reset": true }
            ]
        }"##;
        let script: Script = serde_json::from_str(json).expect("parse");
        assert_eq!(script.steps.len(), 4);
        let back = serde_json::to_string(&script).expect("serialize");
        let reparsed: Script = serde_json::from_str(&back).expect("reparse");
        assert_eq!(reparsed.title, "Create a new project");
    }

    #[test]
    fn target_uses_camel_case_tag() {
        let t = Target::Browser {
            url: "https://x".into(),
        };
        let v = serde_json::to_value(t).unwrap();
        assert_eq!(v["kind"], "browser");
    }

    #[test]
    fn schema_generates() {
        let schema = script_schema();
        let v = serde_json::to_value(&schema).unwrap();
        assert!(v["properties"]["steps"].is_object());
    }
}
