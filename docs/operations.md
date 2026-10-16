---
description: Running the Suhina bot - Cloudflare and GitHub resources, deploying, secrets and rotating them, registering commands, logs and troubleshooting
read_when: repository protection, ruleset, branch protection, dependabot, secret scanning, deploy, wrangler, secret, rotate key, bot token, private key, register commands, logs, wrangler tail, interactions endpoint, KV id, Durable Object, troubleshooting, GitHub could not be reached
type: howto
status: current
last_verified: 2026-10-16
covers: crates/worker/wrangler.toml, SECURITY.md, .github/CODEOWNERS, .github/dependabot.yml, crates/register, .github/workflows/ci.yml
---

# Operations

Everything here is done by hand by a maintainer. CI never deploys and never sees a secret.

## Resources

| What | Where | Value |
|---|---|---|
| Worker | Cloudflare, Workers Free plan (10 ms CPU per request; enough, see below) | `suhina-bot`, <<worker URL>> |
| KV namespace `CACHE` | Cloudflare | `suhina-bot-cache`, id in `crates/worker/wrangler.toml` |
| Durable Object `STATE` | Cloudflare | class `BotState`, SQLite-backed (migration `v1`) |
| Discord application | Developer Portal | **Suhina**, application ID `<application-id>` |
| Interactions Endpoint URL | Developer Portal › General Information | `<worker URL>/interactions` |
| GitHub App | suhina-daw › Settings › GitHub Apps | `suhina-bot` (*Suhina[bot]*), App ID `<app-id>`, installed on `suhina-daw/suhina-daw` only |
| Discord server | | ID `<discord-id>` |

The same Discord application runs the scripts in Suhina's repository that mark merged pull request
threads; they use its bot token from the GitHub secret `DISCORD_BOT_TOKEN` (Suhina's
`docs/releasing.md` › Discord).

## Plan and limits

The Free plan is enough for the bot: 100 000 requests a day, SQLite Durable Objects, KV 100 000
reads and 1 000 writes a day. The limit that matters is 10 ms of CPU per request. The App JWT is
signed with WebCrypto (native, not WebAssembly) and only when the cached installation token has
run out; waiting for GitHub or Discord costs no CPU. Check Workers & Pages › suhina-bot › Metrics ›
CPU Time; if the highest values approach 10 ms, move to Workers Paid or apply to Cloudflare's
open source programme, Project Alexandria.

## Deploying

```sh
cd crates/worker
npx wrangler deploy
```

`wrangler deploy` runs `worker-build --release` first (needs `cargo install worker-build` and the
`wasm32-unknown-unknown` target). Check it afterwards:

```sh
curl <worker URL>/      # 200 and a line of text
curl -X POST <worker URL>/interactions -d '{}'   # 401
```

and `/help` in the server answers.

## Secrets

Set from `crates/worker`. Give the value through a pipe: wrangler's prompt takes one line, a
`.pem` has many. In PowerShell `<` does not work, so:

```powershell
Get-Content path\to\key.pem -Raw | npx wrangler secret put GITHUB_APP_PRIVATE_KEY
```

| Secret | What | Where the value lives |
|---|---|---|
| `GITHUB_APP_PRIVATE_KEY` | the App's private key, the whole `.pem` | 1Password |
| `GITHUB_WEBHOOK_SECRET` | the App's webhook secret; not set until the webhook is turned on (#9) | 1Password |
| `DISCORD_BOT_TOKEN` | reactions and forum tags (#7); not set yet | 1Password |

`npx wrangler secret list` shows the names. A value is never written to a file in the repository,
a log or a command line that is kept in history.

### Rotating the App's private key

1. GitHub: suhina-daw › Settings › GitHub Apps › suhina-bot › **Credentials › Key pairs** ›
   **New key**. The `.pem` downloads.
2. Put it in the Worker (above) and in Suhina's repository: environment `suhina-bot`, secret
   `SUHINA_BOT_PRIVATE_KEY` (used by `open-issue.yml`). Store it in 1Password; delete the file.
3. Delete the old key on the same page. Tokens made with it stop working within the hour; the
   Worker's cached token lasts at most 50 minutes.

### Rotating the Discord bot token

Developer Portal › Bot › **Reset Token**. The old one stops at once, so update both places in the
same go: the GitHub secret `DISCORD_BOT_TOKEN` in Suhina's repository (`gh secret set
DISCORD_BOT_TOKEN --repo suhina-daw/suhina-daw`) and, once set, the Worker secret.

## Registering commands

After a change to the commands, from the repository root:

```powershell
$env:DISCORD_APPLICATION_ID = "<application-id>"
$env:DISCORD_BOT_TOKEN = Read-Host "Bot token" -MaskInput   # paste from 1Password; kept out of history
cargo run -p suhina-bot-register -- --guild <discord-id>
Remove-Item Env:DISCORD_BOT_TOKEN
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
  (SECURITY.md).
- **Actions**: the workflow token is read-only and cannot approve pull requests; workflows from
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
| Discord refuses to save the Interactions Endpoint URL | the Worker is not deployed, or `DISCORD_PUBLIC_KEY` in `wrangler.toml` is not the application's |
| "The application did not respond" in Discord | the Worker failed before answering: `wrangler tail` |
| `/help` says GitHub could not be reached | the private key secret is missing or wrong, or the App is not installed on the repository: `wrangler tail` shows GitHub's answer |
| "Something went wrong on Suhina's side" | a job failed; the log line names it |
| A new command does not show | not registered yet, or registered globally and still spreading |
