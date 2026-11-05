# WHY2 Chat

A terminal client and server for encrypted text, voice, screen sharing and file transfer.

[![build](https://img.shields.io/gitlab/pipeline-status/ENGO150/WHY2?branch=stable&style=flat-square&gitlab_url=https%3A%2F%2Fgit.satan.red)](https://git.satan.red/ENGO150/WHY2/-/pipelines)
[![why2-chat](https://img.shields.io/crates/v/why2-chat?label=why2-chat&style=flat-square)](https://crates.io/crates/why2-chat)
[![license](https://img.shields.io/badge/license-GPLv3-blue?style=flat-square)](https://git.satan.red/ENGO150/WHY2/-/blob/stable/LICENSE)

[Features](#features) · [Build](#build) · [Run](#run) · [Commands](#commands) · [Security](#security) · [Links](#links)

## Features

- **Text:** channels, private messages, replies, reactions, pins, edits and deletes.
- **Voice:** Opus channels with noise suppression, voice activity detection, per-user mute and voice messages.
- **Screen sharing:** H.264 with audio, monitor switching and multiple viewers.
- **Files:** uploads, downloads, inline images, avatars and a server icon.
- **Profiles:** bio, pronouns, website, status and avatar.
- **Server:** roles, moderation, bans, rate limiting and optional encrypted history.
- **Client:** full-screen TUI, command palette, themes, translations and in-app settings.
- **Network:** SOCKS5 proxy support for the TCP session.

## Build

The server needs only Rust. A full client on Linux needs:

```bash
sudo apt install pkg-config libasound2-dev libopus-dev libpipewire-0.3-dev \
    libegl-dev clang libclang-dev libgbm-dev nasm cmake
```

macOS: `brew install opus pkg-config cmake nasm`. Windows: no extra packages.

From the workspace root:

```bash
# client
cargo build --bin why2 --release

# server
cargo build --bin why2-server --no-default-features --features server,windows_resources --release
```

Text-only client:

```bash
cargo build --bin why2 --no-default-features --features client_base,windows_resources --release
```

Features are `client` (default), `client_base`, `client_voice`, `client_screen` and `server`.
`client_voice` and `client_screen` build on `client_base`, and client and server features cannot be
enabled together. `windows_resources` embeds the Windows icon and manifest and is for binaries
only.

Or run the published server image:

```bash
docker compose -f chat/docker-compose.yml up -d
```

## Run

```bash
./target/release/why2-server
./target/release/why2
```

- The server listens on `0.0.0.0:1204` for TCP and UDP. The first account to register becomes the owner.
- The client asks for the address, username and password, and asks before trusting an unknown server key on first use.
- Config lives in `~/.config/WHY2` (`WHY2_CONFIG_DIR` at build time). `/settings` and `/server settings` edit it in-app.

<details>
<summary>Data files and environment variables</summary>

| Path | Contents |
|:---|:---|
| `client.toml`, `server.toml` | Settings |
| `server_users.toml`, `server_bans.toml` | Accounts and bans |
| `server_keys/` | Server identity |
| `server_history_key`, `server_image_key` | At-rest keys |
| `server_messages.bin`, `server_images/` | Stored history and pictures |
| `server_keys.toml` | Pinned servers (client) |
| `WHY2-Uploads/` under temp | Uploaded files |

| Variable | When | Effect |
|:---|:---|:---|
| `WHY2_CONFIG_DIR` | build | Config directory, `{HOME}/.config/WHY2` by default |
| `WHY2_SKIP_TOFU` | build | Disables server identity pinning; testing only |
| `WHY2_CAPTURE_BACKEND` | run | Pins screen capture, `recorder` or `legacy` |
| `WHY2_CAPTURE_PROBE_TIMEOUT` | run | Recorder probe deadline, in seconds |
| `WHY2_SHARE_AUDIO` | run | Pins share audio, `pipewire` or `monitor` |

`server_ip`, `server_port`, `enable_voice_chat`, `server_username` and `log_level` are read at
startup only.

</details>

## Commands

Aliases are available for every command. `Ctrl+<key>` runs the ones with a shortcut.

| Command | Description |
|:---|:---|
| `/help`, `/info <command>` | Command list and usage |
| `/list` | Connected users and IDs |
| `/pm <id> <text>`, `/re <text>` | Private messages |
| `/channel [name]` | Switch channel; no name returns to the lobby |
| `/voice`, `/mute [id]` | Voice chat; mute a user or yourself |
| `/record`, `/play <id>` | Record and play voice messages |
| `/image <path>` | Post a picture |
| `/upload <path>`, `/files`, `/download <user> <file>` | File transfer |
| `/screen [monitor] [sound]`, `/screens`, `/attach <id>`, `/deattach`, `/mutescreen` | Screen sharing |
| `/reply <id> <text>`, `/heart <id>`, `/pin <id>` (Moderator), `/edit <id> <text>`, `/delete <id>` | Message actions |
| `/profile [user]`, `/color <color>`, `/ucolor <color>` | Profiles and colors |
| `/settings`, `/account passwd`, `/account delete`, `/logout`, `/exit` | Settings, account and session |

<details>
<summary>Moderation commands</summary>

| Command | Lowest role |
|:---|:---|
| `/server mute <id>`, `/server kick <id>` | Moderator |
| `/server ban <user>`, `/server banip <id>`, `/server bans` | Owner |
| `/server pardon <id>`, `/server pardonip <id>` | Owner |
| `/server say <text>`, `/server role <user> <role>` | Owner |
| `/server settings`, `/server icon [path]` | Owner |

</details>

## Security

- Hybrid P-521 + ML-KEM-768 handshake with TOFU pinning; sessions rekey every 10 minutes.
- Packets are authenticated and sequence numbers prevent replay.
- The server decrypts messages to route, store and moderate them. This is not end-to-end encryption.
- Channels are not confidentiality boundaries, and history is opt-in and stored on the server.
- A compromised session key exposes traffic until the next rekey.
- REX is experimental and has not been formally audited.

## Links

- [Downloads](https://why2.satan.red/download)
- [Security policy](https://git.satan.red/ENGO150/WHY2/-/blob/stable/SECURITY)
- [Contributing](https://git.satan.red/ENGO150/WHY2/-/blob/stable/CONTRIBUTING)
- [Cipher documentation](https://docs.rs/why2)

## License

GPLv3. See [LICENSE](https://git.satan.red/ENGO150/WHY2/-/blob/stable/LICENSE).
