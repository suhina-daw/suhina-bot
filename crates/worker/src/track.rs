// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `/track` and the message commands: read the report, keep a draft, show the review card, and
//! on **Create issue** open the issue, put it on the board and mark the post
//! (#5). What the card says and allows is decided in core (`review`).

use serde_json::Value;
use suhina_bot_core::forum;
use suhina_bot_core::interaction::Interaction;
use suhina_bot_core::reply;
use suhina_bot_core::review::{self, Action, Draft, Edit, Kind, Post, Source};
use worker::{Env, Result, console_error};

use crate::discord_api::Discord;
use crate::github::GitHub;
use crate::settings::{self, Settings};
use crate::store::Store;

const NOT_IN_A_POST: &str = "Use `/track` inside a post in #bug-reports or #feature-requests. Elsewhere, right-click a message › Apps › **Add as bug** or **Add to backlog**.";
const EXPIRED: &str = "This draft has expired (15 minutes). Run the command again.";
const NOT_YOURS: &str = "This card belongs to the moderator who opened it.";
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
    let owner = interaction.user_id().unwrap_or_default();
    match post_draft(env, &settings, owner, channel_id, triage, None).await? {
        Some(draft) => show_new(env, &settings, draft).await,
        None => Ok(review::closed_card(NOT_IN_A_POST)),
    }
}

/// A message command: the card for that one message. On a post's starter message in
/// bug-reports or feature-requests it is the post's card, as `/track` makes it, of the kind the
/// command names.
pub async fn message(env: &Env, draft: Draft) -> Result<Value> {
    let settings = Settings::read(env);
    if let Source::Message {
        channel_id,
        message_id,
    } = &draft.source
        && channel_id == message_id
        && let Some(post) = post_draft(
            env,
            &settings,
            &draft.owner,
            channel_id,
            draft.triage,
            Some(draft.kind),
        )
        .await?
    {
        return show_new(env, &settings, post).await;
    }
    show_new(env, &settings, draft).await
}

/// The draft for a post in bug-reports or feature-requests, or `None` when the thread is not
/// one. `kind` overrides what the forum makes it.
async fn post_draft(
    env: &Env,
    settings: &Settings,
    owner: &str,
    channel_id: &str,
    triage: bool,
    kind: Option<Kind>,
) -> Result<Option<Draft>> {
    let discord = Discord::new(env);
    let thread = discord.channel(channel_id).await?;
    let parent = thread
        .get("parent_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(forum_kind) = settings.forum_kind(parent) else {
        return Ok(None);
    };
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
    Ok(Some(Draft::from_post(
        owner,
        kind.unwrap_or(forum_kind),
        post,
        triage,
    )))
}

/// Fills in what needs GitHub (similar issues, the board's phases), keeps the draft and shows
/// its card.
async fn show_new(env: &Env, settings: &Settings, mut draft: Draft) -> Result<Value> {
    let tracked = format!("tracked:{}", draft.source.message_id());
    if let Some(number) = Store::new(env)?.get::<u64>(&tracked).await? {
        let repository = settings::require(env, "GITHUB_REPOSITORY")?;
        return Ok(review::closed_card(&format!(
            "Already tracked as {}.",
            reply::issue_link_in(&repository, number)
        )));
    }
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
    // message (a post's starter message has the thread's id), and lasts as long as a draft.
    let reservation = format!("creating:{}", draft.source.message_id());
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
    // Recorded before anything else can fail, so the report never gets a second issue.
    store
        .put(
            &format!("tracked:{}", draft.source.message_id()),
            &number,
            None,
        )
        .await?;
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

    // Thanks and a reaction on the report: a post also gets its Tracked tag, a message is
    // answered with a reply.
    let discord = Discord::new(env);
    let thanks = review::thanks(draft.kind, number, &url);
    let (channel, message) = match &draft.source {
        Source::Thread { thread_id } => {
            if let Err(error) = discord.send_message(thread_id, &thanks).await {
                console_error!("thanks: {error}");
                problems.push("the thank-you message could not be posted".to_owned());
            }
            if let Err(error) = mark_tracked(&discord, thread_id).await {
                console_error!("tag: {error}");
                problems.push("the Tracked tag could not be set".to_owned());
            }
            (thread_id, thread_id)
        }
        Source::Message {
            channel_id,
            message_id,
        } => {
            if let Err(error) = discord.reply(channel_id, message_id, &thanks).await {
                console_error!("thanks: {error}");
                problems.push("the thank-you reply could not be posted".to_owned());
            }
            (channel_id, message_id)
        }
    };
    if let Err(error) = discord.react(channel, message, &settings.reaction).await {
        console_error!("reaction: {error}");
        problems.push("the reaction could not be added".to_owned());
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
