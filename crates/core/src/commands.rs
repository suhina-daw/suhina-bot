// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The dispatcher: every interaction becomes an `Action`, either an answer right away or a
//! deferred answer whose `Job` the worker runs afterwards (anything that calls GitHub, since
//! Discord waits only three seconds for the first answer).
//!
//! Each command family is a module of its own with its definition (registered by
//! `suhina-bot-register`) and its routing. A new family adds its definitions to `definitions`
//! and a branch to `route`.

use serde_json::{Value, json};

use crate::access::Access;
use crate::bug::{self, BugReport};
use crate::interaction::{
    APPLICATION_COMMAND, AUTOCOMPLETE, CHAT_INPUT, Interaction, MESSAGE, MESSAGE_COMPONENT,
    MODAL_SUBMIT, PING,
};
use crate::reply;
use crate::review::{self, Draft, Edit, Kind};

/// What the worker does with an interaction.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Send this body as the answer.
    Reply(Value),
    /// Answer "thinking…" now, then run the job and edit the answer with its result.
    Defer { ephemeral: bool, job: Job },
    /// For a button or a form on a card: acknowledge without a new message, run the job and
    /// edit the card.
    DeferUpdate { job: Job },
    /// Open the edit form of a draft, which the worker reads first.
    OpenEdit { key: String },
}

/// Work that needs the network, run after the deferred answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    /// `/help`: what the bot is, its commands, the latest nightly and the open issues.
    Help,
    /// A submitted `/bug` form: make the forum post.
    Bug(Box<BugReport>),
    /// `/track` in a thread: read the post and show the review card.
    TrackThread { channel_id: String, triage: bool },
    /// Add as bug / Add to backlog: show the review card for one message.
    TrackMessage(Box<Draft>),
    /// A button or menu on a review card.
    Card { action: review::Action, key: String },
    /// The submitted edit form of a review card.
    CardEdit { key: String, edit: Edit },
}

/// The answer to anything the bot does not handle (yet).
pub const UNKNOWN: &str = "Suhina does not know this command yet.";
pub const STAFF_ONLY: &str = "Only moderators and contributors can turn reports into issues.";
pub const SERVER_ONLY: &str = "This works in the Suhina server only.";

pub fn route(interaction: &Interaction, access: Access) -> Action {
    match interaction.kind {
        PING => Action::Reply(reply::pong()),
        APPLICATION_COMMAND => route_command(interaction, access),
        AUTOCOMPLETE => Action::Reply(json!({ "type": 8, "data": { "choices": [] } })),
        MESSAGE_COMPONENT => route_component(interaction, access),
        MODAL_SUBMIT => route_modal(interaction, access),
        _ => Action::Reply(reply::ephemeral(UNKNOWN)),
    }
}

fn private(text: &str) -> Action {
    Action::Reply(reply::ephemeral(text))
}

fn route_command(interaction: &Interaction, access: Access) -> Action {
    let Some(data) = interaction.data.as_ref() else {
        return private(UNKNOWN);
    };
    let in_server = access.home;
    match (data.kind.unwrap_or(CHAT_INPUT), data.name.as_deref()) {
        (CHAT_INPUT, Some(help::NAME)) => Action::Defer {
            ephemeral: true,
            job: Job::Help,
        },
        (CHAT_INPUT, Some(bug::NAME)) if in_server => Action::Reply(bug::modal()),
        (CHAT_INPUT, Some(review::NAME)) if in_server => {
            if !access.staff {
                return private(STAFF_ONLY);
            }
            match interaction.channel_id.clone() {
                Some(channel_id) => Action::Defer {
                    ephemeral: true,
                    job: Job::TrackThread {
                        channel_id,
                        triage: access.triage,
                    },
                },
                None => private(UNKNOWN),
            }
        }
        (MESSAGE, Some(name @ (review::ADD_AS_BUG | review::ADD_TO_BACKLOG))) if in_server => {
            if !access.staff {
                return private(STAFF_ONLY);
            }
            let kind = if name == review::ADD_AS_BUG {
                Kind::Bug
            } else {
                Kind::Feature
            };
            let draft = Draft::from_message(
                interaction.user_id().unwrap_or_default(),
                kind,
                interaction.channel_id.as_deref().unwrap_or_default(),
                interaction.target_message_id().unwrap_or_default(),
                interaction.target_message_content().unwrap_or_default(),
                &interaction.target_message_attachments(),
                access.triage,
            );
            Action::Defer {
                ephemeral: true,
                job: Job::TrackMessage(Box::new(draft)),
            }
        }
        (_, Some(bug::NAME | review::NAME | review::ADD_AS_BUG | review::ADD_TO_BACKLOG)) => {
            private(SERVER_ONLY)
        }
        _ => private(UNKNOWN),
    }
}

fn route_component(interaction: &Interaction, access: Access) -> Action {
    let Some(data) = interaction.data.as_ref() else {
        return private(UNKNOWN);
    };
    match data.custom_id.as_deref().and_then(review::parse_custom_id) {
        Some(_) if !access.staff => private(STAFF_ONLY),
        Some((review::Action::Edit, key)) => Action::OpenEdit { key },
        Some((action, key)) => Action::DeferUpdate {
            job: Job::Card { action, key },
        },
        None => private(UNKNOWN),
    }
}

fn route_modal(interaction: &Interaction, access: Access) -> Action {
    let custom_id = interaction
        .data
        .as_ref()
        .and_then(|data| data.custom_id.as_deref())
        .unwrap_or_default();
    if custom_id == bug::MODAL_ID {
        if !access.home {
            return private(SERVER_ONLY);
        }
        return match BugReport::from_submit(interaction) {
            Ok(report) => Action::Defer {
                ephemeral: true,
                job: Job::Bug(Box::new(report)),
            },
            Err(problem) => private(problem),
        };
    }
    match review::parse_custom_id(custom_id) {
        Some(_) if !access.staff => private(STAFF_ONLY),
        Some((review::Action::EditSubmit, key)) => Action::DeferUpdate {
            job: Job::CardEdit {
                key,
                edit: Edit::from_submit(interaction),
            },
        },
        _ => private(UNKNOWN),
    }
}

/// Every command, in the form Discord's bulk overwrite takes
/// (`PUT /applications/{id}/commands`). `/help` lists them from here too.
pub fn definitions() -> Value {
    let mut all = vec![help::definition(), bug::definition()];
    all.extend(review::definitions());
    Value::Array(all)
}

/// `docs/commands.md`, generated from `definitions` (`suhina-bot-register --markdown`); CI fails
/// when the file in the repository differs.
pub fn markdown(definitions: &Value) -> String {
    let mut out = String::from(
        "---\n\
         description: Every command of the Suhina bot, generated from the command definitions\n\
         read_when: commands, slash command, message command, what can the bot do, who can use\n\
         type: system\n\
         status: current\n\
         covers: crates/core/src/commands.rs\n\
         ---\n\n\
         # Commands\n\n\
         <!-- Generated by `cargo run -p suhina-bot-register -- --markdown > docs/commands.md`. \
         Do not edit by hand. -->\n\n\
         Registered with Discord by `suhina-bot-register` (docs/operations.md › Registering \
         commands).\n\n\
         | Command | How to use it | Who | What it does |\n\
         |---|---|---|---|\n",
    );
    for command in definitions
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let name = command
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let kind = command.get("type").and_then(Value::as_u64).unwrap_or(1);
        let description = describe(command);
        let who = if command.get("default_member_permissions").is_some() {
            "Moderators and contributors"
        } else {
            "Everyone"
        };
        let usage = if kind == u64::from(crate::interaction::MESSAGE) {
            format!("Right-click a message › Apps › {name}")
        } else {
            let options: Vec<String> = command
                .get("options")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(|option| option.get("name").and_then(Value::as_str))
                .map(|option| format!(" {option}:…"))
                .collect();
            format!("`/{name}{}`", options.concat())
        };
        let shown = if kind == u64::from(crate::interaction::MESSAGE) {
            name.to_owned()
        } else {
            format!("`/{name}`")
        };
        out.push_str(&format!("| {shown} | {usage} | {who} | {description} |\n"));
    }
    out
}

/// What a command does: its description, or for a message command (whose description Discord
/// keeps empty) the text from `review::message_command_about`.
fn describe(command: &Value) -> &str {
    let name = command
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match command.get("description").and_then(Value::as_str) {
        Some(text) if !text.is_empty() => text,
        _ => review::message_command_about(name),
    }
}

fn staff_mark(command: &Value) -> &'static str {
    if command.get("default_member_permissions").is_some() {
        " (moderators and contributors)"
    } else {
        ""
    }
}

pub mod help {
    //! `/help`: the bot's help. What it is, every command (read from `definitions`, so the list
    //! never falls behind), direct links to the latest nightly, and the open issues, which also
    //! shows that the bot can reach GitHub.

    use serde_json::{Value, json};

    use crate::interaction::MESSAGE;
    use crate::nightly::{Nightly, megabytes};

    pub const NAME: &str = "help";

    pub fn definition() -> Value {
        json!({
            "name": NAME,
            "type": 1,
            "description": "What Suhina bot does, its commands, and the latest nightly build",
        })
    }

    /// The answer. `nightly` and `open_issues` are `None` when GitHub could not be reached.
    pub fn text(
        definitions: &Value,
        repository: &str,
        nightly: Option<&Nightly>,
        open_issues: Option<u64>,
    ) -> String {
        let mut lines = vec![
            "**Suhina bot**: looks up issues, pull requests and test builds, turns messages into \
             issues for moderators and lets contributors claim work. It answers commands only and \
             never reads messages on its own."
                .to_owned(),
            String::new(),
            "**Commands**".to_owned(),
        ];
        for command in definitions
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let name = command
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let kind = command.get("type").and_then(Value::as_u64).unwrap_or(1);
            let staff = super::staff_mark(command);
            if kind == u64::from(MESSAGE) {
                lines.push(format!(
                    "• Right-click a message › Apps › **{name}**{staff}"
                ));
            } else {
                let description = super::describe(command);
                lines.push(format!("• `/{name}`: {description}{staff}"));
            }
        }

        lines.push(String::new());
        match nightly {
            Some(nightly) => {
                lines.push(format!(
                    "**Latest nightly** `{}` ({})",
                    nightly.version, nightly.date
                ));
                for download in &nightly.downloads {
                    lines.push(format!(
                        "• [{}](<{}>) {}",
                        download.platform,
                        download.url,
                        megabytes(download.size_bytes)
                    ));
                }
                lines.push(format!("Release notes: <{}>", nightly.page));
            }
            None => {
                lines.push("**Latest nightly**: could not be read from GitHub just now.".to_owned())
            }
        }

        lines.push(String::new());
        if let Some(count) = open_issues {
            lines.push(format!(
                "{repository} has {count} open issues and pull requests."
            ));
        }
        lines.push("Source: <https://github.com/suhina-daw/suhina-bot>".to_owned());
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::Source;

    fn interaction(body: &str) -> Interaction {
        Interaction::parse(body.as_bytes()).unwrap()
    }

    const STAFF: Access = Access {
        home: true,
        staff: true,
        triage: false,
    };
    /// A member of Suhina's server without a staff role.
    const NOBODY: Access = Access {
        home: true,
        staff: false,
        triage: false,
    };
    /// Someone in another server or a direct message.
    const ELSEWHERE: Access = Access {
        home: false,
        staff: false,
        triage: false,
    };

    #[test]
    fn answers_ping_with_pong() {
        assert_eq!(
            route(&interaction(r#"{"type":1}"#), NOBODY),
            Action::Reply(json!({ "type": 1 }))
        );
    }

    #[test]
    fn markdown_lists_each_command_once() {
        let markdown = markdown(&json!([
            { "name": "help", "type": 1, "description": "Help" },
            { "name": "issue", "type": 1, "description": "An issue", "options": [{ "name": "number" }] },
            { "name": "Add as bug", "type": 3, "default_member_permissions": "0" },
        ]));
        assert!(markdown.starts_with("---\n"));
        assert!(markdown.contains("| `/help` | `/help` | Everyone | Help |"));
        assert!(markdown.contains("| `/issue` | `/issue number:…` | Everyone | An issue |"));
        assert!(markdown.contains(
            "| Add as bug | Right-click a message › Apps › Add as bug | Moderators and contributors | Turn a message into a GitHub bug issue, after a review |"
        ));
    }

    #[test]
    fn defers_help() {
        let action = route(
            &interaction(r#"{"type":2,"data":{"name":"help","type":1}}"#),
            NOBODY,
        );
        assert_eq!(
            action,
            Action::Defer {
                ephemeral: true,
                job: Job::Help
            }
        );
    }

    #[test]
    fn bug_opens_the_form_for_anyone_in_the_server() {
        let Action::Reply(answer) = route(
            &interaction(r#"{"type":2,"guild_id":"g","data":{"name":"bug","type":1}}"#),
            NOBODY,
        ) else {
            panic!()
        };
        assert_eq!(answer["type"], 9);
        let Action::Reply(answer) = route(
            &interaction(r#"{"type":2,"guild_id":"other","data":{"name":"bug","type":1}}"#),
            ELSEWHERE,
        ) else {
            panic!()
        };
        assert_eq!(answer["data"]["content"], SERVER_ONLY);
        let submit = r#"{"type":5,"data":{"custom_id":"bug:submit","components":[]}}"#;
        let Action::Reply(answer) = route(&interaction(submit), ELSEWHERE) else {
            panic!()
        };
        assert_eq!(answer["data"]["content"], SERVER_ONLY);
    }

    #[test]
    fn track_is_for_staff() {
        let body =
            r#"{"type":2,"guild_id":"g","channel_id":"t1","data":{"name":"track","type":1}}"#;
        let Action::Reply(answer) = route(&interaction(body), NOBODY) else {
            panic!()
        };
        assert_eq!(answer["data"]["content"], STAFF_ONLY);
        assert_eq!(
            route(&interaction(body), STAFF),
            Action::Defer {
                ephemeral: true,
                job: Job::TrackThread {
                    channel_id: "t1".to_owned(),
                    triage: false
                }
            }
        );
    }

    #[test]
    fn message_commands_make_a_draft() {
        let body = r#"{"type":2,"guild_id":"g","channel_id":"c1","member":{"user":{"id":"7"},"roles":[]},
            "data":{"name":"Add to backlog","type":3,"target_id":"5",
            "resolved":{"messages":{"5":{"content":"Tempo tap\nPlease add it","attachments":[]}}}}}"#;
        let Action::Defer {
            job: Job::TrackMessage(draft),
            ..
        } = route(&interaction(body), STAFF)
        else {
            panic!()
        };
        assert_eq!(draft.title, "Tempo tap");
        assert_eq!(draft.owner, "7");
        assert_eq!(draft.kind, Kind::Feature);
        assert_eq!(
            draft.source,
            Source::Message {
                channel_id: "c1".to_owned(),
                message_id: "5".to_owned()
            }
        );
        assert_eq!(draft.source.message_id(), "5");
        let Action::Reply(answer) = route(&interaction(body), NOBODY) else {
            panic!()
        };
        assert_eq!(answer["data"]["content"], STAFF_ONLY);
    }

    #[test]
    fn card_buttons_and_the_edit_form() {
        let button = r#"{"type":3,"data":{"custom_id":"track:check:ab"}}"#;
        assert_eq!(
            route(&interaction(button), STAFF),
            Action::DeferUpdate {
                job: Job::Card {
                    action: review::Action::Check,
                    key: "ab".to_owned()
                }
            }
        );
        let edit = r#"{"type":3,"data":{"custom_id":"track:edit:ab"}}"#;
        assert_eq!(
            route(&interaction(edit), STAFF),
            Action::OpenEdit {
                key: "ab".to_owned()
            }
        );
        let Action::Reply(answer) = route(&interaction(edit), NOBODY) else {
            panic!()
        };
        assert_eq!(answer["data"]["content"], STAFF_ONLY);
        let submit = r#"{"type":5,"data":{"custom_id":"track:editsubmit:ab","components":[]}}"#;
        assert!(matches!(
            route(&interaction(submit), STAFF),
            Action::DeferUpdate {
                job: Job::CardEdit { .. }
            }
        ));
    }

    #[test]
    fn a_bug_form_without_a_platform_is_answered_privately() {
        let body = r#"{"type":5,"data":{"custom_id":"bug:submit","components":[
            {"type":18,"component":{"type":4,"custom_id":"title","value":"T"}},
            {"type":18,"component":{"type":4,"custom_id":"what","value":"W"}}]}}"#;
        let Action::Reply(answer) = route(&interaction(body), NOBODY) else {
            panic!()
        };
        assert_eq!(answer["data"]["content"], "Please choose your platform.");
    }

    #[test]
    fn answers_unknown_commands_ephemerally() {
        for body in [
            r#"{"type":2,"data":{"name":"nope","type":1}}"#,
            r#"{"type":2,"data":{"name":"help","type":3}}"#,
            r#"{"type":2}"#,
            r#"{"type":3,"data":{"custom_id":"x"}}"#,
            r#"{"type":5,"data":{"custom_id":"x"}}"#,
            r#"{"type":99}"#,
        ] {
            let Action::Reply(answer) = route(&interaction(body), STAFF) else {
                panic!("{body} was deferred")
            };
            assert_eq!(answer["data"]["flags"], 64, "{body}");
            assert_eq!(answer["data"]["content"], UNKNOWN, "{body}");
        }
    }

    #[test]
    fn answers_autocomplete_with_no_choices() {
        let Action::Reply(answer) =
            route(&interaction(r#"{"type":4,"data":{"name":"x"}}"#), NOBODY)
        else {
            panic!()
        };
        assert_eq!(answer["type"], 8);
    }

    #[test]
    fn definitions_are_valid_for_discord() {
        let definitions = definitions();
        let all = definitions.as_array().unwrap();
        assert!(!all.is_empty());
        for command in all {
            let name = command["name"].as_str().unwrap();
            assert!(name.chars().count() <= 32);
            if command["type"] == 1 {
                assert!(
                    name.chars()
                        .all(|c| c.is_ascii_lowercase() || c == '-' || c == '_')
                );
                assert!(command["description"].as_str().unwrap().chars().count() <= 100);
            } else {
                assert!(
                    !describe(command).is_empty(),
                    "{name} has no text for /help"
                );
            }
        }
    }

    #[test]
    fn help_lists_every_command_and_the_nightly() {
        let definitions = json!([
            { "name": "help", "type": 1, "description": "Help" },
            { "name": "Add as bug", "type": 3 },
        ]);
        let nightly = crate::nightly::Nightly {
            version: "0.0.1-nightly.20261016+7e0d477".into(),
            date: "2026-10-16".into(),
            page: "https://github.com/suhina-daw/suhina-daw/releases/tag/nightly".into(),
            downloads: vec![crate::nightly::Download {
                platform: "Windows",
                url: "https://example.invalid/w.zip".into(),
                size_bytes: 17_926_253,
            }],
        };
        let text = help::text(
            &definitions,
            "suhina-daw/suhina-daw",
            Some(&nightly),
            Some(73),
        );
        assert!(text.contains("`/help`: Help"));
        assert!(text.contains("Right-click a message › Apps › **Add as bug**"));
        assert!(text.contains("[Windows](<https://example.invalid/w.zip>) 17.9 MB"));
        assert!(text.contains("73 open issues"));
        assert!(text.chars().count() <= crate::reply::CONTENT_LIMIT);
        // The introduction is one line, with single spaces, however the source is wrapped.
        let first = text.lines().next().unwrap();
        assert!(first.ends_with("never reads messages on its own."));
        assert!(!text.contains("  "));
    }

    #[test]
    fn help_says_when_github_is_unreachable() {
        let text = help::text(&definitions(), "suhina-daw/suhina-daw", None, None);
        assert!(text.contains("could not be read from GitHub"));
        assert!(!text.contains("open issues"));
    }
}
