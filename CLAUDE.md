# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

WHY2 is a Rust workspace with two members:

- **`core`** (crate `why2`, published to crates.io) — the REX encryption algorithm itself: a
  configurable grid-based SPN block cipher (ARX nonlinear mixing + true MDS diffusion) run in CTR
  mode, with optional HMAC-SHA256 authentication. This is a standalone cryptography library with
  no networking code.
- **`chat`** (crate `why2-chat`, binaries `why2` and `why2-server`) — a reference chat application
  (text + voice + screen share) built on top of `why2` to demonstrate it in a real protocol. This
  is where almost all active development happens (see recent commits around async networking).

Both crates share the GPLv3 license header block at the top of every source file — preserve it
when creating new files (copy from a sibling file in the same directory).

## Build commands

```bash
# Build core library only
cargo build -p why2 --release

# Build chat client (default features; needs system deps, see below)
cargo build --release
cargo build --bin why2 --release        # explicit client binary

# Build chat server (mutually exclusive with client features)
cargo build --bin why2-server --no-default-features --features server --release
```

The client TUI is built on `ratatui` (feature `crossterm_0_29`, so it reuses the crossterm 0.29
already in the tree instead of pulling a second backend). The server build has no UI dependencies.

`chat`'s `build.rs` enforces that `client_base`/`client_voice`/`client_screen`/`client` and
`server` are never enabled together, and that the internal `chat` feature is never enabled
directly (only via `client*` or `server`) — it will `panic!` with an explanatory message if you
get this wrong. Set `WHY2_DEV_BYPASS=1` to skip that check when experimenting with unusual feature
combinations (never use it for real builds).

Building the full client (default features, includes voice + screen share) requires system
packages: on Debian/Ubuntu, `pkg-config libasound2-dev libopus-dev libpipewire-0.3-dev
libegl-dev clang libclang-dev libgbm-dev nasm cmake`. The server build has no such requirement.

## Test commands

Tests for `core` live in the top-level `tests/` directory (registered as the `why2_integration`
test binary in `core/Cargo.toml`, not under `core/`) and are run with `-p why2`:

```bash
cargo test -p why2 --release                             # all core tests
cargo test -p why2 encrypt_decrypt --release -- --nocapture
cargo test -p why2 verify_multi_grid_overflow --release -- --nocapture
cargo test -p why2 diffusion_test --release -- --nocapture      # runs test_input_diffusion + test_key_diffusion
cargo test -p why2 test_auth_tamper_resistance --release -- --nocapture   # requires `auth` feature (default)
cargo test -p why2 test_ciphertext_entropy --release -- --nocapture
cargo test -p why2 stream_test --release -- --nocapture
```

CI (`.gitlab-ci.yml`) always runs these with `--release`; prefer that locally too since the cipher
is slow in debug builds and some tests exercise multi-grid overflow / entropy over meaningful
amounts of data.

Examples double as API-drift tests and should still compile after any `core` API change:

```bash
cargo build --examples --release -p why2 --verbose
```

Benchmarks (Criterion, not run in normal CI — only on `stable` branch on real hardware):

```bash
cargo bench --bench comprehensive
```

There is currently no automated test suite for the `chat` crate; CI only builds it
(`cargo build --release` + the server feature combo above).

## Core architecture (`core/src`)

Pipeline for a single encrypt/decrypt call flows through:

- **`grid.rs`** — `Grid<const W, const H>` is the fundamental state: a fixed-size 2D matrix of
  `i64` cells (default 8×8 = 64 cells of 64 bits). Implements the three SPN transform steps as
  methods: `subcell` (ARX nonlinear mixing, SIMD via `wide::i64x4`), `shift_rows` (row rotation),
  `mix_columns` (true MDS matrix multiply over GF(2^64), coefficients in `consts/mds.rs`).
- **`gf.rs`** — Galois field (GF(2^64)) arithmetic backing `mix_columns`.
- **`crypto.rs`** — key derivation/round-key expansion (ChaCha20-seeded from a SHA-256 hash of the
  grid) and CTR-mode keystream application; parallelized with `rayon`.
- **`encrypter.rs` / `decrypter.rs`** — one-shot, whole-buffer API: `encrypter::encrypt_string`,
  `decrypter::decrypt_string`, etc. Splits input into `Grid`s, generates round keys, applies CTR
  mode across all grids in parallel.
- **`stream.rs`** — `RexStream`: a stateful, incremental version of the same CTR-mode logic for
  processing data in chunks (network sockets, large files) without buffering everything in memory.
  Critical invariant: the internal `block_counter` must track *total Grid blocks processed since
  stream init*, not calls to `update` — reusing a keystream block breaks CTR-mode security. Read
  the module doc comment before touching nonce/counter handling.
  `RexStream` itself is synchronous and CPU-bound; `chat` drives it from async code by calling
  `update`/`finalize` inline between socket awaits (no lock or await is ever held across a
  `RexStream` call).
- **`auth.rs`** (feature `auth`, default-on) — Encrypt-then-MAC via HMAC-SHA256, exposed as
  `AuthenticatedData`.
- **`types.rs`** — `EncryptedData` / `DecryptedData` containers threading key/nonce/output between
  the above modules. Note `EncryptedData::key` is a convenience field for the encrypt/decrypt
  round-trip in a single process — it is not meant to be serialized alongside ciphertext.
- **`consts/`** — round counts (`ROUND_KEYS`), ARX round counts, MDS matrices, default grid
  dimensions (8×8).

Cargo features: `constant-time` (default, via `subtle` — disabling opens timing side-channels) and
`auth` (default, via `hmac`). Grid dimensions `W`/`H` are const generics threaded through nearly
every public type/function — when adding new APIs, follow the existing pattern of defaulting them
to `consts::DEFAULT_GRID_WIDTH`/`HEIGHT` rather than hardcoding 8.

## Chat architecture (`chat/src`)

`why2-chat` layers a custom protocol on top of raw `why2` primitives:

- **Feature flags gate almost everything** (`Cargo.toml`): `client_base` (TUI + core networking),
  `client_voice`, `client_screen` (screen share, pulls in voice), `client` (all three, default),
  `server`. Internal shared code lives behind the `chat` feature, auto-enabled by any of the above.
  When editing `chat/src`, check which feature(s) gate the file/module before assuming code is
  reachable in both client and server binaries.
- **`network/mod.rs`** — shared packet-level plumbing used by both client and server: `Packet`
  struct (control code + sequence number), `SequencedPacket` trait, `EncryptionMode` (either
  one-shot `SharedKeys`-based or a stateful `RexStream`), and `send_tcp`/`read_tcp` helpers.
  **Everything here is async (tokio) — there is no sync path.** `send_tcp`/`send` take a
  `&mut OwnedWriteHalf`; `read_tcp`/`receive` take `&mut Streams<'_>`, the
  `(&mut OwnedReadHalf, Arc<tokio::sync::Mutex<OwnedWriteHalf>>)` alias in `consts.rs`.
  Sequence numbers are used to prevent replay/reordering; obfuscation (`obfuscate_data`, a simple
  XOR) is a distinct, non-cryptographic layer applied on top of the real encryption.
- **`network/client.rs` / `network/server.rs`** — connection-level logic (handshake, auth, message
  dispatch) for each side. `network/file`, `network/screen`, `network/voice` are protocol
  extensions with their own client/server submodules for file transfer, screen sharing (feature
  `client_screen`/`server`), and voice chat (`client_voice`/`server`) respectively — voice runs
  over UDP while text runs over TCP, on the same port.
- **`crypto/kex.rs`** — hybrid key exchange: classical ECC (`p521`) + post-quantum ML-KEM,
  combined via HKDF (`crypto/mod.rs::get_correct_key`, `derive_stream_nonce`) to derive the actual
  `why2` grid key/nonce and HMAC key (`SharedKeys = (why2 key, HMAC key)`) from the raw shared
  secret. `crypto/password.rs` (feature `server`) handles Argon2 password hashing.
  Rekeying happens periodically (`consts::REKEY_INTERVAL`, 10 minutes) to bound the damage from any
  single session key.
  TOFU (trust-on-first-use) server key pinning is expected; `WHY2_SKIP_TOFU` env var (baked in at
  build time via `build.rs`) disables that check for local/dev testing only.
- **`config/mod.rs`** — TOML config for client (`client.toml`) and server (`server.toml`), plus
  server user store (`server_users.toml`) and server keypair storage
  (`server_keys/{private,public,private_pq,public_pq}`), all under `WHY2_CONFIG_DIR` (defaults to
  `~/.config/WHY2`, baked in by `build.rs` unless overridden at build time).
- **`bin/client/`** — the client entrypoint (`mod.rs`), the full-screen TUI (`tui/`, ratatui over
  the crossterm backend), and color handling (`colors.rs`).

  **The client renders through one event loop — there is no printing anywhere else.**
  `tui::run` (`tui/mod.rs`) is a single `tokio::select!` over three sources: the
  `crossterm::EventStream` (keys, resize, mouse wheel), the `mpsc::Receiver<ClientEvent>`, and a
  33 ms redraw tick. `ClientEvent` handling (`App::apply`, `tui/event.rs`) is **pure state
  mutation** — it appends `Line`s to `App::messages`, updates the sidebar/voice roster or sets
  `should_quit`, and never touches the terminal. The tick sets nothing and draws only when
  `App::dirty` is set, which is what keeps `VoiceActivity` (one event per voice packet) from
  repainting hundreds of times a second. Consequences for new code:
  - Never `println!`/`print!` after the TUI is entered. Anything with something to say sends a
    `ClientEvent` (from the network layer) or calls `App::push*` (from a locally handled command in
    `mod.rs::submit`). The one pre-TUI phase — version check, IP prompt, connection failure — runs
    on the normal screen before `TerminalGuard::enter`, and uses `flush_plain`.
  - C libraries that write to fd 2 (cpal/ALSA, openh264/xcap) corrupt the frame; the existing
    `gag::Gag::stderr()` wrappers in `network/voice/client` must stay.
  - `tui::install_panic_hook` is called first thing in `main` and is **not optional**: the release
    profile sets `panic = "abort"`, so `TerminalGuard::drop` never runs on a panic and the hook is
    the only path that leaves the alternate screen.
  - Fatal events (`TofuError`, `Quit`) do not `process::exit` from the draw path. They call
    `App::quit(code, message)`; the loop breaks, the guard restores the terminal, and `run_client`
    prints the message on the normal screen.
  - The message pane is wrapped by `state::wrap_line` (cached per width + history generation) rather
    than by `Paragraph`, so the scroll offset is exact. `App::scroll == None` means stuck to the
    bottom.
  - `config::read_config` re-parses the TOML on every call — read config-driven styling through
    `App::theme` (`tui/theme.rs`), and call `Theme::reload` after a `config::client_write`.
  - Every chrome color in `tui/theme.rs` is a `Color::Rgb` — never a named ANSI color, and never a
    `Color::Indexed` either. Both of those are slots the user's terminal scheme fills in, so
    `Color::Cyan` renders sky blue in one theme and swamp green in the next, and schemes routinely
    redefine the upper greys of the 256-color cube as well; the constants are the reference palette
    so every truecolor terminal draws the client identically. `draw::draw` also paints `theme::TEXT`
    over the whole frame first (and over the palette popup again, since `Clear` resets those cells)
    so unstyled spans do not inherit the terminal's default foreground. Scheme-relative colors
    survive in exactly one place: `colors.rs`, where they are the user's own `/color`/`/ucolor`
    choice.
  - `tui/input.rs`'s `InputBuffer` is the single source of truth for the input line (there is no
    global partial-input state), and `tui/palette.rs` drives the slash-command popup straight off
    `command::COMMAND_LIST` — never duplicate the trigger table. The popup has two modes
    (`PaletteMode`): a filtered command menu while the command word is still being typed, and a
    single-row signature hint highlighting the parameter the caret is on once it is finished.
  - The sidebar is fed by events, never by polling. `App::refresh_online` (a `PacketCode::List`
    request drained on the redraw tick) is only set for things that genuinely change the roster —
    `Authenticated`, `Join`, `Leave`. **A channel switch must not trigger one**: it would land
    inside the server's `min_message_delay` window right behind the `/channel` packet and earn a
    `SpamWarning` (three of those disconnect). The channel list is maintained from the globally
    broadcast `ChannelCreated`/`ChannelDestroyed` packets plus whatever the last `List` showed —
    a channel exists exactly as long as somebody is in it, so the lobby is not one and is not
    listed.
  - Block-command output (`/list`, `/files`, `/screens`, `/help`, `/info`) is a tree, not a table:
    every row opens with `tui::branch` (`├─`/`╰─`, `│` continuing the trunk past a non-last owner's
    files in `/files`) in `theme::BORDER`, then a right-aligned dim id column, then the name. Keep
    new block output to that shape — boxed tables were tried and rejected, and anything wider than
    the message pane is re-wrapped by it and comes out as rubble.
  - `/settings` (`tui/settings.rs`) is a modal overlay, not a block command: while `App::settings.open`
    is set it swallows the keyboard in `tui::mod::handle_key` and suppresses the input caret in
    `draw::draw_input`. Every row writes straight through to `client.toml` (typed, via
    `config::client_write_bool`/`client_write_int`) and into the atomics in
    `network/voice/client/options.rs` that the running cpal callbacks read — there is no save step.
    A row whose config key is phrased as a negative (`disable_colors`) carries `invert`, and the
    inversion happens in exactly one place per direction (`settings::toggle_value` on read,
    `settings::toggle` on write) — inverting on only one side silently makes the row a no-op.
    Device lists come from one `spawn_blocking` cpal enumeration when the command is typed
    (`mod.rs::audio_devices`, gagged stderr), never from the draw path. Picking a device bumps
    `voice_options::mark_devices_changed`, and the voice session's VAD task rebuilds both cpal streams
    (`voice::client::replace_streams`) within its 100 ms tick: the UDP socket, `CONSUMERS` and the
    jitter buffers survive, so the call does not drop. A device that will not open keeps the old pair
    and reports `ClientEvent::VoiceDeviceFailed`.
  - Chat messages live in `App::messages` as `state::Entry::Message` (username/id/text/colors), not as
    rendered `Line`s — `Theme::render` turns an entry into a line on every wrap, so a `show_id` or
    `disable_colors` change repaints the messages already in the pane. Anything that rewrites
    config-driven styling must call `App::reload_theme` (which bumps the wrap-cache generation), never
    `Theme::reload` on its own.
  - Transient prompts belong in the chrome, not the history. The username/password prompts render
    as the input box's title (plus `App::login_hint`) and vanish once answered; nothing pushes them
    into `App::messages`. Block commands (`/help`, `/list`, `/files`, …) end without a trailing
    blank line — the styled headings already separate them.
- **`bin/server.rs`** — headless server entrypoint (`tokio::main`), wires together `network::server`,
  `network::file::server`, `network::screen::server`, `network::voice::server`.
- **`command.rs` / `options.rs`** — in-chat slash commands (`/pm`, `/channel`, `/voice`, etc.) and
  CLI argument parsing respectively.

When adding a new packet type or handler, changes typically need to touch: `network/codes.rs`
(`PacketCode` enum) and both `network/client.rs` and `network/server.rs` (or the relevant
file/screen/voice submodule).

## Concurrency rules (`chat`)

The crate is async top to bottom — both binaries are `#[tokio::main]` and there are no manually
spawned OS threads. When adding code, keep to these rules:

- **Tasks, not threads.** Use `tokio::spawn`. The two exceptions, both genuinely blocking, are
  `tokio::task::spawn_blocking` (screen capture's frame-paced xcap/H.264 loop, Argon2 hashing, the
  `ureq` version check, file hashing for uploads) and the OS threads that `cpal` owns internally
  for audio callbacks.
- **Realtime callbacks never touch the network.** `cpal` input/output callbacks are not async and
  must not block: they `try_send` onto a `tokio::sync::mpsc` channel and a task does the actual
  `voice::send`. The winit event loop does the same for `/deattach` (`ScreenShareRequest.deattach`).
- **Never hold a lock across an `.await`.** This applies to `std::sync::Mutex` guards, and equally
  to `DashMap` `Ref`/`RefMut` guards on `server::CONNECTIONS` and `file::ACTIVE_FILESHARES` — a
  shard lock held across an await will deadlock against `send_to_all`/`remove_connection`. The
  established pattern is to collect what you need into locals in a scoped block, drop the guard,
  then await. Note that `if let Some(x) = map.get(..)` keeps the guard alive for the whole block,
  while a plain `if` condition drops it before the body.
- **Force-closing a connection = aborting its task.** Connections store a `tokio::task::AbortHandle`
  (`Connection::task`, `file_streams`, `screen_stream`) instead of a cloned socket, because tokio
  has no `Shutdown::Both` on a split stream. `server::spawn_with_abort` spawns a task and hands it
  its own handle (via a oneshot, so registration can't race). `remove_connection` is `async` and
  aborts **last**, after all of its awaits — it is frequently called by the very task it aborts.
- **Dropping an `OwnedWriteHalf` shuts the write side down**, so a viewer/attach socket is closed
  simply by dropping the `Arc` that holds it.
