// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Discord interactions: check the signature, decide the answer in core, and for a deferred
//! answer run the job after answering and edit the answer with its result.

use serde_json::Value;
use suhina_bot_core::access::Access;
use suhina_bot_core::commands::{self, Action, Job};
use suhina_bot_core::interaction::Interaction;
use suhina_bot_core::reply;
use suhina_bot_core::verify::DiscordKey;
use worker::{Context, Env, Response, Result, console_error};

use crate::discord_api::edit_original;
use crate::github::GitHub;
use crate::settings::Settings;
use crate::{bug, track};

/// The answer when a job fails; the details go to the log only.
const FAILED: &str = "Something went wrong on Suhina's side. Try again in a moment.";

pub async fn handle(mut req: worker::Request, env: Env, ctx: Context) -> Result<Response> {
    let signature = req
        .headers()
        .get("X-Signature-Ed25519")?
        .unwrap_or_default();
    let timestamp = req
        .headers()
        .get("X-Signature-Timestamp")?
        .unwrap_or_default();
    let body = req.bytes().await?;

    let key = DiscordKey::from_hex(&env.var("DISCORD_PUBLIC_KEY")?.to_string())
        .map_err(|error| worker::Error::RustError(error.to_string()))?;
    if key.verify(&signature, &timestamp, &body).is_err() {
        return Response::error("Bad signature", 401);
    }

    let interaction = match Interaction::parse(&body) {
        Ok(interaction) => interaction,
        Err(error) => {
            console_error!("interaction: {error}");
            return Response::error("Bad request", 400);
        }
    };

    let access = Access::of(&interaction, &Settings::read(&env).roles);
    match commands::route(&interaction, access) {
        Action::Reply(answer) => Response::from_json(&answer),
        Action::Defer { ephemeral, job } => {
            ctx.wait_until(run_job(env, interaction, job));
            Response::from_json(&reply::deferred(ephemeral))
        }
        Action::DeferUpdate { job } => {
            ctx.wait_until(run_job(env, interaction, job));
            Response::from_json(&reply::deferred_update())
        }
        // A form must be the first answer, so the draft is read before answering.
        Action::OpenEdit { key } => match track::edit_form(&env, &key).await {
            Ok(Some(form)) => Response::from_json(&form),
            Ok(None) => Response::from_json(&reply::ephemeral(
                "This draft has expired (15 minutes). Run the command again.",
            )),
            Err(error) => {
                console_error!("edit form: {error}");
                Response::from_json(&reply::ephemeral(FAILED))
            }
        },
    }
}

/// Runs a deferred job and puts its result in place of "thinking…" (or of the card). A failure
/// is logged and shown to the user as a short apology.
async fn run_job(env: Env, interaction: Interaction, job: Job) {
    let body = match job_answer(&env, &interaction, &job).await {
        Ok(body) => body,
        Err(error) => {
            console_error!("job {}: {error}", job_name(&job));
            reply::edit_original(FAILED)
        }
    };
    if let Err(error) = edit_original(&interaction.application_id, &interaction.token, &body).await
    {
        console_error!("editing the answer of {}: {error}", job_name(&job));
    }
}

/// For logs: the job without its data, which can hold a user's text.
fn job_name(job: &Job) -> &'static str {
    match job {
        Job::Help => "help",
        Job::Bug(_) => "bug",
        Job::TrackThread { .. } => "track",
        Job::TrackMessage(_) => "track message",
        Job::Card { .. } => "card",
        Job::CardEdit { .. } => "card edit",
    }
}

async fn job_answer(env: &Env, interaction: &Interaction, job: &Job) -> Result<Value> {
    match job {
        Job::Help => {
            let github = GitHub::new(env)?;
            let nightly = match github.latest_nightly().await {
                Ok(nightly) => nightly,
                Err(error) => {
                    console_error!("nightly: {error}");
                    None
                }
            };
            let open = github.open_issue_count().await.ok();
            Ok(reply::edit_original(&commands::help::text(
                &commands::definitions(),
                github.repository(),
                nightly.as_ref(),
                open,
            )))
        }
        Job::Bug(report) => Ok(reply::edit_original(
            &bug::submit(env, interaction, report).await?,
        )),
        Job::TrackThread { channel_id, triage } => {
            track::thread(env, interaction, channel_id, *triage).await
        }
        Job::TrackMessage(draft) => track::message(env, (**draft).clone()).await,
        Job::Card {
            action,
            key,
            values,
        } => track::card_action(env, key, *action, values).await,
        Job::CardEdit { key, edit } => track::card_edit(env, key, edit).await,
    }
}
