// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The bot's state: review drafts, which issue a post became, interactions already handled,
//! `/bug`'s rate limit, and later account links (#6) and pending claims (#7). One Durable Object
//! instance holds it all, so a read always sees the last write, wherever the request arrives; KV
//! could not promise that.
//!
//! The worker talks to it with JSON calls on a made-up URL: `/get {key}`, `/put {key, value,
//! ttl_seconds?}`, `/delete {key}` and `/claim {key, value, ttl_seconds}`. `/claim` stores the
//! value only if the key is free and answers 409 when it is taken: a check and a reservation in
//! one call. A Durable Object handles one request at a time and its storage calls hold other
//! requests back until they finish (input gates), so two claims of the same key cannot both win.
//!
//! Values may expire. An expired value reads as missing, and an alarm set for the earliest expiry
//! deletes every expired value, so nothing outlives its time because no one asked for it again.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use worker::{Date, DurableObject, Env, Request, Response, Result, State, Storage, durable_object};

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

impl Stored {
    fn live(&self, now: u64) -> bool {
        self.expires_at.is_none_or(|at| at > now)
    }
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

fn now() -> u64 {
    Date::now().as_millis()
}

impl BotState {
    async fn read(&self, storage: &Storage, key: &str) -> Result<Option<Stored>> {
        let stored: Option<Stored> = storage.get(key).await?;
        match stored {
            Some(stored) if stored.live(now()) => Ok(Some(stored)),
            Some(_) => {
                storage.delete(key).await?;
                Ok(None)
            }
            None => Ok(None),
        }
    }

    async fn write(&self, storage: &Storage, call: Call) -> Result<()> {
        let expires_at = call
            .ttl_seconds
            .map(|ttl| now().saturating_add(ttl.saturating_mul(1000)));
        storage
            .put(
                &call.key,
                Stored {
                    value: call.value,
                    expires_at,
                },
            )
            .await?;
        if let Some(at) = expires_at {
            self.alarm_by(storage, at).await?;
        }
        Ok(())
    }

    /// The alarm rings no later than `at`.
    async fn alarm_by(&self, storage: &Storage, at: u64) -> Result<()> {
        let current = storage.get_alarm().await?;
        let at = i64::try_from(at).unwrap_or(i64::MAX);
        if current.is_none_or(|current| current > at) {
            let offset =
                u64::try_from(at.saturating_sub(i64::try_from(now()).unwrap_or(0))).unwrap_or(0);
            storage.set_alarm(Duration::from_millis(offset)).await?;
        }
        Ok(())
    }
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
            "/get" => match self.read(&storage, &call.key).await? {
                Some(stored) => Response::from_json(&stored.value),
                None => Response::from_json(&Value::Null),
            },
            "/put" => {
                self.write(&storage, call).await?;
                Response::empty()
            }
            "/claim" => {
                if call.ttl_seconds.is_none() {
                    return Response::error("A claim needs ttl_seconds", 400);
                }
                if self.read(&storage, &call.key).await?.is_some() {
                    return Response::error("Taken", 409);
                }
                self.write(&storage, call).await?;
                Response::empty()
            }
            "/delete" => {
                storage.delete(&call.key).await?;
                Response::empty()
            }
            _ => Response::error("Unknown call", 404),
        }
    }

    /// Deletes every expired value and sets the alarm for the next expiry.
    async fn alarm(&self) -> Result<Response> {
        let storage = self.state.storage();
        let now = now();
        let mut expired: Vec<String> = Vec::new();
        let mut next: Option<u64> = None;
        let all = storage.list().await?;
        for entry in all.entries() {
            let Ok(pair) = entry else { continue };
            let pair = worker::js_sys::Array::from(&pair);
            let Some(key) = pair.get(0).as_string() else {
                continue;
            };
            let Ok(stored) = serde_wasm_bindgen::from_value::<Stored>(pair.get(1)) else {
                continue;
            };
            match stored.expires_at {
                Some(at) if at <= now => expired.push(key),
                Some(at) => next = Some(next.map_or(at, |n| n.min(at))),
                None => {}
            }
        }
        for chunk in expired.chunks(128) {
            storage.delete_multiple(chunk.to_vec()).await?;
        }
        if let Some(at) = next {
            self.alarm_by(&storage, at).await?;
        }
        Response::empty()
    }
}
