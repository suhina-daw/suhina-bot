// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading a request body with a size limit, before its signature is checked: an unsigned
//! request must not be able to fill the Worker's memory. A declared `Content-Length` over the
//! limit is refused without reading; otherwise reading stops as soon as the limit is passed.

use futures_util::StreamExt;
use worker::{Request, Result};

/// Discord's interactions are small; 64 KB is far above any of them.
pub const INTERACTION_LIMIT: usize = 64 * 1024;
/// GitHub caps a webhook delivery's payload at 25 MB.
pub const WEBHOOK_LIMIT: usize = 25 * 1024 * 1024;

/// The body, or `None` when it is larger than `limit`.
pub async fn read(req: &mut Request, limit: usize) -> Result<Option<Vec<u8>>> {
    let declared = req
        .headers()
        .get("Content-Length")?
        .and_then(|length| length.parse::<usize>().ok());
    if declared.is_some_and(|length| length > limit) {
        return Ok(None);
    }
    let mut body = Vec::with_capacity(declared.unwrap_or(0).min(limit));
    let mut stream = req.stream()?;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if body.len().saturating_add(chunk.len()) > limit {
            return Ok(None);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Some(body))
}
