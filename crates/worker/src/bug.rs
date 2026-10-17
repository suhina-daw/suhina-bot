// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `/bug`: a submitted form becomes a post in the bug-reports forum, with the platform's tag
//! and the screenshots. Nothing goes to GitHub here (#9).

use serde_json::Value;
use suhina_bot_core::bug::BugReport;
use suhina_bot_core::forum;
use suhina_bot_core::interaction::Interaction;
use worker::{Env, Result, console_error};

use crate::discord_api::{Discord, MAX_DOWNLOAD, Upload, download};
use crate::settings::Settings;
use crate::store::Store;

/// One report a minute per person.
const RATE_SECONDS: u64 = 60;
/// All of a report's screenshots together; a Worker has 128 MB of memory.
const MAX_TOTAL: usize = 25 * 1024 * 1024;

/// The private answer to the reporter.
pub async fn submit(env: &Env, interaction: &Interaction, report: &BugReport) -> Result<String> {
    let settings = Settings::read(env);
    let Some(forum_id) = settings.bug_forum.as_deref() else {
        return Ok("Bug reports are not set up yet (BUG_REPORTS_FORUM_ID). Please post in #bug-reports by hand.".to_owned());
    };
    let reporter = interaction.user_id().unwrap_or_default();

    let store = Store::new(env)?;
    if !store
        .claim(&format!("bug-rate:{reporter}"), &true, RATE_SECONDS)
        .await?
    {
        return Ok("One report a minute, please. Try again in a moment.".to_owned());
    }

    let discord = Discord::new(env);
    let forum_channel = discord.channel(forum_id).await?;
    let tag = forum::tag_id(&forum_channel, report.platform.tag);

    // Each file at most MAX_DOWNLOAD, all of them together at most MAX_TOTAL: checked against the
    // declared sizes first, and again while reading.
    let mut files = Vec::new();
    let mut skipped = 0;
    let mut total: usize = 0;
    for shot in &report.screenshots {
        let declared = usize::try_from(shot.size).unwrap_or(usize::MAX);
        let room = MAX_TOTAL.saturating_sub(total).min(MAX_DOWNLOAD);
        if declared > room {
            skipped += 1;
            continue;
        }
        match download(&shot.url, room).await {
            Ok(bytes) => {
                total = total.saturating_add(bytes.len());
                files.push(Upload {
                    filename: shot.filename.clone(),
                    bytes,
                });
            }
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
