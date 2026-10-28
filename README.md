# Rain-bot v2.1

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)

Discord bot for the **Rain Server** — a Monster Hunter Frontier Z (MHFZ / Erupe)
private server. It connects the Discord community directly to the game server
database: account registration and binding, the bounty (hunt contract) system,
gacha, the server market, guild management, party recruitment, and staff
administration.

The bot is a single Rust binary backed by the same PostgreSQL database as the
Erupe game server, driven entirely through Discord slash commands, buttons,
modals, and autocomplete.

> **This is a self-hosted bot.** It is unpublished software built for one
> specific server deployment: it expects an Erupe schema, a Discord guild with
> matching channels and roles, and the game-data files in `static/`. It is
> shared as a reference implementation, not as a drop-in product.

---

## Stack

| Layer | Choice |
|---|---|
| Language | Rust, edition 2021 |
| Discord | serenity 0.12 (rustls backend) |
| Database | sqlx 0.8 → PostgreSQL (`erupe`) |
| Runtime | tokio (full), mimalloc global allocator |
| Images | image, imageproc, rusttype, gifski, webp-animation |
| AI | OpenAI API (chat + image) |
| Moderation | Sightengine (optional) |
| Auth | bcrypt |

---

## Repository layout

| Path | Contents |
|---|---|
| `src/` | Rust source (`commands/`, `event/`, `reusable/`, `material/`) |
| `hertz/` | Local proc-macro crate wrapping command handlers |
| `static/` | Game-data JSON + user-generated hunter cards |
| `image/` | Gacha rarity art and animated bounty titles |
| `icon/` | Fonts, hunter-card templates, icons |
| `query/` | SQL schema and helper scripts |
| `config.example.json` | Configuration template — copy to `config.json` |
| `.env.example` | Environment variable template — copy to `.env` |

---

## Prerequisites

- Rust toolchain (edition 2021; `rustup` recommended)
- A PostgreSQL database reachable from the host running the bot
- A Discord bot application and token

On Linux, building `release` needs a C toolchain (for `mimalloc` / `gifski`):

```bash
sudo apt install -y build-essential pkg-config
```

---

## Configuration

### 1. Configuration file — `config.json`

```bash
cp config.example.json config.json
```

Then replace the placeholder values with your own IDs. `config.example.json`
uses `0` for every Discord ID; a guild, channel, or role left at `0` simply will
not resolve, so fill in the ones for the features you enable.

What lives in `config.json`:

| Section | Contents |
|---|---|
| `discord` | Prefix, owner user ID, moderation toggle |
| `postgress` | Database host, port, user, database name |
| `authorized_guilds` | Guilds the bot is allowed to operate in |
| `bounty`, `server_role`, `lfg_roles`, `log_channel` | Channel and role IDs |
| `bot_config`, `commands_toggle` | Feature and per-command switches |

`config.json` is git-ignored because it holds server-specific infrastructure
details. Only `config.example.json` — with all IDs zeroed — is tracked.

> **Lockout guard:** if `authorized_guilds` is empty, the bot auto-leaves every
> guild it joins. Always keep at least one valid guild ID.

### 2. Secrets — `.env`

```bash
cp .env.example .env
```

Fill in `DISCORD_TOKEN`, `POSTGRES_PASSWORD`, and — only when the matching
feature is enabled — `CHATGPT_TOKEN` and the `SIGHTENGINE_*` pair.

Secrets are never stored in `config.json`: it keeps `LOADED_FROM_ENV`
placeholders, and the real values are injected from the environment at startup
by `src/reusable/config.rs::apply_env_overrides()`.

---

## Build

```bash
cargo build --release
```

The release profile uses `lto = "fat"` and `codegen-units = 1`, so a clean build
takes several minutes; incremental builds are much faster.

> **Note on `.cargo/config.toml`:** this project sets
> `rustflags = ["-C", "target-cpu=native"]`. The resulting binary is tuned for
> the CPU it was built on. Build on the machine that will run it, or remove that
> flag when producing a binary meant to move between hosts.

For fast feedback during development:

```bash
cargo check
```

---

## Running

The bot must be started from the project root — all paths (`static/`, `image/`,
`icon/`, `save/`, `logs/`, `CACHE`) are relative.

```bash
./rain-bot
```

`loop.sh` wraps that in an infinite restart loop for unattended operation:

```bash
./loop.sh
```

**Never run two instances at the same time** — both would use the same Discord
token and kick each other offline, and both would write to the same `CACHE`
file.

Shutdown with `Ctrl+C` (SIGINT). The bot flushes its in-memory state to `CACHE`
before exiting.

---

## Features

### Player-facing

| Area | Commands |
|---|---|
| Account | `/create`, `/bind`, `/check`, `/switch`, `/change_password`, `/add_psn` |
| Profile | `/card`, `/event`, `/huntercard`, `/dm_save`, `/transfer` |
| Bounty | `/bounty submit`, `/bounty pedia`, `/cooldown`, `/distribution` |
| Gacha | `/pull single`, `/pull multi` |
| Market | `/trading market\|bar\|restourant\|casino\|jewelry\|guild` |
| Guild | `/guild info\|join\|leave\|clear_app\|extend_food` |
| Social | `/party`, `/ferias`, `/active_feature`, `/gpt`, `?tag` |

### Staff

`/interface`, `/purge`, `/clearcache`, `/gacha_check`, `/mod_pass`, `/send`,
`/config`, `/monitor`, `/title`, `/add`, `/remind`, the `*_autoresponse`
family, `/title_add|edit|delete`, plus `?execute` / `?query` for raw SQL.

Staff access is gated by `execute_guard`
(`src/commands/admin/mod.rs`).

---

## Data and runtime files

| Path | Purpose | In git? |
|---|---|---|
| `CACHE` | In-memory state snapshot (GPT sessions, pending bounties, parties) | No |
| `save/` | Player save-file uploads | No |
| `logs/` | Auto-response log | No |
| `static/huntercards/` | User-generated hunter card images and notes | No |
| `static/*.json` | Game data: market, gacha, bounty, autoresponse | Yes |
| `image/`, `icon/` | Rendering assets (required at runtime) | Yes |

`static/huntercards/`, `save/`, and `logs/` are kept in the repository only as
empty directories via `.gitkeep`; their contents are user data and stay local.

---

## Maintenance

This repository holds the application source and the assets required to run it.
Operational and analytical documentation — architecture notes, backlog,
changelog, audit findings, and release reports — is maintained separately by the
project maintainers and is intentionally not distributed with the source.

For day-to-day work, `cargo check` and `cargo build --release` are the two
commands that matter; the sections above cover the rest.

---

## Contributing

Issues and pull requests are welcome. Before opening a PR:

1. Run `cargo check` and `cargo clippy` — both should be clean.
2. Keep commits focused; describe *why* in the message, not just *what*.
3. Never commit `.env`, `config.json`, credentials, or user data.

Because this bot targets one specific server deployment, changes that add a new
external dependency, a new required configuration key, or a database migration
need a short explanation of how an existing deployment is expected to upgrade.

---

## Security

Please do **not** open a public issue for a security problem. Report it privately
through GitHub's [private vulnerability reporting](../../security/advisories/new)
so a fix can be prepared before disclosure.

---

## License

[MIT](LICENSE) © 2026 S&S (Rain Server)

---

## Acknowledgements

Built for the Rain Server community. Monster Hunter Frontier Z is a trademark of
Capcom; this project is unaffiliated fan tooling and ships no game assets.
