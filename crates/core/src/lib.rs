// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the Suhina bot does, kept apart from Cloudflare so that it builds and is tested on any
//! machine: checking that a request comes from Discord or GitHub, reading an interaction,
//! deciding the answer, and the pieces of a GitHub App token. `suhina-bot-worker` is the thin
//! layer that runs it on Cloudflare Workers.

pub mod commands;
pub mod error;
pub mod github;
pub mod interaction;
pub mod nightly;
pub mod reply;
pub mod verify;
pub mod webhook;

pub use error::Error;
