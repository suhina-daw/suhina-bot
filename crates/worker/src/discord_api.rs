// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Discord's REST API. Editing an interaction's answer needs only the interaction's token; the
//! rest (reading a post, making one, tags, reactions) needs the bot token, the Worker secret
//! `DISCORD_BOT_TOKEN`.

use futures_util::StreamExt;
use serde_json::Value;
use suhina_bot_core::reply::shorten;
use web_sys::{Blob, FormData};
use worker::js_sys::{Array, Uint8Array};
use worker::wasm_bindgen::JsValue;
use worker::{Env, Error, Fetch, Headers, Method, Request, RequestInit, Result};

pub const API: &str = "https://discord.com/api/v10";
const USER_AGENT: &str = "DiscordBot (https://github.com/suhina-daw/suhina-bot, 0.1)";

/// Largest attachment downloaded from Discord.
pub const MAX_DOWNLOAD: usize = 10 * 1024 * 1024;

pub struct Discord {
    token: Option<String>,
}

/// A file to attach to a message.
pub struct Upload {
    pub filename: String,
    pub bytes: Vec<u8>,
}

impl Discord {
    pub fn new(env: &Env) -> Self {
        Self {
            token: env
                .secret("DISCORD_BOT_TOKEN")
                .ok()
                .map(|secret| secret.to_string()),
        }
    }

    fn token(&self) -> Result<&str> {
        self.token
            .as_deref()
            .ok_or_else(|| Error::RustError("the secret DISCORD_BOT_TOKEN is not set".into()))
    }

    async fn call(&self, method: Method, path: &str, json: Option<&Value>) -> Result<Value> {
        let headers = Headers::new();
        headers.set("Authorization", &format!("Bot {}", self.token()?))?;
        headers.set("User-Agent", USER_AGENT)?;
        let mut init = RequestInit::new();
        init.with_method(method);
        if let Some(json) = json {
            headers.set("Content-Type", "application/json")?;
            init.with_body(Some(JsValue::from_str(&json.to_string())));
        }
        init.with_headers(headers);
        send(&format!("{API}{path}"), &init).await
    }

    pub async fn channel(&self, id: &str) -> Result<Value> {
        self.call(Method::Get, &format!("/channels/{id}"), None)
            .await
    }

    pub async fn message(&self, channel: &str, id: &str) -> Result<Value> {
        self.call(
            Method::Get,
            &format!("/channels/{channel}/messages/{id}"),
            None,
        )
        .await
    }

    /// A message with no mentions parsed.
    pub async fn send_message(&self, channel: &str, content: &str) -> Result<Value> {
        let body = serde_json::json!({ "content": content, "allowed_mentions": { "parse": [] } });
        self.call(
            Method::Post,
            &format!("/channels/{channel}/messages"),
            Some(&body),
        )
        .await
    }

    /// A reply to a message, with no mentions parsed; still sent when the message is gone.
    pub async fn reply(&self, channel: &str, message: &str, content: &str) -> Result<Value> {
        let body = serde_json::json!({
            "content": content,
            "allowed_mentions": { "parse": [] },
            "message_reference": { "message_id": message, "fail_if_not_exists": false },
        });
        self.call(
            Method::Post,
            &format!("/channels/{channel}/messages"),
            Some(&body),
        )
        .await
    }

    /// `emoji` as Discord takes it in the URL, e.g. `%E2%9C%85` for ✅.
    pub async fn react(&self, channel: &str, message: &str, emoji: &str) -> Result<()> {
        self.call(
            Method::Put,
            &format!("/channels/{channel}/messages/{message}/reactions/{emoji}/@me"),
            None,
        )
        .await
        .map(|_| ())
    }

    pub async fn set_tags(&self, thread: &str, tags: &[String]) -> Result<()> {
        let body = serde_json::json!({ "applied_tags": tags });
        self.call(Method::Patch, &format!("/channels/{thread}"), Some(&body))
            .await
            .map(|_| ())
    }

    /// A post in a forum channel: its title, tags, first message and attached files. Returns the
    /// new thread.
    pub async fn create_post(
        &self,
        forum: &str,
        title: &str,
        tags: &[String],
        content: &str,
        files: &[Upload],
    ) -> Result<Value> {
        let attachments: Vec<Value> = files
            .iter()
            .enumerate()
            .map(|(i, file)| serde_json::json!({ "id": i, "filename": file.filename }))
            .collect();
        let payload = serde_json::json!({
            "name": title,
            "applied_tags": tags,
            "message": {
                "content": content,
                "allowed_mentions": { "parse": [] },
                "attachments": attachments,
            },
        });
        let form = FormData::new()?;
        form.append_with_str("payload_json", &payload.to_string())?;
        for (i, file) in files.iter().enumerate() {
            let bytes = Uint8Array::from(file.bytes.as_slice());
            let blob = Blob::new_with_u8_array_sequence(&Array::of1(&bytes))?;
            form.append_with_blob_and_filename(&format!("files[{i}]"), &blob, &file.filename)?;
        }
        let headers = Headers::new();
        headers.set("Authorization", &format!("Bot {}", self.token()?))?;
        headers.set("User-Agent", USER_AGENT)?;
        let mut init = RequestInit::new();
        init.with_method(Method::Post)
            .with_headers(headers)
            .with_body(Some(JsValue::from(form)));
        send(&format!("{API}/channels/{forum}/threads"), &init).await
    }
}

/// `PATCH /webhooks/{application}/{token}/messages/@original`: the interaction's token is the
/// credential, valid for 15 minutes; no bot token needed.
pub async fn edit_original(application: &str, token: &str, body: &Value) -> Result<()> {
    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;
    let mut init = RequestInit::new();
    init.with_method(Method::Patch)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(&body.to_string())));
    send(
        &format!("{API}/webhooks/{application}/{token}/messages/@original"),
        &init,
    )
    .await
    .map(|_| ())
}

/// A file from Discord's CDN, at most `limit` bytes. The declared length is checked before
/// reading, and reading stops as soon as the limit is passed, so a large file never fills memory.
pub async fn download(url: &str, limit: usize) -> Result<Vec<u8>> {
    if !url.starts_with("https://cdn.discordapp.com/")
        && !url.starts_with("https://media.discordapp.net/")
    {
        return Err(Error::RustError("not a Discord attachment URL".into()));
    }
    let mut response = Fetch::Request(Request::new(url, Method::Get)?)
        .send()
        .await?;
    if response.status_code() != 200 {
        return Err(Error::RustError(format!(
            "attachment download answered {}",
            response.status_code()
        )));
    }
    let too_large = || Error::RustError("attachment too large".into());
    let declared = response
        .headers()
        .get("Content-Length")?
        .and_then(|length| length.parse::<usize>().ok());
    if declared.is_some_and(|length| length > limit) {
        return Err(too_large());
    }
    let mut bytes = Vec::with_capacity(declared.unwrap_or(0).min(limit));
    let mut stream = response.stream()?;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(too_large());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn send(url: &str, init: &RequestInit) -> Result<Value> {
    let mut response = Fetch::Request(Request::new_with_init(url, init)?)
        .send()
        .await?;
    let status = response.status_code();
    let text = response.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(Error::RustError(format!(
            "Discord answered {status}: {}",
            shorten(&text, 300)
        )));
    }
    if text.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text)
        .map_err(|_| Error::RustError(format!("Discord answered {status} with no JSON")))
}
