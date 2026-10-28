# Rain-bot v2.1

Discord bot for the **Rain Server** — a Monster Hunter Frontier Z (MHFZ / Erupe)
private server. It connects the Discord community directly to the game server
database: account registration and binding, the bounty (hunt contract) system,
gacha, the server market, guild management, party recruitment, and staff
administration.

The bot is a single Rust binary backed by the same PostgreSQL database as the
Erupe game server, driven entirely through Discord slash commands, buttons,
modals, and autocomplete.

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
| `config.json` | Public configuration (IDs, toggles) — **no secrets** |
| `.env` | Secrets only (git-ignored; see `.env.example`) |

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

### 1. Secrets — `.env`

```bash
cp .env.example .env
```

Fill in `DISCORD_TOKEN`, `POSTGRES_PASSWORD`, and — only when the matching
feature is enabled — `CHATGPT_TOKEN` and the `SIGHTENGINE_*` pair.

`config.json` deliberately stores placeholders (`LOADED_FROM_ENV`); the real
values are injected from the environment at startup by
`src/reusable/config.rs::apply_env_overrides()`.

### 2. Public settings — `config.json`

Guild IDs, channel IDs, role IDs, feature toggles (`bot_config`), per-command
toggles (`commands_toggle`), database host/port/database name, and bounty
configuration live here. This file must never contain a token or password.

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
