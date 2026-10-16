// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `/bug`: a bug report form for anyone. It makes a post in the forum channel `bug-reports` and
//! nothing else: nothing reaches GitHub until a moderator reviews the post with `/track`.
//!
//! A Discord modal holds at most five fields, so the platform's choices carry the system version
//! too, and "what you expected" is asked for in the description of "What happened".

use serde_json::{Value, json};

use crate::interaction::{Attachment, Interaction};
use crate::reply::shorten;

pub const NAME: &str = "bug";
pub const MODAL_ID: &str = "bug:submit";

const TITLE: &str = "title";
const WHAT: &str = "what";
const STEPS: &str = "steps";
const PLATFORM: &str = "platform";
const SCREENSHOTS: &str = "screenshots";

/// A forum post's title holds at most 100 characters.
pub const TITLE_LIMIT: usize = 100;
const WHAT_LIMIT: usize = 1200;
const STEPS_LIMIT: usize = 600;
pub const MAX_SCREENSHOTS: usize = 5;

/// One choice of the platform menu: its value, what the reporter sees, and the forum tag it gets.
pub struct Platform {
    pub value: &'static str,
    pub label: &'static str,
    pub tag: &'static str,
}

pub const PLATFORMS: [Platform; 11] = [
    Platform {
        value: "windows-11",
        label: "Windows 11",
        tag: "Windows",
    },
    Platform {
        value: "windows-10",
        label: "Windows 10",
        tag: "Windows",
    },
    Platform {
        value: "windows",
        label: "Windows (other, or not sure)",
        tag: "Windows",
    },
    Platform {
        value: "macos-26",
        label: "macOS 26 Tahoe or newer",
        tag: "Mac",
    },
    Platform {
        value: "macos-15",
        label: "macOS 15 Sequoia",
        tag: "Mac",
    },
    Platform {
        value: "macos-14",
        label: "macOS 14 Sonoma or older",
        tag: "Mac",
    },
    Platform {
        value: "macos",
        label: "macOS (not sure which)",
        tag: "Mac",
    },
    Platform {
        value: "linux-debian",
        label: "Linux: Ubuntu, Debian, Mint",
        tag: "Linux",
    },
    Platform {
        value: "linux-fedora",
        label: "Linux: Fedora",
        tag: "Linux",
    },
    Platform {
        value: "linux-arch",
        label: "Linux: Arch, Manjaro",
        tag: "Linux",
    },
    Platform {
        value: "linux",
        label: "Linux (other)",
        tag: "Linux",
    },
];

pub fn definition() -> Value {
    json!({
        "name": NAME,
        "type": 1,
        "description": "Report a bug in Suhina: a form that posts it in #bug-reports",
        // Server channels only, from the server's installation.
        "contexts": [0],
        "integration_types": [0],
    })
}

fn label(text: &str, description: Option<&str>, component: Value) -> Value {
    match description {
        Some(description) => {
            json!({ "type": 18, "label": text, "description": description, "component": component })
        }
        None => json!({ "type": 18, "label": text, "component": component }),
    }
}

/// The form.
pub fn modal() -> Value {
    let options: Vec<Value> = PLATFORMS
        .iter()
        .map(|p| json!({ "label": p.label, "value": p.value }))
        .collect();
    json!({
        "type": 9,
        "data": {
            "custom_id": MODAL_ID,
            "title": "Report a bug in Suhina",
            "components": [
                label("Title", Some("A short summary, like a headline"),
                    json!({ "type": 4, "custom_id": TITLE, "style": 1, "max_length": TITLE_LIMIT, "required": true })),
                label("What happened", Some("What you did, what happened, what you expected. Your Suhina version helps."),
                    json!({ "type": 4, "custom_id": WHAT, "style": 2, "max_length": WHAT_LIMIT, "required": true })),
                label("Steps to reproduce (optional, helps a lot)", Some("1. Open … 2. Click … 3. …"),
                    json!({ "type": 4, "custom_id": STEPS, "style": 2, "max_length": STEPS_LIMIT, "required": false })),
                label("Platform", None,
                    json!({ "type": 3, "custom_id": PLATFORM, "options": options, "min_values": 1, "max_values": 1, "required": true })),
                label("Screenshots (optional)", Some("Up to five images"),
                    json!({ "type": 19, "custom_id": SCREENSHOTS, "min_values": 0, "max_values": MAX_SCREENSHOTS, "required": false, "file_types": ["image"] })),
            ],
        },
    })
}

/// A submitted form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BugReport {
    pub title: String,
    pub what: String,
    pub steps: Option<String>,
    pub platform: &'static Platform,
    pub screenshots: Vec<Attachment>,
}

impl std::fmt::Debug for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.value)
    }
}

impl PartialEq for Platform {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for Platform {}

impl BugReport {
    /// Reads the form; the error is the answer for the reporter.
    pub fn from_submit(interaction: &Interaction) -> Result<Self, &'static str> {
        let title = interaction
            .modal_value(TITLE)
            .ok_or("The title is missing.")?;
        let what = interaction
            .modal_value(WHAT)
            .ok_or("Please say what happened.")?;
        let chosen = interaction.modal_values(PLATFORM);
        let platform = PLATFORMS
            .iter()
            .find(|p| chosen.first().map(String::as_str) == Some(p.value))
            .ok_or("Please choose your platform.")?;
        let screenshots: Vec<Attachment> = interaction
            .uploaded(&interaction.modal_values(SCREENSHOTS))
            .into_iter()
            .take(MAX_SCREENSHOTS)
            .collect();
        Ok(Self {
            title: shorten(title.trim(), TITLE_LIMIT),
            what: shorten(what.trim(), WHAT_LIMIT),
            steps: interaction
                .modal_value(STEPS)
                .map(|steps| shorten(steps.trim(), STEPS_LIMIT)),
            platform,
            screenshots,
        })
    }

    /// The text of the forum post. Names the reporter without pinging (the post is sent with no
    /// mentions parsed), so they can follow the thread. `/track` leaves that line out of issues.
    pub fn post_content(&self, reporter_id: &str) -> String {
        let mut parts = vec![
            format!("{REPORTED_BY}<@{reporter_id}>"),
            format!("**What happened**\n{}", self.what),
        ];
        if let Some(steps) = &self.steps {
            parts.push(format!("**Steps to reproduce**\n{steps}"));
        }
        parts.push(format!("**Platform** {}", self.platform.label));
        shorten(&parts.join("\n\n"), crate::reply::CONTENT_LIMIT)
    }
}

/// The first line of a post made by `/bug`.
pub const REPORTED_BY: &str = "Reported by ";

#[cfg(test)]
mod tests {
    use super::*;

    fn submit(fields: &str) -> Interaction {
        let body = format!(
            r#"{{"type":5,"data":{{"custom_id":"bug:submit","components":[{fields}],
               "resolved":{{"attachments":{{"7":{{"id":"7","filename":"s.png","content_type":"image/png","size":5,"url":"https://cdn/s"}}}}}}}}}}"#
        );
        Interaction::parse(body.as_bytes()).unwrap()
    }

    fn text(id: &str, value: &str) -> String {
        format!(r#"{{"type":18,"component":{{"type":4,"custom_id":"{id}","value":"{value}"}}}}"#)
    }

    fn values(id: &str, kind: u8, values: &str) -> String {
        format!(
            r#"{{"type":18,"component":{{"type":{kind},"custom_id":"{id}","values":[{values}]}}}}"#
        )
    }

    #[test]
    fn the_modal_fits_discord() {
        let modal = modal();
        let fields = modal["data"]["components"].as_array().unwrap();
        assert!(fields.len() <= 5);
        assert!(modal["data"]["title"].as_str().unwrap().len() <= 45);
        for field in fields {
            assert_eq!(field["type"], 18);
            assert!(field["label"].as_str().unwrap().chars().count() <= 45);
            assert!(
                field
                    .get("description")
                    .is_none_or(|d| d.as_str().unwrap().chars().count() <= 100)
            );
        }
        assert!(PLATFORMS.len() <= 25);
        assert!(
            PLATFORMS
                .iter()
                .all(|p| ["Mac", "Windows", "Linux"].contains(&p.tag))
        );
    }

    #[test]
    fn reads_a_full_report() {
        let fields = [
            text("title", "Clip does not loop"),
            text("what", "It stops."),
            text("steps", "1. Play"),
            values("platform", 3, r#""macos-15""#),
            values("screenshots", 19, r#""7""#),
        ]
        .join(",");
        let report = BugReport::from_submit(&submit(&fields)).unwrap();
        assert_eq!(report.title, "Clip does not loop");
        assert_eq!(report.platform.tag, "Mac");
        assert_eq!(report.screenshots.len(), 1);
        let content = report.post_content("42");
        assert!(content.starts_with("Reported by <@42>"));
        assert!(content.contains("**Steps to reproduce**\n1. Play"));
        assert!(content.ends_with("**Platform** macOS 15 Sequoia"));
    }

    #[test]
    fn optional_parts_are_left_out() {
        let fields = [
            text("title", "T"),
            text("what", "W"),
            text("steps", ""),
            values("platform", 3, r#""linux""#),
            values("screenshots", 19, ""),
        ]
        .join(",");
        let report = BugReport::from_submit(&submit(&fields)).unwrap();
        assert_eq!(report.steps, None);
        assert!(report.screenshots.is_empty());
        assert!(!report.post_content("1").contains("Steps"));
    }

    #[test]
    fn refuses_a_report_without_the_required_parts() {
        let no_platform = [text("title", "T"), text("what", "W")].join(",");
        assert_eq!(
            BugReport::from_submit(&submit(&no_platform)),
            Err("Please choose your platform.")
        );
        let unknown = [
            text("title", "T"),
            text("what", "W"),
            values("platform", 3, r#""amiga""#),
        ]
        .join(",");
        assert!(BugReport::from_submit(&submit(&unknown)).is_err());
        let no_what = [text("title", "T"), values("platform", 3, r#""linux""#)].join(",");
        assert_eq!(
            BugReport::from_submit(&submit(&no_what)),
            Err("Please say what happened.")
        );
    }
}
