// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Registers the bot's commands with Discord: replaces the whole list with the one in
//! `suhina_bot_core::commands::definitions`, so a removed command disappears too.
//!
//! ```sh
//! DISCORD_APPLICATION_ID=... DISCORD_BOT_TOKEN=... cargo run -p suhina-bot-register
//! DISCORD_APPLICATION_ID=... DISCORD_BOT_TOKEN=... cargo run -p suhina-bot-register -- --guild <server id>
//! cargo run -p suhina-bot-register -- --print
//! cargo run -p suhina-bot-register -- --markdown > docs/commands.md
//! ```
//!
//! Commands for one server (`--guild`) show at once; global ones can take a while.

use std::process::ExitCode;

use suhina_bot_core::commands::{definitions, markdown};

const DISCORD_API: &str = "https://discord.com/api/v10";

fn main() -> ExitCode {
    match run() {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<String, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let commands = definitions();
    if args.iter().any(|arg| arg == "--print") {
        return serde_json::to_string_pretty(&commands).map_err(|error| error.to_string());
    }
    if args.iter().any(|arg| arg == "--markdown") {
        // `println!` adds the file's last line break.
        return Ok(markdown(&commands).trim_end().to_owned());
    }
    let guild = match args.as_slice() {
        [] => None,
        [flag, id] if flag == "--guild" && id.chars().all(|c| c.is_ascii_digit()) => {
            Some(id.as_str())
        }
        _ => {
            return Err(
                "usage: suhina-bot-register [--guild <server id>] | --print | --markdown".into(),
            );
        }
    };

    let application = env("DISCORD_APPLICATION_ID")?;
    let token = env("DISCORD_BOT_TOKEN")?;
    let url = match guild {
        Some(guild) => format!("{DISCORD_API}/applications/{application}/guilds/{guild}/commands"),
        None => format!("{DISCORD_API}/applications/{application}/commands"),
    };

    let mut response = ureq::put(&url)
        .header("Authorization", &format!("Bot {token}"))
        .header("Content-Type", "application/json")
        .config()
        .http_status_as_error(false)
        .build()
        .send(commands.to_string())
        .map_err(|error| format!("could not reach Discord: {error}"))?;
    let status = response.status();
    let body = response.body_mut().read_to_string().unwrap_or_default();
    if !status.is_success() {
        return Err(format!("Discord answered {status}: {body}"));
    }
    let count = commands.as_array().map_or(0, Vec::len);
    let place = guild.map_or_else(
        || "globally".to_owned(),
        |guild| format!("in server {guild}"),
    );
    Ok(format!("Registered {count} commands {place}."))
}

fn env(name: &str) -> Result<String, String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{name} is not set"))
}
