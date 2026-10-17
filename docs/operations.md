---
description: Running the Suhina bot - resources, settings and secrets, Discord setup, deploying, rotating keys, registering commands, repository protection, logs and troubleshooting
read_when: repository protection, ruleset, branch protection, dependabot, secret scanning, pinned actions, deploy, wrangler, secret, settings, rotate key, bot token, private key, register commands, logs, wrangler tail, interactions endpoint, Durable Object, troubleshooting, GitHub could not be reached, role IDs, guild, staff roles, bug-reports forum, Message Content intent, command permissions, REVIEW_WORDS, screenshots
type: howto
status: current
last_verified: 2026-10-17
covers: crates/worker/wrangler.toml, SECURITY.md, .github/CODEOWNERS, .github/dependabot.yml, crates/worker/src/settings.rs, crates/register, .github/workflows/ci.yml
---

# Operations

Everything here is done by hand by a maintainer. CI never deploys and never sees a secret.

**No ID, key or account URL is written in this repository.** Every one is a Worker secret, and the
values live in the maintainer's password manager. Where a command below needs one, it reads it
from there.

## Resources

| What | Where |
|---|---|
| Worker `suhina-bot` | Cloudflare, Workers Free plan; its URL is shown by `npx wrangler deploy` |
| Durable Object `STATE` (`BotState`) | in the Worker; SQLite-backed (migration `v1`); everything the bot keeps |
| Discord application **Suhina** | Discord Developer Portal |
| Interactions Endpoint URL | Developer Portal › General Information: the Worker's URL + `/interactions` |
| GitHub App `suhina-bot` (*Suhina[bot]*) | suhina-daw › Settings › GitHub Apps; installed on `suhina-daw/suhina-daw` only |

The same Discord application runs the scripts in Suhina's repository that mark merged pull request
threads; they use its bot token from the GitHub secret `DISCORD_BOT_TOKEN` (Suhina's
`docs/releasing.md` › Discord).

## Plan and limits

The Free plan is enough: 100 000 requests a day and SQLite Durable Objects. The limit that
matters is 10 ms of CPU per request. The App JWT is signed with WebCrypto (native, not
WebAssembly) and only when the cached installation token has run out, and `/help`'s answer is
kept five minutes; waiting for GitHub or Discord costs no CPU. Check Workers & Pages ›
suhina-bot › Metrics › CPU Time; if the highest values approach 10 ms, move to Workers Paid or
apply to Cloudflare's open source programme, Project Alexandria.

## Settings

All secrets, set from `crates/worker` with `npx wrangler secret put <NAME>`. `npx wrangler secret
list` shows the names. A value is never written to a file in the repository, a log, or a command
line that is kept in history: paste it at the prompt, or pipe it from the password manager.

| Secret | What | Where to find it |
|---|---|---|
| `DISCORD_APPLICATION_ID` | the Discord application | Developer Portal › General Information |
| `DISCORD_PUBLIC_KEY` | its public key; checks that a request comes from Discord | Developer Portal › General Information |
| `DISCORD_BOT_TOKEN` | reading a post, making posts, tags and reactions | Developer Portal › Bot › Reset Token (see Rotating) |
| `DISCORD_GUILD_ID` | Suhina's server: the only one where anything writes. Unset: nothing writes | Discord with Developer Mode on (User Settings › Advanced): right-click the server › Copy Server ID |
| `BUG_REPORTS_FORUM_ID` | the forum `bug-reports`: `/bug` posts there, `/track` there makes a bug | right-click the channel › Copy Channel ID |
| `FEATURE_REQUESTS_FORUM_ID` | the forum `feature-requests`: `/track` there makes a feature request | as above |
| `STAFF_ROLE_IDS` | roles that may use `/track` and the message commands (`Mod`, `github-mod`, `github-contributor`), comma-separated | Server Settings › Roles › right-click a role › Copy Role ID |
| `TRIAGE_ROLE_IDS` | roles that also set Priority and Phase (`github-mod`) | as above |
| `REVIEW_WORDS` | a JSON array of product names the review card marks, e.g. `'["…", "…"]'`; include old and new names of a renamed product. Unset: the card says the check is off | the maintainer's list |
| `GITHUB_APP_ID` | the GitHub App | the App's General page |
| `GITHUB_APP_PRIVATE_KEY` | the App's private key, the whole `.pem` | the App's Credentials › Key pairs |
| `GITHUB_WEBHOOK_SECRET` | the App's webhook secret; not set until the webhook is turned on (#9) | chosen when the webhook is set up |

`[vars]` in `wrangler.toml` holds only names: `GITHUB_REPOSITORY`, `BOARD_OWNER`, `BOARD_NUMBER`.
A server administrator of Suhina's server counts as staff and triage without a role; an
administrator of any other server is nobody. An empty setting turns its feature off, and the bot
says which setting is missing.

A `.pem` has many lines and wrangler's prompt takes one, so pipe it (PowerShell has no `<`):

```powershell
Get-Content path\to\key.pem -Raw | npx wrangler secret put GITHUB_APP_PRIVATE_KEY
```

### Discord setup for `/bug` and `/track`

1. Developer Portal › Installation: the app is installed to Suhina's server only, and **Public
   Bot** is off (Bot tab), so no one else can add it. The Worker refuses other servers anyway.
2. Developer Portal › Bot › **Message Content Intent** on. The bot reads a post's starter message
   only when `/track` is used in it, never otherwise.
3. In `bug-reports` and `feature-requests`, give the bot (Discord's names in a forum): View
   Channel, Create Posts, Send Messages in Posts, Attach Files, Read Post History, Add Reactions,
   Manage Posts (tags).
4. `bug-reports` has the tags Mac, Windows, Linux (for `/bug`); both forums have **Tracked** (set
   by `/track`; staff only, so reporters cannot set it). Names are matched ignoring case. Tags
   that become labels are listed in `crates/core/src/review.rs` (`TAG_LABELS`).
5. Server Settings › Integrations › Suhina: allow `/track`, **Add as bug** and **Add to backlog**
   for the staff roles. They are hidden from everyone else by default; the Worker checks the
   roles anyway.

Screenshots stay in Discord: the bot never copies them anywhere, and an issue says how many the
report has.

## Deploying

```sh
cd crates/worker
npx wrangler deploy
```

`wrangler deploy` runs `worker-build --release` first (needs `cargo install worker-build` and the
`wasm32-unknown-unknown` target) and prints the Worker's URL. Check it afterwards:

```sh
curl <worker URL>/                                   # 200 and a line of text
curl -X POST <worker URL>/interactions -d '{}'       # 401
```

and `/help` in the server answers.

## Rotating keys

### The App's private key

1. GitHub: suhina-daw › Settings › GitHub Apps › suhina-bot › **Credentials › Key pairs** ›
   **New key**. The `.pem` downloads.
2. Put it in the Worker (above) and in Suhina's repository: environment `suhina-bot`, secret
   `SUHINA_BOT_PRIVATE_KEY` (used by `open-issue.yml`). Store it in the password manager; delete
   the file.
3. Delete the old key on the same page. Tokens made with it stop working within the hour; the
   Worker's cached token lasts at most 50 minutes.

### The Discord bot token

Developer Portal › Bot › **Reset Token**. The old one stops at once, so update both places in the
same go: the GitHub secret `DISCORD_BOT_TOKEN` in Suhina's repository (`gh secret set
DISCORD_BOT_TOKEN --repo suhina-daw/suhina-daw`) and the Worker secret.

## Registering commands

After a change to the commands, from the repository root:

```powershell
$env:DISCORD_APPLICATION_ID = Read-Host "Application ID"
$env:DISCORD_BOT_TOKEN = Read-Host "Bot token" -MaskInput
cargo run -p suhina-bot-register -- --guild (Read-Host "Server ID")
Remove-Item Env:DISCORD_APPLICATION_ID, Env:DISCORD_BOT_TOKEN
```

`--guild` registers for that server and shows at once; without it the commands are global and can
take a while to appear. The list replaces the old one, so a removed command disappears.
`--print` shows the JSON without sending it.

## Repository protection

- **`main`** (ruleset *main*): no deletion, no force push, changes only by pull request with one
  approval (the last push approved, threads resolved), squash merge only, and the CI jobs *fmt,
  clippy and tests*, *licence headers* and *cargo deny* must pass. Repository admins may bypass
  through a pull request. Merged branches are deleted.
- **Tags `v*`** (ruleset *release tags*): cannot be moved or deleted.
- **Code security**: Dependabot alerts and security updates (`.github/dependabot.yml` adds weekly
  version updates), secret scanning with push protection, private vulnerability reporting
  (SECURITY.md). Non-provider patterns and validity checks need GitHub Secret Protection, which
  the organization does not have.
- **Actions**: every action is pinned to a commit (the repository requires it; Dependabot updates
  the pins); the workflow token is read-only and cannot approve pull requests; workflows from
  every outside contributor's fork wait for a maintainer's approval. CI holds no secrets.
- **Code owners**: `.github/CODEOWNERS` names the maintainer for every path.

## Logs

```sh
cd crates/worker
npx wrangler tail
```

shows each request and its log lines live. Observability is on, so the Cloudflare dashboard ›
Workers › suhina-bot › Logs keeps them too. Log lines hold no secrets and no message text.

## Troubleshooting

| Symptom | Likely cause |
|---|---|
| Discord refuses to save the Interactions Endpoint URL | the Worker is not deployed, or `DISCORD_PUBLIC_KEY` is not the application's |
| "The application did not respond" in Discord | the Worker failed before answering: `wrangler tail` (a missing setting is named there) |
| `/help` says GitHub could not be reached | the private key or App ID is missing or wrong, or the App is not installed on the repository: `wrangler tail` shows GitHub's answer |
| "Something went wrong on Suhina's side" | a job failed; the log line names it |
| A new command does not show | not registered yet, or registered globally and still spreading |
| "This works in the Suhina server only" in Suhina's server | `DISCORD_GUILD_ID` is unset or not this server's ID |
| `/track` says "Use /track inside a post" in a post | `BUG_REPORTS_FORUM_ID` or `FEATURE_REQUESTS_FORUM_ID` is not that forum's ID |
| "Only moderators and contributors…" for a moderator | their role is not in `STAFF_ROLE_IDS` |
| The card shows an empty text | Message Content Intent is off |
| "the Tracked tag could not be set" | the forum has no Tracked tag, or the bot lacks Manage Posts there |
