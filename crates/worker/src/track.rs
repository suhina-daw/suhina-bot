// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `/track` and the message commands: read the report, keep a draft, show the review card, and
//! on **Create issue** open the issue, put it on the board and mark the post
//! (#7). What the card says and allows is decided in core (`review`).

use serde_json::Value;
use suhina_bot_core::forum;
use suhina_bot_core::interaction::Interaction;
use suhina_bot_core::reply;
use suhina_bot_core::review::{self, Action, Draft, Edit, Post, Source};
use worker::{Env, Result, console_error};

use crate::discord_api::Discord;
use crate::github::GitHub;
use crate::settings::{self, Settings};
use crate::store::Store;

const NOT_IN_A_POST: &str = "Use `/track` inside a post in #bug-reports or #feature-requests. Elsewhere, right-click a message › Apps › **Add as bug** or **Add to backlog**.";
const EXPIRED: &str = "This draft has expired (15 minutes). Run the command again.";
const NOT_YOURS: &str = "This card belongs to the moderator who opened it.";
/// ✅ in a URL.
const CHECK_MARK: &str = "%E2%9C%85";
const TRACKED_TAG: &str = "Tracked";

fn draft_key(key: &str) -> String {
    format!("draft:{key}")
}

/// `/track` in a thread: the card for its post, or a line saying why not.
pub async fn thread(
    env: &Env,
    interaction: &Interaction,
    channel_id: &str,
    triage: bool,
) -> Result<Value> {
    let settings = Settings::read(env);
    let discord = Discord::new(env);
    let thread = discord.channel(channel_id).await?;
    let parent = thread
        .get("parent_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(kind) = settings.forum_kind(parent) else {
        return Ok(review::closed_card(NOT_IN_A_POST));
    };
    let store = Store::new(env)?;
    if let Some(number) = store.get::<u64>(&format!("tracked:{channel_id}")).await? {
        let repository = settings::require(env, "GITHUB_REPOSITORY")?;
        return Ok(review::closed_card(&format!(
            "Already tracked as {}.",
            reply::issue_link_in(&repository, number)
        )));
    }
    let forum_channel = discord.channel(parent).await?;
    // A forum post's starter message has the thread's id.
    let starter = discord.message(channel_id, channel_id).await?;
    let attachments = starter
        .get("attachments")
        .and_then(|all| serde_json::from_value(all.clone()).ok())
        .unwrap_or_default();
    let tags = forum::tag_names(&forum_channel, &thread);
    let post = Post {
        thread_id: channel_id,
        title: thread
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        starter: starter
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        attachments,
        tags: &tags,
    };
    let draft = Draft::from_post(
        interaction.user_id().unwrap_or_default(),
        kind,
        post,
        triage,
    );
    show_new(env, &settings, draft).await
}

/// A message command: the card for that one message.
pub async fn message(env: &Env, draft: Draft) -> Result<Value> {
    let settings = Settings::read(env);
    show_new(env, &settings, draft).await
}

/// Fills in what needs GitHub (similar issues, the board's phases), keeps the draft and shows
/// its card.
async fn show_new(env: &Env, settings: &Settings, mut draft: Draft) -> Result<Value> {
    let github = GitHub::new(env)?;
    draft.similar = github.similar(&draft.title).await.unwrap_or_else(|error| {
        console_error!("similar issues: {error}");
        Vec::new()
    });
    if draft.triage {
        match github
            .board(&settings.board_owner, settings.board_number)
            .await
        {
            Ok(board) => draft.phases = board.options("Phase"),
            Err(error) => console_error!("board: {error}"),
        }
    }
    let key = crate::store::random_key()?;
    Store::new(env)?
        .put(&draft_key(&key), &draft, Some(review::DRAFT_SECONDS))
        .await?;
    Ok(review::card(&draft, &key, settings.review_words.as_deref()))
}

/// The edit form for a stored draft, or `None` when it has expired or is someone else's.
pub async fn edit_form(env: &Env, key: &str, user: &str) -> Result<Option<Value>> {
    let draft: Option<Draft> = Store::new(env)?.get(&draft_key(key)).await?;
    Ok(draft
        .filter(|draft| draft.owner == user)
        .map(|draft| review::edit_modal(&draft, key)))
}

/// A button or the menu on a card: the card as it is now.
pub async fn card_action(env: &Env, key: &str, user: &str, action: Action) -> Result<Value> {
    let settings = Settings::read(env);
    let store = Store::new(env)?;
    let Some(mut draft) = store.get::<Draft>(&draft_key(key)).await? else {
        return Ok(review::closed_card(EXPIRED));
    };
    if draft.owner != user {
        return Ok(review::closed_card(NOT_YOURS));
    }
    match action {
        Action::Replace => {
            if let Some(words) = settings.review_words.as_deref() {
                draft.replace_words(words);
            }
        }
        Action::Check => draft.reviewed = true,
        Action::Cancel => {
            store.delete(&draft_key(key)).await?;
            return Ok(review::closed_card(
                "Cancelled. Nothing was sent to GitHub.",
            ));
        }
        Action::Create if draft.reviewed => {
            return create(env, &settings, &store, key, &draft).await;
        }
        Action::Create | Action::Edit | Action::EditSubmit => {}
    }
    store
        .put(&draft_key(key), &draft, Some(review::DRAFT_SECONDS))
        .await?;
    Ok(review::card(&draft, key, settings.review_words.as_deref()))
}

/// The submitted edit form: the card with the new text.
pub async fn card_edit(env: &Env, key: &str, user: &str, edit: &Edit) -> Result<Value> {
    let settings = Settings::read(env);
    let store = Store::new(env)?;
    let Some(mut draft) = store.get::<Draft>(&draft_key(key)).await? else {
        return Ok(review::closed_card(EXPIRED));
    };
    if draft.owner != user {
        return Ok(review::closed_card(NOT_YOURS));
    }
    draft.apply_edit(edit);
    store
        .put(&draft_key(key), &draft, Some(review::DRAFT_SECONDS))
        .await?;
    Ok(review::card(&draft, key, settings.review_words.as_deref()))
}

/// Opens the issue, puts it on the board, marks the post, and closes the card.
async fn create(
    env: &Env,
    settings: &Settings,
    store: &Store,
    key: &str,
    draft: &Draft,
) -> Result<Value> {
    // Reserve first: a second press, a second moderator's card for the same post, or a retry
    // finds the reservation and creates nothing. The reservation is per post for a post, per
    // draft otherwise, and lasts as long as a draft.
    let reservation = match &draft.source {
        Source::Thread { thread_id } => format!("creating:{thread_id}"),
        Source::Message => format!("creating:{key}"),
    };
    if !store
        .claim(&reservation, &true, review::DRAFT_SECONDS)
        .await?
    {
        return Ok(review::closed_card(
            "This report is already being turned into an issue.",
        ));
    }
    let mut problems: Vec<String> = Vec::new();

    let body = review::issue_body(draft);

    let github = GitHub::new(env)?;
    let (number, url, node) = github
        .create_issue(
            &draft.title,
            &body,
            &draft.labels,
            review::issue_type(draft.kind),
        )
        .await?;
    // Recorded before anything else can fail, so the post never gets a second issue.
    if let Source::Thread { thread_id } = &draft.source {
        store
            .put(&format!("tracked:{thread_id}"), &number, None)
            .await?;
    }
    store.delete(&draft_key(key)).await?;

    match github
        .board(&settings.board_owner, settings.board_number)
        .await
    {
        Ok(board) => {
            if let Err(error) = github
                .add_to_board(
                    &board,
                    &node,
                    draft.priority.as_deref(),
                    draft.phase.as_deref(),
                )
                .await
            {
                console_error!("board add: {error}");
                problems.push("it is not on the board yet".to_owned());
            }
        }
        Err(error) => {
            console_error!("board: {error}");
            problems.push("it is not on the board yet".to_owned());
        }
    }

    if let Source::Thread { thread_id } = &draft.source {
        let discord = Discord::new(env);
        if let Err(error) = discord
            .send_message(thread_id, &review::thanks(draft.kind, number, &url))
            .await
        {
            console_error!("thanks: {error}");
            problems.push("the thank-you message could not be posted".to_owned());
        }
        if let Err(error) = mark_tracked(&discord, thread_id).await {
            console_error!("tag: {error}");
            problems.push("the Tracked tag could not be set".to_owned());
        }
        if let Err(error) = discord.react(thread_id, thread_id, CHECK_MARK).await {
            console_error!("reaction: {error}");
            problems.push("the ✅ reaction could not be added".to_owned());
        }
    }

    let mut text = format!("Created {}.", reply::issue_link(number, &url));
    if !problems.is_empty() {
        text.push_str(&format!("\nThe issue exists, but {}.", problems.join(", ")));
    }
    Ok(review::closed_card(&text))
}

/// Adds the Tracked tag of the post's own forum to it, keeping its others (a post holds at most
/// five).
async fn mark_tracked(discord: &Discord, thread_id: &str) -> Result<()> {
    let thread = discord.channel(thread_id).await?;
    let parent = thread
        .get("parent_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let forum_channel = discord.channel(parent).await?;
    let Some(tracked) = forum::tag_id(&forum_channel, TRACKED_TAG) else {
        return Err(worker::Error::RustError(
            "the forum has no Tracked tag".into(),
        ));
    };
    let tags = forum::with_tag(&thread, &tracked);
    discord.set_tags(thread_id, &tags).await
}
