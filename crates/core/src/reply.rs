// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The bodies the bot sends to Discord. Every message carries `allowed_mentions: {parse: []}`,
//! so text taken from GitHub or from a user can never ping anyone.

use serde_json::{Value, json};

/// Interaction response types.
const PONG: u8 = 1;
const CHANNEL_MESSAGE: u8 = 4;
const DEFERRED_CHANNEL_MESSAGE: u8 = 5;
const MODAL: u8 = 9;

/// Message flag: only the person who used the command sees it.
const EPHEMERAL: u64 = 1 << 6;

/// Discord's limit for a message's content.
pub const CONTENT_LIMIT: usize = 2000;

/// No mentions are ever parsed from the text.
fn no_mentions() -> Value {
    json!({ "parse": [] })
}

/// `text` cut to at most `limit` characters, ending with `…` when it was cut.
pub fn shorten(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let kept: String = text.chars().take(limit.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// The answer to Discord's PING.
pub fn pong() -> Value {
    json!({ "type": PONG })
}

/// A message, in the channel or only for the user.
pub fn message(content: &str, ephemeral: bool) -> Value {
    json!({
        "type": CHANNEL_MESSAGE,
        "data": message_data(content, ephemeral),
    })
}

/// An answer that only the user sees.
pub fn ephemeral(content: &str) -> Value {
    message(content, true)
}

/// "Suhina is thinking…": the real answer follows by editing it (`edit_original`).
pub fn deferred(ephemeral: bool) -> Value {
    let flags = if ephemeral { EPHEMERAL } else { 0 };
    json!({ "type": DEFERRED_CHANNEL_MESSAGE, "data": { "flags": flags } })
}

/// The body that replaces a deferred answer (`PATCH /webhooks/{app}/{token}/messages/@original`).
pub fn edit_original(content: &str) -> Value {
    json!({
        "content": shorten(content, CONTENT_LIMIT),
        "allowed_mentions": no_mentions(),
    })
}

/// The body that replaces a deferred answer with embeds.
pub fn edit_original_embeds(embeds: Vec<Value>) -> Value {
    json!({
        "content": "",
        "embeds": embeds,
        "allowed_mentions": no_mentions(),
    })
}

/// A form with one text field per row; a prefilled value longer than the field's limit is cut.
pub fn modal(custom_id: &str, title: &str, fields: &[TextField]) -> Value {
    let rows: Vec<Value> = fields
        .iter()
        .map(|field| {
            json!({
                "type": 1,
                "components": [{
                    "type": 4,
                    "custom_id": field.custom_id,
                    "label": field.label,
                    "style": if field.paragraph { 2 } else { 1 },
                    "value": shorten(&field.value, field.max_length),
                    "max_length": field.max_length,
                    "required": field.required,
                }],
            })
        })
        .collect();
    json!({
        "type": MODAL,
        "data": { "custom_id": custom_id, "title": shorten(title, 45), "components": rows },
    })
}

/// One text field of a modal.
#[derive(Debug, Clone)]
pub struct TextField {
    pub custom_id: &'static str,
    pub label: &'static str,
    pub value: String,
    pub paragraph: bool,
    pub max_length: usize,
    pub required: bool,
}

fn message_data(content: &str, ephemeral: bool) -> Value {
    let flags = if ephemeral { EPHEMERAL } else { 0 };
    json!({
        "content": shorten(content, CONTENT_LIMIT),
        "flags": flags,
        "allowed_mentions": no_mentions(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_parses_no_mentions() {
        for body in [
            message("@everyone hi", false),
            ephemeral("<@1>"),
            edit_original("@here"),
        ] {
            let mentions = body
                .get("data")
                .unwrap_or(&body)
                .get("allowed_mentions")
                .unwrap();
            assert_eq!(mentions, &json!({ "parse": [] }));
        }
        assert_eq!(
            edit_original_embeds(vec![])["allowed_mentions"],
            json!({ "parse": [] })
        );
    }

    #[test]
    fn ephemeral_sets_the_flag() {
        assert_eq!(ephemeral("x")["data"]["flags"], 64);
        assert_eq!(message("x", false)["data"]["flags"], 0);
        assert_eq!(deferred(true)["data"]["flags"], 64);
        assert_eq!(deferred(false)["type"], 5);
    }

    #[test]
    fn shortens_long_text_to_the_limit() {
        let long = "å".repeat(CONTENT_LIMIT + 10);
        let content = message(&long, false)["data"]["content"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(content.chars().count(), CONTENT_LIMIT);
        assert!(content.ends_with('…'));
        assert_eq!(shorten("short", 10), "short");
    }

    #[test]
    fn builds_a_modal() {
        let body = modal(
            "backlog",
            "Add to backlog",
            &[TextField {
                custom_id: "title",
                label: "Title",
                value: "x".repeat(300),
                paragraph: false,
                max_length: 256,
                required: true,
            }],
        );
        assert_eq!(body["type"], 9);
        let field = &body["data"]["components"][0]["components"][0];
        assert_eq!(field["custom_id"], "title");
        assert_eq!(field["value"].as_str().unwrap().chars().count(), 256);
    }
}
