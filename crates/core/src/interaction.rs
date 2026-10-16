// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The parts of a Discord interaction the bot reads. Discord sends many more fields; serde
//! ignores them, and a field the bot needs but does not get is `None` rather than an error.

use serde::Deserialize;
use serde_json::Value;

/// Interaction types (Discord's `type` field).
pub const PING: u8 = 1;
pub const APPLICATION_COMMAND: u8 = 2;
pub const MESSAGE_COMPONENT: u8 = 3;
pub const AUTOCOMPLETE: u8 = 4;
pub const MODAL_SUBMIT: u8 = 5;

/// Application command types (`data.type`).
pub const CHAT_INPUT: u8 = 1;
pub const MESSAGE: u8 = 3;

#[derive(Debug, Clone, Deserialize)]
pub struct Interaction {
    #[serde(rename = "type")]
    pub kind: u8,
    /// Used to answer later: edit the original answer or send follow-ups (15 minutes).
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub application_id: String,
    pub guild_id: Option<String>,
    /// Present in a server: the member, with its roles.
    pub member: Option<Member>,
    /// Present in a direct message.
    pub user: Option<User>,
    pub data: Option<Data>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Member {
    pub user: Option<User>,
    #[serde(default)]
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub id: String,
}

/// `data` of a command, a component or a modal; which fields are set depends on the type.
#[derive(Debug, Clone, Deserialize)]
pub struct Data {
    /// Command name.
    pub name: Option<String>,
    /// Command type (`CHAT_INPUT`, `MESSAGE`).
    #[serde(rename = "type")]
    pub kind: Option<u8>,
    /// Slash command options.
    #[serde(default)]
    pub options: Vec<CommandOption>,
    /// The message a message command was used on.
    pub target_id: Option<String>,
    /// Messages, users and roles the command refers to, by id.
    pub resolved: Option<Value>,
    /// Component or modal id.
    pub custom_id: Option<String>,
    /// Modal fields.
    #[serde(default)]
    pub components: Vec<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommandOption {
    pub name: String,
    pub value: Option<Value>,
    #[serde(default)]
    pub options: Vec<CommandOption>,
}

impl Interaction {
    pub fn parse(body: &[u8]) -> Result<Self, crate::Error> {
        Ok(serde_json::from_slice(body)?)
    }

    /// The id of the person who used the interaction, in a server or a direct message.
    pub fn user_id(&self) -> Option<&str> {
        self.member
            .as_ref()
            .and_then(|member| member.user.as_ref())
            .or(self.user.as_ref())
            .map(|user| user.id.as_str())
    }

    /// The roles of the member; empty outside a server.
    pub fn roles(&self) -> &[String] {
        self.member
            .as_ref()
            .map_or(&[], |member| member.roles.as_slice())
    }

    /// A string option of a slash command, by name.
    pub fn option_str(&self, name: &str) -> Option<&str> {
        self.data
            .as_ref()?
            .options
            .iter()
            .find(|option| option.name == name)?
            .value
            .as_ref()?
            .as_str()
    }

    /// The value of a text field of a submitted modal, by its custom id.
    pub fn modal_value(&self, custom_id: &str) -> Option<&str> {
        self.data.as_ref()?.components.iter().find_map(|row| {
            row.get("components")?.as_array()?.iter().find_map(|field| {
                (field.get("custom_id")?.as_str()? == custom_id)
                    .then(|| field.get("value")?.as_str())
                    .flatten()
            })
        })
    }

    /// The content of the message a message command was used on.
    pub fn target_message_content(&self) -> Option<&str> {
        let data = self.data.as_ref()?;
        let target = data.target_id.as_deref()?;
        data.resolved
            .as_ref()?
            .get("messages")?
            .get(target)?
            .get("content")?
            .as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_ping() {
        let interaction = Interaction::parse(
            br#"{"type":1,"id":"1","application_id":"2","token":"t","version":1}"#,
        )
        .unwrap();
        assert_eq!(interaction.kind, PING);
        assert!(interaction.data.is_none());
    }

    #[test]
    fn reads_a_slash_command_in_a_server() {
        let body = br#"{"type":2,"token":"tok","application_id":"app","guild_id":"g",
            "member":{"user":{"id":"42"},"roles":["7","8"]},
            "data":{"name":"issue","type":1,"options":[{"name":"number","type":4,"value":538},{"name":"text","type":3,"value":"hi"}]}}"#;
        let interaction = Interaction::parse(body).unwrap();
        assert_eq!(interaction.user_id(), Some("42"));
        assert_eq!(interaction.roles(), ["7", "8"]);
        assert_eq!(interaction.option_str("text"), Some("hi"));
        assert_eq!(interaction.option_str("number"), None);
        assert_eq!(interaction.data.unwrap().name.as_deref(), Some("issue"));
    }

    #[test]
    fn reads_a_message_command_target() {
        let body = br#"{"type":2,"token":"t","application_id":"a","user":{"id":"5"},
            "data":{"name":"Add to backlog","type":3,"target_id":"99",
            "resolved":{"messages":{"99":{"id":"99","content":"Please add MIDI learn"}}}}}"#;
        let interaction = Interaction::parse(body).unwrap();
        assert_eq!(interaction.user_id(), Some("5"));
        assert!(interaction.roles().is_empty());
        assert_eq!(
            interaction.target_message_content(),
            Some("Please add MIDI learn")
        );
    }

    #[test]
    fn reads_modal_values() {
        let body = br#"{"type":5,"token":"t","application_id":"a","data":{"custom_id":"backlog",
            "components":[{"type":1,"components":[{"type":4,"custom_id":"title","value":"MIDI learn"}]},
                          {"type":1,"components":[{"type":4,"custom_id":"body","value":"Text"}]}]}}"#;
        let interaction = Interaction::parse(body).unwrap();
        assert_eq!(interaction.modal_value("title"), Some("MIDI learn"));
        assert_eq!(interaction.modal_value("body"), Some("Text"));
        assert_eq!(interaction.modal_value("missing"), None);
    }

    #[test]
    fn refuses_a_body_that_is_not_an_interaction() {
        assert!(Interaction::parse(b"not json").is_err());
        assert!(Interaction::parse(br#"{"no_type":true}"#).is_err());
    }
}
