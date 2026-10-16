// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The bot's state: account links (Discord ID → GitHub account, #8) and
//! pending claims (#9).
//! One Durable Object instance holds it all, so a read always sees the last write, wherever the
//! request arrives; KV could not promise that. Values may have an expiry; an expired value reads
//! as missing and is removed.
//!
//! The worker talks to it with three JSON calls on a made-up URL: `/get {key}`, `/put {key,
//! value, ttl_seconds?}` and `/delete {key}`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use worker::{Date, DurableObject, Env, Request, Response, Result, State, durable_object};

#[durable_object]
pub struct BotState {
    state: State,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    value: Value,
    /// Milliseconds since the epoch, or none for a value that never expires.
    expires_at: Option<u64>,
}

#[derive(Deserialize)]
struct Call {
    key: String,
    #[serde(default)]
    value: Value,
    ttl_seconds: Option<u64>,
}

/// Keys are short, plain names such as `link:<discord id>`.
fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= 200 && key.chars().all(|c| c.is_ascii_graphic())
}

impl DurableObject for BotState {
    fn new(state: State, _env: Env) -> Self {
        Self { state }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        let call: Call = match req.json().await {
            Ok(call) => call,
            Err(_) => return Response::error("Bad call", 400),
        };
        if !valid_key(&call.key) {
            return Response::error("Bad key", 400);
        }
        let storage = self.state.storage();
        match req.path().as_str() {
            "/get" => {
                let stored: Option<Stored> = storage.get(&call.key).await?;
                let now = Date::now().as_millis();
                match stored {
                    Some(stored) if stored.expires_at.is_none_or(|at| at > now) => {
                        Response::from_json(&stored.value)
                    }
                    Some(_) => {
                        storage.delete(&call.key).await?;
                        Response::from_json(&Value::Null)
                    }
                    None => Response::from_json(&Value::Null),
                }
            }
            "/put" => {
                let expires_at = call.ttl_seconds.map(|ttl| {
                    Date::now()
                        .as_millis()
                        .saturating_add(ttl.saturating_mul(1000))
                });
                storage
                    .put(
                        &call.key,
                        Stored {
                            value: call.value,
                            expires_at,
                        },
                    )
                    .await?;
                Response::empty()
            }
            "/delete" => {
                storage.delete(&call.key).await?;
                Response::empty()
            }
            _ => Response::error("Unknown call", 404),
        }
    }
}
