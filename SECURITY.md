# Security policy

The Suhina bot answers Discord interactions and GitHub webhooks, holds the private key of the
GitHub App `suhina-bot` and a Discord bot token, and writes issues and Discord messages. Treat
every request, message, attachment and command parameter as potentially malicious.

## Reporting a vulnerability

Do not open a public issue with a working exploit or a secret. Use GitHub's private
vulnerability reporting for this repository (Security › Report a vulnerability), or open an issue
with a short, non-exploitable summary and ask for a private channel.

A useful report names the commit or deployed version, the route or command (`/interactions`,
`/github/webhook`, a slash command, a card button), exact steps, and what happened instead of
what should have.

## Scope

- Signature checks: Discord's Ed25519 (`/interactions`) and GitHub's HMAC (`/github/webhook`).
- Who may do what: staff and triage roles, the review before an issue is created.
- Handling of secrets (the App key, the bot token, `REVIEW_WORDS`) and of anything that could
  leak them into logs, messages or issues.
- Attachments and screenshots: downloads, the image checks, what reaches R2 and issues.
- Mentions: no message the bot sends may ping anyone.
- Dependencies, CI and the deployment.

## Secrets

Secrets live only in Cloudflare (`wrangler secret put`), the GitHub environment `suhina-bot` of
`suhina-daw/suhina-daw`, and the maintainer's password manager. If one is exposed, rotate it as
docs/operations.md › Secrets describes and report it as above.
