# Suhina bot

The Discord bot of [Suhina](https://github.com/suhina-daw/suhina-daw), the open-source clip-launching
DAW. It looks up issues, pull requests and test builds, lets moderators turn a Discord message into
a GitHub issue, and lets contributors claim work through Suhina's claim bot. It answers commands
only and never reads messages on its own.

A Cloudflare Worker in Rust ([workers-rs](https://github.com/cloudflare/workers-rs)) that writes to
GitHub as the App `suhina-bot` (`suhina-bot[bot]`). The plan is
[#1](https://github.com/suhina-daw/suhina-bot/issues/1).

## Documentation

| Document | Read it for |
|---|---|
| [docs/commands.md](docs/commands.md) | every command, who can use it (generated) |
| [docs/architecture.md](docs/architecture.md) | how a command flows, the crates, GitHub sign-in, what is stored, adding a command |
| [docs/operations.md](docs/operations.md) | resources, deploying, secrets and rotating them, registering commands, logs, troubleshooting |

## Developing

```sh
rustup target add wasm32-unknown-unknown
cargo install worker-build --locked
cargo test -p suhina-bot-core
cargo clippy -p suhina-bot-worker --target wasm32-unknown-unknown -- -D warnings
cargo run -p suhina-bot-register -- --markdown > docs/commands.md   # after changing a command
```

The same rules as Suhina: no `unwrap`, `expect` or `panic!` outside tests, no `unsafe`,
GPL-compatible dependencies only (`cargo deny check`), and the licence header on every source file
(`python scripts/license_header.py`). CI checks all of them and that `docs/commands.md` is current.

## Licence

GPL-3.0-or-later, like Suhina.
