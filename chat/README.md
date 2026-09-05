# WHY2 Chat
[![Build Status]][pipelines] [![Codacy Badge]][gitlab] [![Latest Version]][crates.io]

[Build Status]: https://git.satan.red/ENGO150/WHY2/badges/development/pipeline.svg
[pipelines]: https://git.satan.red/ENGO150/WHY2/-/pipelines
[Latest Version]: https://img.shields.io/crates/v/why2-chat
[crates.io]: https://crates.io/crates/why2-chat
[Codacy Badge]: https://app.codacy.com/project/badge/Grade/80836146f6fa4567b734e7b5ed452f2d
[gitlab]: https://git.satan.red/ENGO150/WHY2

**Privacy-focused encrypted chat application powered by WHY2 encryption.**

WHY2 Chat is a reference implementation demonstrating the WHY2 encryption system in a real-world application. It provides encrypted text, voice, file transfer and screen sharing with no metadata collection, no backdoors, and complete transparency.

---

## Features

### Security
- **REX Encryption**: Everything on the wire is encrypted with WHY2 before transmission
- **Hybrid Key Exchange**: ephemeral ECC (NIST P-521) + ML-KEM-768 post-quantum encapsulation, combined through HKDF
- **Signed Handshake**: the ephemeral offer is signed by the server's static identity, so the pinned key authenticates every exchange
- **Forward Secrecy**: every handshake uses fresh ephemerals, and the session rekeys every 10 minutes
- **Authenticated Encryption**: HMAC-SHA256 encrypt-then-MAC on one-shot packets *and* on the streamed ones (file transfer, screen share), where the MAC covers the stream counter
- **Sequence Numbers**: prevent replay and reordering attacks
- **TOFU (Trust On First Use)**: server public key is pinned on the first connection
- **Encrypted Message History**: the optional lobby history is stored authenticated-encrypted on disk, under a key of its own

### Communication
- **Text Messaging**: real-time encrypted chat, channels, and per-channel scrollback
- **Voice Channels**: encrypted voice communication
  - Opus codec compression (48 kHz, 20 ms frames)
  - Noise reduction (nnnoiseless)
  - Voice activity detection, jitter buffering, automatic gain control
  - Per-user mute and a live voice roster of the channel
- **Screen Sharing**: H.264 screen share with audio, multiple viewers per sharer
  - Monitor selection (`/screen <index|name>`), swappable while the share runs
  - GPU colour conversion and GPU-side playback (`wgpu`), with CPU fallbacks
  - Echo cancellation keeps your own voice output out of the shared audio
- **File Transfer**: upload files to the server and download them by ID
- **Private Messages**: direct user-to-user encrypted messaging
- **Multi-Channel Support**: organize conversations into separate channels

### Interface
- **Full-screen TUI**: `ratatui` over crossterm — message pane, user sidebar, voice panel, mouse-wheel scrolling
- **Slash-command palette**: filtered command menu and a signature hint for the parameter you are typing
- **In-app settings**: `/settings` writes audio and interface options straight through to the config
- **Server settings**: the same overlay edits `server.toml` remotely for owners, including a graceful server restart
- **Roles**: `User` / `Moderator` / `Owner`, with moderation (mute, kick, ban, IP ban, pardon, broadcast) gated by rank

### Technical Highlights
- **CTR Mode Encryption**: parallel message processing
- **TCP + UDP**: reliable text (TCP), low-latency voice (UDP) — on the same port
- **Async top to bottom**: tokio on both binaries, no manually spawned OS threads
- **Spam Protection**: rate limiting, packet size limits and packet validation
- **Session Management**: automatic timeout and cleanup
- **Cross-Platform**: Linux, macOS, Windows support

---

## Building from Source

### Prerequisites

#### Linux
```bash
sudo apt-get update

# Server, or a client without voice/screen share
sudo apt-get install -y pkg-config

# Full client (voice + screen share)
sudo apt-get install -y pkg-config libasound2-dev libopus-dev libpipewire-0.3-dev \
    libegl-dev clang libclang-dev libgbm-dev nasm cmake
```

#### macOS
```bash
brew update
brew install opus pkg-config cmake nasm
```

#### Windows
No additional dependencies required (uses Windows Audio APIs and DXGI Desktop Duplication).

### Compilation

#### Client (Default)
```bash
# Build client binary
cargo build --release

# Binary location: ./target/release/why2
```

#### Server
```bash
# Build server binary (no client features)
cargo build --bin why2-server --no-default-features --features server,windows_resources --release

# Binary location: ./target/release/why2-server
```

### Features

- **`client`** (default): the whole client — `client_base` + `client_voice` + `client_screen`
- **`client_base`**: TUI and core networking (text, channels, file transfer), no audio or video
- **`client_voice`**: voice chat (cpal, Opus, noise suppression)
- **`client_screen`**: screen sharing (implies `client_voice`)
- **`server`**: server functionality (multi-client handling, no UI)
- **`windows_resources`** (in `default`): embeds the Windows icon, VERSIONINFO and manifest — for
  **binary** builds only. A crate depending on `why2-chat` as a library must build it with
  `default-features = false`, or those resources are force-linked into its own executable.

Client and server features are mutually exclusive; the build script says so rather than failing at
link time.

### Build-time environment variables

| Variable | Effect |
|----------|--------|
| `WHY2_CONFIG_DIR` | Baked-in config directory (default `{HOME}/.config/WHY2`) |
| `WHY2_SKIP_TOFU` | Disables server key pinning — local/dev testing only |
| `WHY2_DEV_BYPASS` | Skips the feature-combination check in `build.rs` |

---

## Usage

### Server Setup

1. **Run the server**:
   ```bash
   ./target/release/why2-server
   ```

2. **Configuration** (auto-generated on first run):
   - Location: `~/.config/WHY2/server.toml`
   - Important Settings:
     - `server_ip`: Bind address (default: `0.0.0.0`)
     - `server_port`: Server port (default: `1204`)
     - `server_name` / `server_username`: how the server presents itself
     - `max_clients`, `max_unauth_clients`, `max_ip_clients`: connection limits
     - `allow_register`: enable/disable new user registration
     - `enable_voice_chat`, `enable_screenshare`: toggle the two side channels
     - `max_upload_size`, `max_client_parallel_uploads`: file transfer limits
     - `persistent_messages`, `max_persistent_messages`: keep lobby messages on disk (off by default)
   - Owners can edit all of it in-app with `/server settings`; `server_ip`, `server_port`,
     `enable_voice_chat` and `server_username` are only read at startup and are marked as needing a
     restart, which the same overlay can trigger.

3. **State on disk** (all under `~/.config/WHY2/`):
   - `server_users.toml` — users and their roles
   - `server_bans.toml` — user and IP bans
   - `server_keys/` — the server identity (`private`, `public`) and `history_key`
   - `server_messages.bin` — the encrypted lobby history, if enabled

   Uploaded files are kept outside of it, in `WHY2-Uploads/` under the system temp directory.

### Client Setup

1. **Run the client**:
   ```bash
   ./target/release/why2
   ```

2. **First-time setup** (all of it inside the TUI's connect box):
   - Enter server address (optionally followed by `:PORT`)
   - Server public key verification (TOFU)
   - Create username and password

3. **Configuration** (auto-generated):
   - Location: `~/.config/WHY2/client.toml`
   - Important Settings:
     - `default_port`: default server port
     - `auto_connect` / `auto_connect_addr`: dial a server without a keystroke
     - `socks5_enabled` / `socks5_addr`: route the connection through a SOCKS5 proxy
     - `download_directory`: where downloads land
     - Display options (`show_id`, `disable_colors`, `disable_logo`, `mouse_capture`)
     - Audio (`input_device`, `output_device`, `input_volume`, `output_volume`,
       `screen_volume`, `noise_suppression`, `automatic_gain`) — all editable in-app with
       `/settings`, and applied immediately, including to a voice call that is already running. The
       two device keys hold a cpal device ID, so pick them in `/settings` rather than by hand

### Runtime environment variables (client)

| Variable | Effect |
|----------|--------|
| `WHY2_CAPTURE_BACKEND` | Pins the screen capture backend (`recorder` / `legacy`) |
| `WHY2_CAPTURE_PROBE_TIMEOUT` | Overrides the recorder probe deadline, in seconds |
| `WHY2_CAPTURE_CONVERTER` | Pins the RGBA → I420 path (`gpu` / `cpu`) |

### Important Commands

Every command has aliases, and the ones with a shortcut can be typed with `Ctrl+<key>`.

| Command | Description |
|---------|-------------|
| `/help` | Display available commands |
| `/info <command>` | Show a command's usage |
| `/list` | List connected users and their IDs |
| `/pm <id> <message>` | Send private message |
| `/channel [name]` | Switch to channel (back to the lobby if omitted) |
| `/voice` | Toggle voice chat |
| `/mute [id]` | Toggle-mute a user, or yourself |
| `/upload <path>` | Upload a file to the server |
| `/files` | Show available files and their IDs |
| `/download <user id> <file id>` | Download a file from the server |
| `/screen [monitor]` | Toggle screen sharing, or swap the shared monitor |
| `/screens` | Show everyone who is sharing |
| `/attach <id>` | Watch someone's screen share |
| `/deattach` | Stop watching |
| `/color <color>` / `/ucolor <color>` | Set message / username color |
| `/settings` | Open audio and interface settings |
| `/logout` | Disconnect and return to the login screen |
| `/exit` | Disconnect and quit |

Moderation lives under `/server` and is offered by rank:

| Command | Minimal role |
|---------|--------------|
| `/server mute <id>`, `/server kick <id>` | Moderator |
| `/server ban <id>`, `/server banip <id>`, `/server bans` | Owner |
| `/server pardon <id>`, `/server pardonip <id>` | Owner |
| `/server say <message>`, `/server role <id> <role>` | Owner |
| `/server settings` | Owner |

---

## Network Architecture

### Protocols
- **Text Communication**: TCP (port 1204 by default)
  - Key exchange (ECC + ML-KEM)
  - Encrypted messaging and server commands
  - File transfer and screen share run as authenticated streams over their own TCP connections

- **Voice Communication**: UDP (same port as TCP)
  - Encrypted Opus packets
  - Low latency streaming
  - Voice activity detection
  - Noise reduction

### Security Flow

1. **Connection**:
   - Server → Client: static identity + a fresh ephemeral ECC key and ML-KEM encapsulation key,
     signed by the identity over the handshake transcript
   - Client → Server: its own ephemeral ECC key + the ML-KEM ciphertext
   - Both derive the WHY2 grid key, nonce and HMAC key from the combined secret via HKDF

2. **Authentication**:
   - Server → Client: its rules (name, username/password limits) and version
   - Client → Server: username, then password
   - Server verifies the Argon2 hash it stores, and answers with the client's role

3. **Session**:
   - One-shot packets: WHY2 (CTR mode) with HMAC-SHA256 encrypt-then-MAC
   - Streams: the same, with the stream counter inside the tag, verified before the cipher advances
   - Sequence numbers prevent replay and reordering
   - Periodic rekeying (every 10 minutes) with fresh ephemerals

---

## Downloads

### Prebuilt Binaries
- [GitHub Actions Artifacts](https://github.com/ENGO150/WHY2/actions/workflows/build.yml)
- [Arch Linux (AUR)](https://aur.archlinux.org/packages/why2)
- [Gentoo Linux (GURU)](https://cgit.gentoo.org/repo/proj/guru.git/tree/net-im/why2)

---

## Security Notice

**WHY2 Chat is an experimental application** built on the WHY2 encryption system, which has **not undergone formal security audit**.

### Known Limitations:
- **Trust On First Use**: the first connection to a server is the one that matters — nothing
  authenticates the identity you pin there
- **Rekeying window**: a compromised session key exposes traffic until the next rekey (10 minutes)
- **Message history is opt-in and server-side**: with `persistent_messages` on, the lobby's messages
  sit encrypted next to their key in the server's config directory, which protects a leaked copy of
  the file and nothing that already has that directory
- **The server sees the plaintext**: this is transport encryption between client and server, not
  end-to-end encryption between users
- **Experimental crypto**: the WHY2 algorithm lacks peer review

### Best Practices:
1. **Verify server keys**: always validate TOFU prompts
2. **Use strong passwords**: minimum 12 characters by default
3. **Secure server**: run the server on trusted infrastructure — its config directory holds the
   identity, the user store and the history key
4. **Regular updates**: keep software up-to-date

---

## Technical Details

### Dependencies
- **Core Crypto**: `why2` (WHY2 encryption system)
- **Key Exchange**: `p521` (ECC), `ml-kem` (ML-KEM post-quantum), `hkdf`
- **Authentication**: `hmac` (HMAC-SHA256), `argon2` (password hashing)
- **Async Runtime**: `tokio`, `dashmap`
- **Voice**: `audiopus` (Opus codec), `nnnoiseless` (noise reduction), `ringbuf`
- **Audio I/O**: `cpal` (cross-platform audio)
- **Screen Share**: `xcap` / `libwayshot` (capture), `openh264` (codec), `wgpu` (colour conversion
  and playback), `winit` (viewer window)
- **Networking**: `tokio-socks` (SOCKS5 proxy support), `socket2`
- **Serialization**: `wincode` (binary encoding), `toml_edit` (config)
- **UI**: `ratatui` + `crossterm` (terminal interface)

### Performance
- **Voice Latency**: ~25ms (depends on network)
- **Screen Share**: 30 FPS target, 4 Mbps H.264, shed rather than buffered when the link is full
- **Concurrent Users**: tested up to 100 simultaneous connections
- **Message Throughput**: limited by spam protection by default

### Platform Support
| Platform | Text Chat | Voice Chat | Screen Share | Notes |
|----------|-----------|------------|--------------|-------|
| Linux | ✅ | ✅ | ✅ | ALSA, PulseAudio, PipeWire; X11 and Wayland (portal/wlr) |
| macOS | ✅ | ✅ | ✅ | CoreAudio, AVCaptureScreenInput |
| Windows | ✅ | ✅ | ✅ | WASAPI, DXGI Desktop Duplication |

---

## Contributing

See [CONTRIBUTING](https://git.satan.red/ENGO150/WHY2/-/blob/stable/CONTRIBUTING) in the repository root for contribution guidelines.

---

## Getting Help

- **Issues**: [GitLab Issues](https://git.satan.red/ENGO150/WHY2/-/issues)
- **Discord**: DM [engo150](https://discord.com/users/634385503956893737)
- **Email**: engo@satan.red

---

## License

WHY2 Chat is licensed under the **GNU GPLv3**.

You are free to use, modify, and redistribute it under the terms of the license. See <a href="https://www.gnu.org/licenses/" target="_blank">https://www.gnu.org/licenses/</a> for details.

---

## Philosophy

WHY2 Chat embodies the principle that **privacy is a fundamental right**:

- **No telemetry**: Zero data collection
- **No backdoors**: All code is auditable
- **No subscriptions**: Free as in freedom
- **No censorship**: You control your server
- **No trust required**: Verify the code yourself
