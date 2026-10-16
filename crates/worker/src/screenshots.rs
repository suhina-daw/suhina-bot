// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Copies a screenshot a moderator selected into the R2 bucket `SCREENSHOTS`, served read-only
//! at `SCREENSHOTS_URL`, and returns its permanent link (#10). Nothing
//! else is ever copied: what may go in is decided in core (`screenshot::check`).

use suhina_bot_core::interaction::Attachment;
use suhina_bot_core::screenshot;
use web_sys::WorkerGlobalScope;
use worker::wasm_bindgen::JsCast;
use worker::{Env, Error, HttpMetadata, Result};

use crate::discord_api::download;
use crate::settings::Settings;

pub async fn copy(env: &Env, settings: &Settings, shot: &Attachment) -> Result<String> {
    let base = settings
        .screenshots_url
        .as_deref()
        .ok_or_else(|| Error::RustError("SCREENSHOTS_URL is not set".into()))?;
    let bucket = env.bucket("SCREENSHOTS")?;
    let bytes = download(&shot.url).await?;
    let image = screenshot::check(shot.content_type.as_deref(), &bytes)
        .map_err(|refused| Error::RustError(format!("screenshot refused: {refused:?}")))?;
    let key = screenshot::key(&random_uuid()?, image)
        .ok_or_else(|| Error::RustError("no random name".into()))?;
    bucket
        .put(&key, bytes)
        .http_metadata(HttpMetadata {
            content_type: Some(image.content_type().to_owned()),
            cache_control: Some("public, max-age=31536000, immutable".to_owned()),
            ..HttpMetadata::default()
        })
        .execute()
        .await?;
    Ok(format!("{base}/{key}"))
}

/// A random UUID from WebCrypto.
pub fn random_uuid() -> Result<String> {
    let global: WorkerGlobalScope = worker::js_sys::global().unchecked_into();
    Ok(global.crypto()?.random_uuid())
}
