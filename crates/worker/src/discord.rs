// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Discord interactions: check the signature, decide the answer in core, and for a deferred
//! answer run the job after answering and edit the answer with its result.

use suhina_bot_core::commands::{self, Action, Job};
use suhina_bot_core::interaction::Interaction;
use suhina_bot_core::reply;
use suhina_bot_core::verify::DiscordKey;
use worker::wasm_bindgen::JsValue;
use worker::{
    Context, Env, Fetch, Headers, Method, Request, RequestInit, Response, Result, console_error,
};

use crate::github::GitHub;

const DISCORD_API: &str = "https://discord.com/api/v10";

/// The answer when a job fails; the details go to the log only.
const FAILED: &str = "Something went wrong on Suhina's side. Try again in a moment.";

pub async fn handle(mut req: Request, env: Env, ctx: Context) -> Result<Response> {
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

    match commands::route(&interaction) {
        Action::Reply(answer) => Response::from_json(&answer),
        Action::Defer { ephemeral, job } => {
            ctx.wait_until(run_job(env, interaction, job));
            Response::from_json(&reply::deferred(ephemeral))
        }
    }
}

/// Runs a deferred job and puts its result in place of "thinking…". A failure is logged and
/// shown to the user as a short apology.
async fn run_job(env: Env, interaction: Interaction, job: Job) {
    let body = match job_answer(&env, &job).await {
        Ok(body) => body,
        Err(error) => {
            console_error!("job {job:?}: {error}");
            reply::edit_original(FAILED)
        }
    };
    if let Err(error) = edit_original(&interaction, &body).await {
        console_error!("editing the answer of {job:?}: {error}");
    }
}

async fn job_answer(env: &Env, job: &Job) -> Result<serde_json::Value> {
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
    }
}

/// `PATCH /webhooks/{application}/{token}/messages/@original`: needs no bot token, the
/// interaction's token is the credential, valid for 15 minutes.
async fn edit_original(interaction: &Interaction, body: &serde_json::Value) -> Result<()> {
    let url = format!(
        "{DISCORD_API}/webhooks/{}/{}/messages/@original",
        interaction.application_id, interaction.token
    );
    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;
    let mut init = RequestInit::new();
    init.with_method(Method::Patch)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&body.to_string())));
    let mut response = Fetch::Request(Request::new_with_init(&url, &init)?)
        .send()
        .await?;
    if response.status_code() >= 300 {
        let status = response.status_code();
        let text = response.text().await.unwrap_or_default();
        return Err(worker::Error::RustError(format!(
            "Discord answered {status}: {}",
            reply::shorten(&text, 300)
        )));
    }
    Ok(())
}
