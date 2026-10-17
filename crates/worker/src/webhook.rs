// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The App's webhook. Every delivery is checked against `GITHUB_WEBHOOK_SECRET`; a delivery that
//! fails is refused with 401. The bot subscribes to `issue_comment` only, to relay the claim
//! bot's answers back to Discord (#7); until then deliveries are accepted
//! and dropped.

use suhina_bot_core::webhook::verify_github;
use worker::{Env, Request, Response, Result};

pub async fn handle(mut req: Request, env: Env) -> Result<Response> {
    let signature = req
        .headers()
        .get("X-Hub-Signature-256")?
        .unwrap_or_default();
    let Some(body) = crate::body::read(&mut req, crate::body::WEBHOOK_LIMIT).await? else {
        return Response::error("Too large", 413);
    };
    let secret = env.secret("GITHUB_WEBHOOK_SECRET")?.to_string();
    if verify_github(&secret, &signature, &body).is_err() {
        return Response::error("Bad signature", 401);
    }
    // `ping` (sent when the webhook is saved) and `issue_comment` alike: nothing to do yet.
    Ok(Response::empty()?.with_status(204))
}
