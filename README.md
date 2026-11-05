# WHY2

A Rust workspace containing a terminal chat client and server, built on the REX cipher.

[![build](https://img.shields.io/gitlab/pipeline-status/ENGO150/WHY2?branch=stable&style=flat-square&gitlab_url=https%3A%2F%2Fgit.satan.red)](https://git.satan.red/ENGO150/WHY2/-/pipelines)
[![why2-chat](https://img.shields.io/crates/v/why2-chat?label=why2-chat&style=flat-square)](https://crates.io/crates/why2-chat)
[![why2](https://img.shields.io/crates/v/why2?label=why2&style=flat-square)](https://crates.io/crates/why2)
[![license](https://img.shields.io/badge/license-GPLv3-blue?style=flat-square)](https://git.satan.red/ENGO150/WHY2/-/blob/stable/LICENSE)

[Features](#features) · [Quick start](#quick-start) · [Security](#security) · [Links](#links)

| Crate | Description |
|:---|:---|
| [`why2-chat`](https://git.satan.red/ENGO150/WHY2/-/blob/stable/chat/README.md) | Terminal client and headless server |
| [`why2`](https://git.satan.red/ENGO150/WHY2/-/blob/stable/core/README.md) | REX encryption cipher, usable on its own |

## Features

- **Text:** channels, private messages, replies, reactions, pins and edits.
- **Voice:** Opus channels with noise suppression, voice activity detection and voice messages.
- **Screen sharing:** H.264 with audio, multiple viewers and monitor switching.
- **Files:** uploads, downloads, inline images, avatars and a server icon.
- **Server:** roles, moderation, bans and optional encrypted history.
- **Client:** full-screen TUI, themes, translations and in-app settings.
- **Network:** SOCKS5 proxy support for the TCP session.

## Quick start

From the repository root:

**Server**

```bash
cargo build --bin why2-server --no-default-features --features server,windows_resources --release
./target/release/why2-server
```

**Client**

```bash
cargo build --bin why2 --release
./target/release/why2
```

On Linux, a full client also needs the audio and video packages listed in the
[chat README](https://git.satan.red/ENGO150/WHY2/-/blob/stable/chat/README.md#build).

**Docker**

```bash
docker compose -f chat/docker-compose.yml up -d
```

The first account to register becomes the owner. Open TCP and UDP port `1204`.

## Security

- Messages, voice, files and screen streams are encrypted and authenticated.
- The handshake is hybrid P-521 + ML-KEM-768 with TOFU pinning; sessions rekey every 10 minutes.
- The server decrypts messages to route and store them. This is not end-to-end encryption.
- REX is experimental and has not been formally audited.

## Links

- [Chat README](https://git.satan.red/ENGO150/WHY2/-/blob/stable/chat/README.md)
- [Downloads](https://why2.satan.red/download)
- [Cipher documentation](https://docs.rs/why2)
- [Security policy](https://git.satan.red/ENGO150/WHY2/-/blob/stable/SECURITY)
- [Contributing](https://git.satan.red/ENGO150/WHY2/-/blob/stable/CONTRIBUTING)

## License

GPLv3. See [LICENSE](https://git.satan.red/ENGO150/WHY2/-/blob/stable/LICENSE).
