---
description: How the Suhina bot works - the request flow, the three crates, how it signs in to GitHub, what it stores, and why
read_when: architecture, how does the bot work, interaction flow, deferred answer, dispatcher, new command, GitHub App token, JWT, WebCrypto, Durable Object, KV, state, webhook
type: system
status: current
last_verified: 2026-10-16
covers: crates/core, crates/worker, crates/register
---

# Architecture

The bot is a Cloudflare Worker written in Rust. Discord calls it over HTTP for each command; between
commands nothing runs, and it never reads messages on its own. It writes to GitHub as the App
`suhina-bot` (`suhina-bot[bot]`).

## Crates

| Crate | Layer | What it holds |
|---|---|---|
| `suhina-bot-core` | logic | Everything that needs no network: Discord's signature check (`verify`), reading interactions (`interaction`), the dispatcher and the commands (`commands`), message bodies (`reply`), the App JWT and key format (`github`), webhook signatures (`webhook`), the nightly release (`nightly`). Builds and tests on any machine. |
| `suhina-bot-worker` | Cloudflare | Routes, the network (Discord, GitHub), the KV token cache and the `BotState` Durable Object. As thin as it can be. |
| `suhina-bot-register` | tool | Registers the commands with Discord and writes `docs/commands.md`. |

The rule: a decision goes in core, with a test; the worker only carries data in and out. Core
never depends on `worker`.

## An interaction, step by step

```
Discord ──POST /interactions──▶ worker::discord::handle
                                  1. verify Ed25519 over timestamp + body (401 if wrong)
                                  2. Interaction::parse
                                  3. core::commands::route ──▶ Action
                                       Reply(body)            → answered at once
                                       Defer { job, private } → "thinking…" at once, then
                                                                ctx.wait_until(run_job)
run_job: job_answer (GitHub calls) ──▶ PATCH /webhooks/{app}/{token}/messages/@original
```

Discord waits three seconds for the first answer, so anything that calls GitHub is a `Job`: the
worker answers "thinking…" and finishes in `ctx.wait_until`, then edits that answer. The
interaction's token is the credential for the edit and lasts 15 minutes; no bot token is needed. A
failed job is logged and the user sees a short apology.

Every message is sent with `allowed_mentions: {"parse": []}` (`reply`), so text from GitHub or
from a user can never ping anyone. Text is cut to Discord's limits there too.

## Adding a command

1. In `crates/core/src/commands.rs`, a module with its `definition()` (Discord's JSON) and its
   answer, with tests; add the definition to `definitions()` and a branch to `route`.
2. If it calls GitHub, a `Job` variant, and its branch in `worker::discord::job_answer`.
3. `cargo run -p suhina-bot-register -- --markdown > docs/commands.md` (CI checks it).
4. After the merge: deploy and register the commands (docs/operations.md).

`/help` lists the commands from `definitions()`, so it never falls behind.

## GitHub as the App

`worker::github::GitHub` calls REST and GraphQL with an installation token:

1. The App JWT: `core::github::jwt_signing_input` (issued a minute back, valid nine minutes),
   signed RS256 with WebCrypto in the Worker. GitHub hands out the private key as PKCS#1;
   WebCrypto imports only PKCS#8, so `core::github::pkcs8_der` wraps it.
2. `GET /repos/{repo}/installation` and `POST /app/installations/{id}/access_tokens` with the JWT.
3. The token is kept in KV for 50 minutes (it lives 60), so most commands sign nothing.

WebCrypto and not a Rust RSA crate: it is native, so signing costs almost no Worker CPU time, and
the `rsa` crate has an advisory without a fix (RUSTSEC-2023-0071) that `cargo deny` refuses.

## State

| Where | What | Why there |
|---|---|---|
| KV `CACHE` | the installation token, 50 minutes | a cache; a stale read only costs a new token |
| Durable Object `STATE` (`BotState`) | account links (Discord ID → GitHub account) and pending claims, each with an optional expiry | one instance, strongly consistent: a GitHub webhook that arrives at another location still finds the claim it answers. KV can take a minute to show a write elsewhere. |

The Durable Object takes `/get`, `/put` (with `ttl_seconds`) and `/delete`, as JSON on a made-up
URL; an expired value reads as missing and is removed. No message text and no OAuth token is
stored anywhere.

## The webhook

`POST /github/webhook` checks `X-Hub-Signature-256` against `GITHUB_WEBHOOK_SECRET` (401 if
wrong). The App subscribes to `issue_comment` only, so the claim bot's answers can be relayed back
to Discord (#9); until then deliveries are accepted and dropped.

## Errors

No `unwrap`, `expect` or `panic!` outside tests (the workspace lints deny them). An error becomes a
plain 500 to the caller, or the apology in Discord, and a log line. `core::Error` never holds a
secret or message text, so logging it is safe.
