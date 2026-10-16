// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The one error type of the core crate. Its messages never hold a secret or the text of a
//! Discord message, since the worker logs them.

/// Everything that can go wrong in the core crate.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    /// A signature header is missing, malformed or does not match the body.
    #[error("bad signature")]
    BadSignature,
    /// A configured key (Discord public key, GitHub private key) cannot be read.
    #[error("bad key: {0}")]
    BadKey(&'static str),
    /// The request body is not the JSON the bot expects.
    #[error("bad request body: {0}")]
    BadBody(String),
    /// GitHub answered with something the bot does not understand.
    #[error("unexpected GitHub answer: {0}")]
    GitHub(String),
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        // Only the position and kind: serde's message can quote the input, which may be a
        // Discord message.
        Error::BadBody(format!(
            "{:?} at line {} column {}",
            error.classify(),
            error.line(),
            error.column()
        ))
    }
}
