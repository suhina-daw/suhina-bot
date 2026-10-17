---
description: How the Suhina bot works - the request flow, the three crates, how it signs in to GitHub, what it stores, and why
read_when: architecture, how does the bot work, interaction flow, deferred answer, dispatcher, new command, GitHub App token, JWT, WebCrypto, Durable Object, state, claim, alarm, replay, timestamp, guild, home server, body size, webhook, bug report, /bug, /track, review card, clean-room, screenshots, roles
type: system
status: current
last_verified: 2026-10-17
covers: crates/core, crates/worker, crates/register
---

# Architecture

The bot is a Cloudflare Worker written in Rust. Discord calls it over HTTP for each command; between
commands nothing runs, and it never reads messages on its own. It writes to GitHub as the App
`suhina-bot` (`suhina-bot[bot]`).

## Crates

| Crate | Layer | What it holds |
|---|---|---|
| `suhina-bot-core` | logic | Everything that needs no network: Discord's signature check (`verify`), reading interactions (`interaction`), who may do what (`access`), the dispatcher and the commands (`commands`), the `/bug` form (`bug`), the review card (`review`), forum tags (`forum`), message bodies (`reply`), the App JWT, key format and board (`github`), webhook signatures (`webhook`), the nightly release (`nightly`). Builds and tests on any machine. |
| `suhina-bot-worker` | Cloudflare | Routes, the network (Discord with `discord_api`, GitHub with `github`), the flows that string them together (`bug`, `track`), settings (`settings`), size-limited bodies (`body`), the `BotState` Durable Object and its client (`store`). As thin as it can be. |
| `suhina-bot-register` | tool | Registers the commands with Discord and writes `docs/commands.md`. |

The rule: a decision goes in core, with a test; the worker only carries data in and out. Core
never depends on `worker`.

## An interaction, step by step

```
Discord ──POST /interactions──▶ worker::discord::handle
                                  1. read the body, at most 64 KB (413 if larger)
                                  2. verify Ed25519 over timestamp + body (401 if wrong)
                                  3. the timestamp within 5 minutes (401 if not)
                                  4. Interaction::parse; claim interaction:<id> (409 if seen)
                                  5. core::access: home server? staff? triage?
                                  6. core::commands::route ──▶ Action
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
from a user can never ping anyone. Text is cut to Discord's limits there too, and an issue is
always named as a masked link, `[#612](<url>)`.

**A signed request is good once.** Discord signs the timestamp with the body; a request older than
five minutes is refused, and each interaction id is claimed (step 4) before anything happens, so a
captured request cannot be sent again.

**Only Suhina's server.** `core::access` gives nobody any rights outside the server named by
`DISCORD_GUILD_ID`, and the commands that write answer *This works in the Suhina server only*
there. An administrator of another server the app was added to is nobody. Without the setting
nothing writes.

## Bug reports: from Discord to an issue

Anyone reports with `/bug`; nothing reaches GitHub until a moderator has reviewed the report. `/track` works the same in `#feature-requests`, where a post becomes a feature request
(`enhancement`, issue type Feature) and the thank-you says it is on the backlog.

```
/bug ── form (core::bug) ──▶ Job::Bug ──▶ worker::bug: claim bug-rate:<user>, post in
                                          #bug-reports, platform tag, screenshots attached
                                          (each ≤ 10 MB, all ≤ 25 MB); "Reported by" names
                                          the reporter
/track in that post ──▶ Job::TrackThread ──▶ worker::track: read the post (title, starter
        message, tags), Draft::from_post, similar issues, board phases ──▶ store draft:<key>
        ──▶ review card (core::review::card), only the moderator sees it
Add as bug / Add to backlog on a message ──▶ Job::TrackMessage ──▶ Draft::from_message (its
        channel and id); on a post's starter message the post's draft, as /track makes it
card buttons ──▶ Job::Card / CardEdit / OpenEdit (the card's owner only): Replace, Edit, Check
Create issue ──▶ claim creating:<message> ──▶ issue (labels from tags, type) ──▶ tracked:<message>
        ──▶ board (Backlog, Priority, Phase) ──▶ a post: thanks in the thread and its Tracked tag;
        a message: a reply ──▶ the TRACKED_EMOJI reaction (✅ when unset) on the report
```

Why the review: issues are project material that people and agents read, so Suhina's clean-room
rule applies, and other products are named only where a file format is handled (Suhina's
AGENTS.md rules 1 and 8). Reports often compare with another DAW and show its screenshots. The
card marks words from `REVIEW_WORDS` (a secret, so the names are not in this repository), offers
to replace them with "another DAW", and keeps **Create issue** disabled until the moderator
confirms. Screenshots never leave Discord, since they may show another product; the issue counts
them. The issue never names, mentions or links the reporter or the message.

**An issue is created once.** Create claims `creating:<message>` before calling GitHub (a post's
starter message has the thread's id, so `/track` and a message command on it share the key), so a
second press, a second moderator's card for the same report or a replay finds the claim and
creates nothing; the report is recorded as tracked as soon as the issue exists, before the board
and Discord are touched, and a new card for it says which issue it already is.

Who may do what is `core::access`: staff roles (`STAFF_ROLE_IDS`) track reports, triage roles
(`TRIAGE_ROLE_IDS`) also set Priority and Phase, an administrator of Suhina's server is both. A
draft lives 15 minutes under a random key that the card's buttons carry (`track:<action>:<key>`),
and only the moderator who opened it may use them.

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
3. The token is kept in the Durable Object for 50 minutes (it lives 60), so most commands sign
   nothing.

Issue creation is retried without the issue type only when GitHub refused the type itself (a 422
naming it); any other failure may already have created the issue, and a retry would open a second.

WebCrypto and not a Rust RSA crate: it is native, so signing costs almost no Worker CPU time, and
the `rsa` crate has an advisory without a fix (RUSTSEC-2023-0071) that `cargo deny` refuses.

## State

| Where | What | Why there |
|---|---|---|
| Durable Object `STATE` (`BotState`) | the GitHub installation token (`github-token`, 50 minutes), `/help`'s answer (`help-text`, 5 minutes), interactions seen (`interaction:<id>`, 15 minutes), review drafts (`draft:<key>`, 15 minutes), issues being created (`creating:<post or key>`, 15 minutes), the issue each post became (`tracked:<thread>`), `/bug`'s one-a-minute limit (`bug-rate:<user>`); later account links and pending claims | one instance, strongly consistent: a GitHub webhook that arrives at another location still finds the claim it answers, and a claim is decided in one place. It needs no ID in the configuration. |

The Durable Object takes `/get`, `/put` (with `ttl_seconds`), `/claim` and `/delete`, as JSON on a
made-up URL (`worker::store` is its client). `/claim` stores a value only if the key is free and
answers 409 if it is taken: one call, and a Durable Object handles one request at a time with its
storage calls holding others back (input gates), so two claims cannot both win. An expired value
reads as missing, and an alarm set for the earliest expiry deletes every expired value, so a
draft left on its card does not outlive its 15 minutes. A draft holds a report's text for that
time; nothing else keeps message text, and no OAuth token is stored anywhere.

## The webhook

`POST /github/webhook` reads at most 25 MB (GitHub's own limit; 413 if larger) and checks
`X-Hub-Signature-256` against `GITHUB_WEBHOOK_SECRET` (401 if wrong). The App subscribes to `issue_comment` only, so the claim bot's answers can be relayed back
to Discord (#7); until then deliveries are accepted and dropped.

## Errors

No `unwrap`, `expect` or `panic!` outside tests (the workspace lints deny them). An error becomes a
plain 500 to the caller, or the apology in Discord, and a log line. `core::Error` never holds a
secret or message text, so logging it is safe.
