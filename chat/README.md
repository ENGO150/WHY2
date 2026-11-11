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
| `server_messages.bin`, `server_images/` | Stored lobby history and pictures |
| `server_private_messages.bin` | Stored private message history |
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
| `/record`, `/play [id]` | Record voice messages; play voice, audio, and video attachments; omit the ID to stop |
| `/upload <path> [persistent]` | Upload any file; `true` keeps it in chat history and previews images |
| `/files`, `/download <user> <file>`, `/download <message-id>` | File transfer |
| `/screen [monitor] [sound]`, `/screens`, `/attach <id>`, `/deattach`, `/mutescreen` | Screen sharing |
| `/reply <id> <text>`, `/heart <id>`, `/pin <id>` (Moderator), `/edit <id> <text>`, `/delete <id>` | Message actions |
| `/profile [user]`, `/color <color>`, `/ucolor <color>` | Profiles and colors |
| `/settings`, `/account passwd`, `/account delete`, `/logout`, `/exit` | Settings, account and session |

`persistent` defaults to `true`. Use `/upload report.pdf` to attach a file to chat history, then `/download <message-id>` to retrieve it, including after a server restart. Downloads use a verified local cache when available, otherwise they request the file from the server. Use `/upload report.pdf false` for a temporary file share. Images are rendered inline as before. Persistent attachments use the existing 8 MB limit and are kept in the lobby when `persistent_messages` is enabled.

With `persistent_private_messages` enabled, private messages are kept encrypted on the server and replayed to both accounts on login, and `/pm` can address an offline account id so the message is delivered on their next login. Offline messages require `max_persistent_private_messages` to be greater than zero and remain subject to that history limit. Private history travels in separate packets and is merged into the lobby timeline by send order, including older pages. Existing private history is migrated using its saved timestamps where available.

PM message IDs have a `p` prefix, such as `#p12`, and are local to each account. The same PM can have different IDs for its two participants; unrelated conversations do not advance your counter. Stored PMs support `/heart p12`, `/hearts p12`, `/edit p12 new text`, `/delete p12`, and `/reply p12 answer`. Only the author may edit or delete a PM; either participant may react. Replies and action updates stay between the two accounts, and changes are kept for offline participants. Existing PMs keep their contents and actions when migrated, but receive new per-account IDs.

With `client_voice`, click an audio/video attachment caption or use `/play <message-id>` to play it inline. The embedded player supports MP3, WAV/PCM, FLAC, AAC/M4A, ALAC, Ogg/Vorbis, and mono/stereo Opus. Video supports H.264, HEVC/H.265, VP9, and Motion JPEG in MP4/M4V, MOV, MKV, or WebM containers as appropriate for each codec (up to 4096 × 2160, 60,000 samples). MOV is a container: ProRes and other unsupported codecs still require downloading the file. VP8 and AV1 are not supported. Videos show a still thumbnail before playback, following the image-preview setting. Video uses terminal image rendering; audio shares the voice-message output device and volume. It needs neither an external player nor `client_screen`. Decode support is the same on Linux, Windows, and macOS; terminal graphics support determines how video is displayed. Encoded attachments are shared in memory for seeking, while decoded audio and video use bounded queues. Playback has play/stop controls; pause and seeking are not implemented.

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
