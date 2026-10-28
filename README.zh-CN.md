# Rain-bot v2.1

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)

**简体中文** | [English](README.md)

面向 **Rain Server**（怪物猎人边境 Z，MHFZ / Erupe 私人服务器）的 Discord 机器人。
它把 Discord 社区与游戏服务器数据库直接连接起来：账号注册与绑定、赏金（狩猎委托）
系统、扭蛋、服务器市场、公会管理、组队招募，以及管理员的日常运维。

该机器人是一个单一的 Rust 二进制程序，与 Erupe 游戏服务器共用同一个 PostgreSQL
数据库，全部交互通过 Discord 斜杠命令、按钮、模态框和自动补全完成。

> **这是一个自托管机器人。** 它是为某一特定服务器部署而构建的未发布软件：它要求
> Erupe 数据库结构、一个拥有对应频道与角色的 Discord 服务器，以及 `static/`
> 目录下的游戏数据文件。本项目作为参考实现公开，并非开箱即用的成品。

---

## 技术栈

| 层次 | 选型 |
|---|---|
| 语言 | Rust，2021 edition |
| Discord | serenity 0.12（rustls 后端） |
| 数据库 | sqlx 0.8 → PostgreSQL（`erupe`） |
| 运行时 | tokio（full），mimalloc 全局分配器 |
| 图像 | image、imageproc、rusttype、gifski、webp-animation |
| AI | OpenAI API（对话 + 绘图） |
| 内容审核 | Sightengine（可选） |
| 认证 | bcrypt |

---

## 仓库结构

| 路径 | 内容 |
|---|---|
| `src/` | Rust 源码（`commands/`、`event/`、`reusable/`、`material/`） |
| `hertz/` | 本地过程宏 crate，包裹命令处理函数 |
| `static/` | 游戏数据 JSON + 用户生成的猎人名片 |
| `image/` | 扭蛋稀有度插画与动态赏金称号 |
| `icon/` | 字体、猎人名片模板、图标 |
| `query/` | SQL 结构与辅助脚本 |
| `config.example.json` | 配置模板 —— 复制为 `config.json` |
| `.env.example` | 环境变量模板 —— 复制为 `.env` |

---

## 前置要求

- Rust 工具链（2021 edition；建议使用 `rustup`）
- 一个可从运行机器人生器的主机访问的 PostgreSQL 数据库
- 一个 Discord 机器人应用及其令牌

在 Linux 上编译 `release` 需要 C 工具链（供 `mimalloc` / `gifski` 使用）：

```bash
sudo apt install -y build-essential pkg-config
```

---

## 配置

### 1. 配置文件 —— `config.json`

```bash
cp config.example.json config.json
```

然后把占位值替换为你自己的 ID。`config.example.json` 中每个 Discord ID 都是 `0`；
留为 `0` 的服务器、频道或角色将无法解析，因此请为你启用的功能填入对应 ID。

`config.json` 包含以下内容：

| 区块 | 内容 |
|---|---|
| `discord` | 前缀、所有者用户 ID、内容审核开关 |
| `postgress` | 数据库主机、端口、用户、数据库名 |
| `authorized_guilds` | 允许机器人运作的服务器列表 |
| `bounty`、`server_role`、`lfg_roles`、`log_channel` | 频道与角色 ID |
| `bot_config`、`commands_toggle` | 功能开关与逐命令开关 |

`config.json` 已被 git 忽略，因为它包含服务器专属的基础设施信息。仓库中仅跟踪
`config.example.json`（所有 ID 均已归零）。

> **锁定保护：** 若 `authorized_guilds` 为空，机器人会自动退出它所加入的每一个
> 服务器。请始终保留至少一个有效的服务器 ID。

### 2. 密钥 —— `.env`

```bash
cp .env.example .env
```

填入 `DISCORD_TOKEN`、`POSTGRES_PASSWORD`，并且**仅当**对应功能启用时才填入
`CHATGPT_TOKEN` 与 `SIGHTENGINE_*` 两项。

密钥从不存放在 `config.json` 中：该文件保留 `LOADED_FROM_ENV` 占位符，真实值在
启动时由 `src/reusable/config.rs::apply_env_overrides()` 从环境变量注入。

---

## 编译

```bash
cargo build --release
```

release 配置使用了 `lto = "fat"` 与 `codegen-units = 1`，因此完整编译需要数分钟；
增量编译会快很多。

> **关于 `.cargo/config.toml`：** 本项目设置了
> `rustflags = ["-C", "target-cpu=native"]`。生成的二进制会针对编译时的 CPU 优化。
> 请在最终运行它的机器上编译，或在需要跨主机迁移二进制时移除该参数。

开发时如需快速检查：

```bash
cargo check
```

---

## 运行

机器人必须从项目根目录启动 —— 所有路径（`static/`、`image/`、`icon/`、`save/`、
`logs/`、`CACHE`）均为相对路径。

```bash
./rain-bot
```

`loop.sh` 将其包装为无限重启循环，用于无人值守运行：

```bash
./loop.sh
```

**切勿同时运行两个实例** —— 两者会使用同一个 Discord 令牌并互相挤下线，且都会写入
同一个 `CACHE` 文件。

使用 `Ctrl+C`（SIGINT）关闭。机器人会在退出前把内存状态写入 `CACHE`。

---

## 功能

### 面向玩家

| 类别 | 命令 |
|---|---|
| 账号 | `/create`、`/bind`、`/check`、`/switch`、`/change_password`、`/add_psn` |
| 个人资料 | `/card`、`/event`、`/huntercard`、`/dm_save`、`/transfer` |
| 赏金 | `/bounty submit`、`/bounty pedia`、`/cooldown`、`/distribution` |
| 扭蛋 | `/pull single`、`/pull multi` |
| 市场 | `/trading market\|bar\|restourant\|casino\|jewelry\|guild` |
| 公会 | `/guild info\|join\|leave\|clear_app\|extend_food` |
| 社交 | `/party`、`/ferias`、`/active_feature`、`/gpt`、`?tag` |

### 面向管理员

`/interface`、`/purge`、`/clearcache`、`/gacha_check`、`/mod_pass`、`/send`、
`/config`、`/monitor`、`/title`、`/add`、`/remind`、`*_autoresponse` 系列、
`/title_add|edit|delete`，以及用于执行原始 SQL 的 `?execute` / `?query`。

管理员权限由 `execute_guard` 把关（`src/commands/admin/mod.rs`）。

---

## 数据与运行时文件

| 路径 | 用途 | 是否纳入 git |
|---|---|---|
| `CACHE` | 内存状态快照（GPT 会话、待审赏金、队伍） | 否 |
| `save/` | 玩家存档上传 | 否 |
| `logs/` | 自动回复日志 | 否 |
| `static/huntercards/` | 用户生成的猎人名片图片与备注 | 否 |
| `static/*.json` | 游戏数据：市场、扭蛋、赏金、自动回复 | 是 |
| `image/`、`icon/` | 渲染素材（运行时必需） | 是 |

`static/huntercards/`、`save/` 与 `logs/` 在仓库中仅以 `.gitkeep` 保留空目录；
其内容属于用户数据，只保留在本地。

---

## 维护

本仓库只包含应用程序源码与运行所需的素材。运维与分析类文档 —— 架构说明、待办清单、
更新日志、审计发现与发布报告 —— 由项目维护者单独保管，有意不随源码分发。

日常开发中只需关心 `cargo check` 与 `cargo build --release` 两条命令；其余内容已在
上文各节覆盖。

---

## 贡献

欢迎提交 Issue 与 Pull Request。发起 PR 之前请：

1. 运行 `cargo check` 与 `cargo clippy` —— 两者都应无警告。
2. 保持提交聚焦；在提交信息中说明**为什么**，而不只是**做了什么**。
3. 切勿提交 `.env`、`config.json`、任何凭据或用户数据。

由于本机器人面向某一特定服务器部署，新增外部依赖、新增必填配置项或数据库迁移的改动，
需要简要说明现有部署应如何升级。

---

## 安全

发现安全问题时，请**不要**提交公开 Issue。请通过 GitHub 的
[私密漏洞报告](../../security/advisories/new) 渠道私下告知，以便在公开披露前准备好修复。

---

## 许可证

[MIT](LICENSE) © 2026 S&S（Rain Server）

---

## 致谢

为 Rain Server 社区构建。Monster Hunter Frontier Z 是 Capcom 的商标；本项目为
非官方粉丝工具，不附带任何游戏素材。
