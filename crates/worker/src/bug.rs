// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `/bug`: a submitted form becomes a post in the bug-reports forum, with the platform's tag
//! and the screenshots. Nothing goes to GitHub here (#11).

use serde_json::Value;
use suhina_bot_core::bug::BugReport;
use suhina_bot_core::forum;
use suhina_bot_core::interaction::Interaction;
use worker::{Env, Result, console_error};

use crate::discord_api::{Discord, Upload, download};
use crate::settings::Settings;
use crate::store::Store;

/// One report a minute per person.
const RATE_SECONDS: u64 = 60;

/// The private answer to the reporter.
pub async fn submit(env: &Env, interaction: &Interaction, report: &BugReport) -> Result<String> {
    let settings = Settings::read(env);
    let Some(forum_id) = settings.bug_forum.as_deref() else {
        return Ok("Bug reports are not set up yet (BUG_REPORTS_FORUM_ID). Please post in #bug-reports by hand.".to_owned());
    };
    let reporter = interaction.user_id().unwrap_or_default();

    let store = Store::new(env)?;
    let rate_key = format!("bug-rate:{reporter}");
    if store.get::<Value>(&rate_key).await?.is_some() {
        return Ok("One report a minute, please. Try again in a moment.".to_owned());
    }
    store.put(&rate_key, &true, Some(RATE_SECONDS)).await?;

    let discord = Discord::new(env);
    let forum_channel = discord.channel(forum_id).await?;
    let tag = forum::tag_id(&forum_channel, report.platform.tag);

    let mut files = Vec::new();
    let mut skipped = 0;
    for shot in &report.screenshots {
        match download(&shot.url).await {
            Ok(bytes) => files.push(Upload {
                filename: shot.filename.clone(),
                bytes,
            }),
            Err(error) => {
                console_error!("bug screenshot: {error}");
                skipped += 1;
            }
        }
    }

    let thread = discord
        .create_post(
            forum_id,
            &report.title,
            &tag.into_iter().collect::<Vec<_>>(),
            &report.post_content(reporter),
            &files,
        )
        .await?;
    let thread_id = thread.get("id").and_then(Value::as_str).unwrap_or_default();
    let guild = interaction.guild_id.as_deref().unwrap_or_default();
    let mut answer = format!(
        "Thanks! Your report is posted: https://discord.com/channels/{guild}/{thread_id}\nA moderator will turn it into a GitHub issue."
    );
    if skipped > 0 {
        answer.push_str(&format!(
            "\n{skipped} screenshot(s) could not be attached; you can add them in the post."
        ));
    }
    Ok(answer)
}
