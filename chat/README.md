# WHY2 Chat
[![Build Status]][pipelines] [![Codacy Badge]][gitlab] [![Latest Version]][crates.io]

[Build Status]: https://git.satan.red/ENGO150/WHY2/badges/development/pipeline.svg
[pipelines]: https://git.satan.red/ENGO150/WHY2/-/pipelines
[Latest Version]: https://img.shields.io/crates/v/why2-chat
[crates.io]: https://crates.io/crates/why2-chat
[Codacy Badge]: https://app.codacy.com/project/badge/Grade/80836146f6fa4567b734e7b5ed452f2d
[gitlab]: https://git.satan.red/ENGO150/WHY2

**Privacy-focused encrypted chat application powered by WHY2 encryption.**

WHY2 Chat is a reference implementation demonstrating the WHY2 encryption system in a real-world application. It provides encrypted text and voice communication with no metadata collection, no backdoors, and complete transparency.

---

## Features

### Security
- **REX Encryption**: All messages encrypted with WHY2 before transmission
- **Hybrid Key Exchange**: ECC + ML-KEM post-quantum key encapsulation
- **Forward Secrecy**: Automatic rekeying invalidates old session keys
- **Authenticated Encryption**: HMAC-SHA256 ensures message integrity
- **Sequence Numbers**: Prevents replay and reordering attacks
- **TOFU (Trust On First Use)**: Server public key verification

### Communication
- **Text Messaging**: Real-time encrypted chat with message history
- **Voice Channels**: Encrypted voice communication
  - Opus codec compression
  - Noise reduction (nnnoiseless)
  - Voice activity detection
  - Audio normalization
- **Private Messages**: Direct user-to-user encrypted messaging
- **Multi-Channel Support**: Organize conversations into separate channels

### Technical Highlights
- **CTR Mode Encryption**: Parallel message processing
- **TCP + UDP**: Reliable text (TCP), low-latency voice (UDP)
- **Spam Protection**: Rate limiting and packet validation
- **Session Management**: Automatic timeout and cleanup
- **Cross-Platform**: Linux, macOS, Windows support

---

## Building from Source

### Prerequisites

#### Linux
```bash
sudo apt-get update
sudo apt-get install -y pkg-config libasound2-dev libopus-dev
```

#### macOS
```bash
brew update
brew install opus pkg-config
```

#### Windows
No additional dependencies required (uses Windows Audio APIs).

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
cargo build --bin why2-server --no-default-features --features server --release

# Binary location: ./target/release/why2-server
```

### Features

- **`client`** (default): Enables client functionality (TUI, audio I/O)
- **`server`**: Enables server functionality (multi-client handling, no UI)

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
     - `port`: Server port (default: `1204`)
     - `max_clients`: Maximum concurrent connections
     - `allow_register`: Enable/disable new user registration
     - `enable_voice_chat`: Enable/disable voice channels

3. **User Management**:
   - Users stored in: `~/.config/WHY2/server_users.toml`
   - First connection creates account automatically (if registration enabled)

### Client Setup

1. **Run the client**:
   ```bash
   ./target/release/why2
   ```

2. **First-time setup**:
   - Enter server address (optionally followed by ':PORT')
   - Create username and password
   - Server public key verification (TOFU)

3. **Configuration** (auto-generated):
   - Location: `~/.config/WHY2/client.toml`
   - Important Settings:
     - `auto_connect_addr`: Default server address
     - `default_port`: Default server port
     - Display options (`show_id`, `disable_colors`)
     - Voice chat audio (`input_device`, `output_device`, `input_volume`, `output_volume`,
       `noise_suppression`, `automatic_gain`) - all editable in-app with `/settings`, and applied
       immediately, including to a voice call that is already running

### Important Commands

| Command | Description |
|---------|-------------|
| `/help` | Display available commands |
| `/list` | List all connected users |
| `/pm <user> <message>` | Send private message |
| `/channel <name>` | Switch to channel |
| `/voice` | Join voice channel |
| `/settings` | Open audio and interface settings |
| `/exit` | Disconnect and quit |

---

## Network Architecture

### Protocols
- **Text Communication**: TCP (port 1204 by default)
  - Key exchange (ECC + ML-KEM)
  - Encrypted messaging
  - Server commands

- **Voice Communication**: UDP (same port as TCP)
  - Encrypted Opus packets
  - Low latency streaming
  - Voice activity detection
  - Noise reduction

### Security Flow

1. **Connection**:
   - Client ← Server: Public key (ECC + ML-KEM)
   - Server ← Client: Public key (ECC + ML-KEM)
   - Both derive shared secret via hybrid KDF

2. **Authentication**:
   - Client ← Server: Challenge
   - Server ← Client: Username + Argon2 hashed password
   - Server validates credentials

3. **Session**:
   - All messages encrypted with WHY2 (CTR mode)
   - HMAC-SHA256 authentication
   - Sequence numbers prevent replay
   - Periodic rekeying (every 10 minutes)

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
- **No perfect forward secrecy** between rekeying intervals
- **Trust On First Use**: First connection to server is critical
- **No message persistence**: Messages not saved server-side
- **Experimental crypto**: WHY2 algorithm lacks peer review

### Best Practices:
1. **Verify server keys**: Always validate TOFU prompts
2. **Use strong passwords**: Minimum 12 characters required
3. **Secure server**: Run server on trusted infrastructure
4. **Regular updates**: Keep software up-to-date

---

## Technical Details

### Dependencies
- **Core Crypto**: `why2` (WHY2 encryption system)
- **Key Exchange**: `r521` (ECC), `mk-kem` (ML-KEM post-quantum)
- **Authentication**: `hmac` (HMAC-SHA256), `argon2` (password hashing)
- **Voice**: `opus` (audio codec), `nnnoiseless` (noise reduction)
- **Audio I/O**: `cpal` (cross-platform audio)
- **Networking**: `socks5` (SOCKS5 proxy support)
- **Serialization**: `wincode` (binary encoding)
- **UI**: `crossterm` (terminal interface)

### Performance
- **Encryption**: ~200 MB/s (8×8 grids, single-threaded)
- **Voice Latency**: ~25ms (depends on network)
- **Concurrent Users**: Tested up to 100 simultaneous connections
- **Message Throughput**: Defaultly limited by spam protection

### Platform Support
| Platform | Text Chat | Voice Chat | Notes |
|----------|-----------|------------|-------|
| Linux | ✅ | ✅ | ALSA, PulseAudio, PipeWire |
| macOS | ✅ | ✅ | CoreAudio |
| Windows | ✅ | ✅ | WASAPI |

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
