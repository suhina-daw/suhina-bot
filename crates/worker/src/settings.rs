// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Worker's settings (docs/operations.md › Settings). Every ID and key is a secret, set with
//! `wrangler secret put`, so none of them is written in the repository. A missing one turns the
//! feature that needs it off, with a message that says which setting is missing.

use suhina_bot_core::access::Roles;
use suhina_bot_core::review::{self, Kind};
use worker::Env;

pub struct Settings {
    /// Suhina's Discord server, `DISCORD_GUILD_ID`: the only one where anything writes.
    pub guild: Option<String>,
    pub roles: Roles,
    /// The `bug-reports` forum channel.
    pub bug_forum: Option<String>,
    /// The `feature-requests` forum channel.
    pub feature_forum: Option<String>,
    /// `REVIEW_WORDS`; `None` turns the check off.
    pub review_words: Option<Vec<String>>,
    /// The reaction on a tracked report, `TRACKED_EMOJI` (a server emoji), as a URL part.
    pub reaction: String,
    /// The board: organization login and project number.
    pub board_owner: String,
    pub board_number: u64,
}

/// A setting by name: a secret, or a plain variable (`wrangler.toml` `[vars]` holds only names
/// that are no IDs, such as `GITHUB_REPOSITORY`). Empty counts as missing.
pub fn get(env: &Env, name: &str) -> Option<String> {
    env.secret(name)
        .map(|secret| secret.to_string())
        .or_else(|_| env.var(name).map(|var| var.to_string()))
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// A setting that must be there, as an error naming it when it is not.
pub fn require(env: &Env, name: &str) -> worker::Result<String> {
    get(env, name).ok_or_else(|| worker::Error::RustError(format!("the setting {name} is not set")))
}

impl Settings {
    pub fn read(env: &Env) -> Self {
        Self {
            guild: get(env, "DISCORD_GUILD_ID"),
            roles: Roles::from_settings(
                &get(env, "STAFF_ROLE_IDS").unwrap_or_default(),
                &get(env, "TRIAGE_ROLE_IDS").unwrap_or_default(),
            ),
            bug_forum: get(env, "BUG_REPORTS_FORUM_ID"),
            feature_forum: get(env, "FEATURE_REQUESTS_FORUM_ID"),
            review_words: get(env, "REVIEW_WORDS").and_then(|words| review::parse_words(&words)),
            reaction: review::reaction(get(env, "TRACKED_EMOJI").as_deref()),
            board_owner: get(env, "BOARD_OWNER").unwrap_or_else(|| "suhina-daw".to_owned()),
            board_number: get(env, "BOARD_NUMBER")
                .and_then(|n| n.parse().ok())
                .unwrap_or(1),
        }
    }

    /// What a post in this forum becomes: bug-reports a bug, feature-requests a feature; other
    /// channels nothing.
    pub fn forum_kind(&self, forum: &str) -> Option<Kind> {
        if forum.is_empty() {
            None
        } else if self.bug_forum.as_deref() == Some(forum) {
            Some(Kind::Bug)
        } else if self.feature_forum.as_deref() == Some(forum) {
            Some(Kind::Feature)
        } else {
            None
        }
    }
}
