---
description: How the Suhina bot works - the request flow, the three crates, how it signs in to GitHub, what it stores, and why
read_when: architecture, how does the bot work, interaction flow, deferred answer, dispatcher, new command, GitHub App token, JWT, WebCrypto, Durable Object, KV, state, webhook, bug report, /bug, /track, review card, clean-room, screenshots, roles
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
| `suhina-bot-core` | logic | Everything that needs no network: Discord's signature check (`verify`), reading interactions (`interaction`), who may do what (`access`), the dispatcher and the commands (`commands`), the `/bug` form (`bug`), the review card (`review`), forum tags (`forum`), screenshot checks (`screenshot`), message bodies (`reply`), the App JWT, key format and board (`github`), webhook signatures (`webhook`), the nightly release (`nightly`). Builds and tests on any machine. |
| `suhina-bot-worker` | Cloudflare | Routes, the network (Discord with `discord_api`, GitHub with `github`), the flows that string them together (`bug`, `track`), settings, the KV token cache, the `BotState` Durable Object and its client (`store`), screenshots into R2 (`screenshots`). As thin as it can be. |
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
                                       DeferUpdate { job }    → a card's button: acknowledged,
                                                                then the card is edited
                                       OpenEdit { key }       → reads the draft, answers with a form
run_job: job_answer (GitHub calls) ──▶ PATCH /webhooks/{app}/{token}/messages/@original
```

Discord waits three seconds for the first answer, so anything that calls GitHub is a `Job`: the
worker answers "thinking…" and finishes in `ctx.wait_until`, then edits that answer. The
interaction's token is the credential for the edit and lasts 15 minutes; no bot token is needed. A
failed job is logged and the user sees a short apology.

Every message is sent with `allowed_mentions: {"parse": []}` (`reply`), so text from GitHub or
from a user can never ping anyone. Text is cut to Discord's limits there too.

## Bug reports: from Discord to an issue

Anyone reports with `/bug`; nothing reaches GitHub until a moderator has reviewed the report. `/track` works the same in `#feature-requests`, where a post becomes a feature request
(`enhancement`, issue type Feature) and the thank-you says it is on the backlog.

```
/bug ── form (core::bug) ──▶ Job::Bug ──▶ worker::bug: post in #bug-reports, platform tag,
                                          screenshots attached; "Reported by" names the reporter
/track in that post ──▶ Job::TrackThread ──▶ worker::track: read the post (title, starter
        message, tags), Draft::from_post, similar issues, board phases ──▶ store draft:<key>
        ──▶ review card (core::review::card), only the moderator sees it
card buttons ──▶ Job::Card / CardEdit / OpenEdit: select screenshots, Replace, Edit, Check
Create issue ──▶ copy selected screenshots to R2 ──▶ issue (labels from tags, type) ──▶ board
        (Backlog, Priority, Phase) ──▶ in the thread: thanks, Tracked tag, ✅ ──▶ tracked:<thread>
```

Why the review: issues are project material that people and agents read, so Suhina's clean-room
rule applies, and other products are named only where a file format is handled (Suhina's
AGENTS.md rules 1 and 8). Reports often compare with another DAW and show its screenshots. The
card marks words from `REVIEW_WORDS` (a secret, so the names are not in this repository), offers
to replace them with "another DAW", offers the screenshots with none selected, and keeps
**Create issue** disabled until the moderator confirms. The issue never names, mentions or links
the reporter or the message.

Who may do what is `core::access`: staff roles (`STAFF_ROLE_IDS`) track reports, triage roles
(`TRIAGE_ROLE_IDS`) also set Priority and Phase, a server administrator is both. A draft lives
15 minutes under a random key that the card's buttons carry (`track:<action>:<key>`).

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
| R2 `SCREENSHOTS` | screenshots a moderator selected for an issue, under random names | public links that issues show; off until set up (docs/operations.md) |
| Durable Object `STATE` (`BotState`) | review drafts (`draft:<key>`, 15 minutes), the issue each post became (`tracked:<thread>`), `/bug`'s one-a-minute limit (`bug-rate:<user>`); later account links and pending claims. Each with an optional expiry | one instance, strongly consistent: a GitHub webhook that arrives at another location still finds the claim it answers. KV can take a minute to show a write elsewhere. |

The Durable Object takes `/get`, `/put` (with `ttl_seconds`) and `/delete`, as JSON on a made-up
URL (`worker::store` is its client); an expired value reads as missing and is removed. A draft
holds a report's text for its 15 minutes; nothing else keeps message text, and no OAuth token is
stored anywhere.

## The webhook

`POST /github/webhook` checks `X-Hub-Signature-256` against `GITHUB_WEBHOOK_SECRET` (401 if
wrong). The App subscribes to `issue_comment` only, so the claim bot's answers can be relayed back
to Discord (#9); until then deliveries are accepted and dropped.

## Errors

No `unwrap`, `expect` or `panic!` outside tests (the workspace lints deny them). An error becomes a
plain 500 to the caller, or the apology in Discord, and a log line. `core::Error` never holds a
secret or message text, so logging it is safe.
