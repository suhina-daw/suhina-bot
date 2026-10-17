// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Suhina bot on Cloudflare Workers. Discord sends each interaction to `POST /interactions`,
//! GitHub sends the App's webhook deliveries to `POST /github/webhook`; nothing else runs. What to
//! answer is decided in `suhina-bot-core`; this crate does the network and the storage.

mod body;
mod bug;
mod discord;
mod discord_api;
mod github;
mod settings;
mod state;
mod store;
mod track;
mod webhook;

use worker::{Context, Env, Method, Request, Response, Result, console_error, event};

pub use state::BotState;

#[event(fetch)]
pub async fn fetch(req: Request, env: Env, ctx: Context) -> Result<Response> {
    let path = req.path();
    let answer = match (req.method(), path.as_str()) {
        (Method::Post, "/interactions") => discord::handle(req, env, ctx).await,
        (Method::Post, "/github/webhook") => webhook::handle(req, env).await,
        (Method::Get, "/") => Response::ok("Suhina bot: https://github.com/suhina-daw/suhina-bot"),
        _ => Response::error("Not found", 404),
    };
    // An error never reaches the caller with its details: it is logged (without secrets or
    // message text, see core::Error) and answered with a plain 500.
    answer.or_else(|error| {
        console_error!("{path}: {error}");
        Response::error("Internal error", 500)
    })
}
