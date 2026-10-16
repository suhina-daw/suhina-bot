// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Worker's settings that vary by server (docs/operations.md › Settings). Missing ones turn
//! the feature that needs them off, with a message that says which setting is missing.

use suhina_bot_core::access::Roles;
use suhina_bot_core::review::{self, Kind};
use worker::Env;

pub struct Settings {
    pub roles: Roles,
    /// The `bug-reports` forum channel.
    pub bug_forum: Option<String>,
    /// The `feature-requests` forum channel.
    pub feature_forum: Option<String>,
    /// `REVIEW_WORDS`, a secret so the names stay out of the repository; `None` turns the check
    /// off.
    pub review_words: Option<Vec<String>>,
    /// The public base URL of the screenshots bucket; `None` copies no screenshots.
    pub screenshots_url: Option<String>,
    /// The board: organization login and project number.
    pub board_owner: String,
    pub board_number: u64,
}

fn var(env: &Env, name: &str) -> Option<String> {
    env.var(name)
        .ok()
        .map(|value| value.to_string())
        .filter(|value| !value.trim().is_empty())
}

impl Settings {
    pub fn read(env: &Env) -> Self {
        Self {
            roles: Roles::from_settings(
                &var(env, "STAFF_ROLE_IDS").unwrap_or_default(),
                &var(env, "TRIAGE_ROLE_IDS").unwrap_or_default(),
            ),
            bug_forum: var(env, "BUG_REPORTS_FORUM_ID"),
            feature_forum: var(env, "FEATURE_REQUESTS_FORUM_ID"),
            review_words: env
                .secret("REVIEW_WORDS")
                .ok()
                .and_then(|secret| review::parse_words(&secret.to_string())),
            screenshots_url: var(env, "SCREENSHOTS_URL")
                .map(|url| url.trim_end_matches('/').to_owned()),
            board_owner: var(env, "BOARD_OWNER").unwrap_or_else(|| "suhina-daw".to_owned()),
            board_number: var(env, "BOARD_NUMBER")
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
