# CLAUDE.md

Guidance for Claude Code when working in this repository.

## What this is

WHY2 is a Rust workspace (edition 2024, resolver 2) with two members:

- **`core`** (`why2`, published to crates.io) — the REX encryption algorithm: a configurable
  grid-based SPN block cipher (ARX nonlinear mixing + MDS diffusion) in CTR mode, with optional
  HMAC-SHA256 authentication. Standalone crypto library, no networking.
- **`chat`** (`why2-chat`, binaries `why2` and `why2-server`) — a reference encrypted chat
  application (text, voice, screen share, files) built on `why2`. Almost all active development
  happens here.

Both crates carry the GPLv3 header block at the top of every source file. Preserve it when creating
files (copy from a sibling).

## Build

```bash
cargo build -p why2 --release                 # core library
cargo build --release                         # chat client (default features)
cargo build --bin why2 --release              # explicit client binary

# Server (no client features; always name windows_resources)
cargo build --bin why2-server --no-default-features \
    --features server,windows_resources --release
```

`chat/build.rs` enforces that client features (`client_base`/`client_voice`/`client_screen`/
`client`) and `server` are never enabled together, and that the internal `chat` feature is never
enabled directly. `WHY2_DEV_BYPASS=1` skips the check for experiments only.

**The one sanctioned exception is documentation**, built with both `client` and `server` so every
module appears. `build.rs` permits this when `DOCS_RS` is set; do not use `WHY2_DEV_BYPASS` for it
(it returns before the `rustc-env` lines, so `env!` on `WHY2_CONFIG_DIR`/`WHY2_GIT_HASH` fails).
The binaries are `doc = false`; only the library is documented, and its docs must not refer to the
binaries. Math is KaTeX via `chat/docs-header.html`:

```bash
DOCS_RS=1 RUSTDOCFLAGS="--html-in-header $PWD/chat/docs-header.html" \
    cargo doc -p why2-chat --no-deps --features client,server --open
```

Rustdoc markdown runs before KaTeX: write subscripts as `\_`, avoid `\,` `\|` `\{` `\!` (use `\ `
and `\Vert`), row breaks as `\\\\`, primes as `^{\prime}`, and leave quotes out of math.

On Windows, `build.rs` embeds `chat/assets/why2.ico`, VERSIONINFO and an application manifest
(`asInvoker`, DPI awareness, UTF-8 code page, long paths). This is why `windows_resources` exists:
`static:+whole-archive` propagates to the final binary, so a crate using `why2-chat` as a library
must build with `default-features = false` to avoid force-linking WHY2's resources into its exe.
New binary build commands must add `windows_resources`; library consumers must not.

Full client (voice + screen share) needs system packages; on Debian/Ubuntu:
`pkg-config libasound2-dev libopus-dev libpipewire-0.3-dev libegl-dev clang libclang-dev
libgbm-dev nasm cmake`. The server build needs none of these. GPU encoders add no build deps
(Vulkan is loaded at runtime).

## Test

Core tests live in top-level `tests/` (registered as `why2_integration` in `core/Cargo.toml`) and
run with `-p why2`. CI always uses `--release`; prefer it locally too.

```bash
cargo test -p why2 --release
cargo test -p why2 encrypt_decrypt --release -- --nocapture
cargo test -p why2 diffusion_test --release -- --nocapture
cargo test -p why2 test_auth_tamper_resistance --release -- --nocapture
cargo test -p why2 stream_test --release -- --nocapture
cargo build --examples --release -p why2 --verbose   # examples are API-drift tests
cargo bench --bench comprehensive                    # stable branch / real hardware only
```

There is no automated test suite for `chat`; CI only builds it. There is deliberately no standing
capture-pipeline benchmark — new measurements must bring their own harness and compare
whole-process CPU, since push backends move work onto other threads.

## Core architecture (`core/src`)

Pipeline for an encrypt/decrypt call:

- **`grid.rs`** — `Grid<const W, const H>`: 2D matrix of `i64` cells (default 8×8 = 64×64 bits).
  SPN steps: `subcell` (ARX, SIMD via `wide::i64x4`), `shift_rows`, `mix_columns` (MDS over
  GF(2^64), coefficients in `consts/mds.rs`).
- **`gf.rs`** — GF(2^64) arithmetic for `mix_columns`.
- **`crypto.rs`** — key derivation/round keys (ChaCha20 seeded from SHA-256 of the grid) and CTR
  keystream; parallelized with `rayon`.
- **`encrypter.rs` / `decrypter.rs`** — one-shot whole-buffer API.
- **`stream.rs`** — `RexStream`, stateful incremental CTR. **Invariant:** `block_counter` tracks
  total grid blocks processed since init, never `update` calls; reusing a keystream block breaks
  CTR. Read the module docs before touching nonce/counter handling. It is synchronous and
  CPU-bound; `chat` calls it inline between socket awaits.
- **`auth.rs`** (feature `auth`, default) — encrypt-then-MAC via HMAC-SHA256,
  `AuthenticatedData`.
- **`types.rs`** — `EncryptedData` / `DecryptedData`. `EncryptedData::key` is a convenience for the
  same-process round trip; never serialize it with ciphertext.
- **`consts/`** — round counts, ARX counts, MDS matrices, default dimensions (8×8).

Features: `constant-time` (default; disabling opens timing side-channels) and `auth` (default).
Grid dimensions are const generics threaded through public types; default them to
`consts::DEFAULT_GRID_WIDTH`/`HEIGHT` rather than hardcoding 8.

## Chat architecture (`chat/src`)

### Features and layout

`client_base` (TUI + core networking), `client_voice`, `client_screen` (implies voice),
`client` (all, default), `server`; shared code behind internal `chat`. Check which feature gates a
module before editing it. Client and server features are mutually exclusive.

- `network/mod.rs` — shared packet plumbing.
- `network/client/`, `network/server/` — handshake, auth, dispatch.
- `network/file/`, `network/screen/`, `network/voice/` — protocol extensions with client/server
  halves.
- `crypto/kex.rs`, `crypto/password.rs`; `config/`; `cache.rs`; `command.rs`; `i18n.rs`;
  `role.rs`; `options.rs`.
- `bin/client/` — TUI; `bin/server.rs` — headless server.

Adding a packet type usually touches `network/codes.rs` plus both `network/client/mod.rs` and
`network/server/mod.rs` (or the relevant extension).

### Packet transport

`network/mod.rs` defines `Packet` (control code + sequence), `SequencedPacket`, `EncryptionMode`,
and `send_tcp`/`read_tcp`. `send_tcp`/`read_tcp` never touch a cipher; `crypto` does.

Both modes are authenticated:

- One-shot: `crypto::encrypt_packet` / `decrypt_packet` (`why2::auth::AuthenticatedData`).
- Stream: `RexPacketStream::seal` / `open` around a `RexStream`. Tag is HMAC-SHA256 over
  `counter || len || ciphertext` under a stream key derived from the session HMAC key and transfer
  token. Framing is `[len][32-byte tag][ciphertext]`. The counter prevents replay/reordering;
  `open` verifies **before** advancing the stream, so a forged frame costs nothing.
- The bare `RexStream` survives only in `file/server.rs::disk_stream` (at-rest upload encryption,
  not a packet).

Everything is async (tokio); `send_tcp` takes `&mut OwnedWriteHalf`, `read_tcp` takes
`&mut Streams<'_>` (read half + `Arc<Mutex<OwnedWriteHalf>>`). Sequence numbers stop replay;
`obfuscate_data` is a non-cryptographic XOR layer on top of real encryption.

### Rate limiting

Two different rules, because a typed message and a packet are not the same:

- `min_message_delay` is a minimum gap for `PacketCode::Message` only. Violations earn
  `SpamWarning`; `max_message_delay_violations` ends the session.
- Everything else uses a per-connection token bucket (`Connection::take_credit`, called from
  `network::receive`'s server block). Refill at `max_packet_rate` up to `max_packet_burst`.
  Overdrawn packets are **delayed on that connection's own read loop**, not warned; only
  consecutive throttles count toward `max_packet_rate_violations` (disconnect reason `RATE`).
  `KeepAlive` and rekeys are free; `Message` is charged.

Voice has its own bucket (`voice/server.rs`, `voice::consts::MAX_PACKET_RATE`/`BURST`), charged per
datagram after the seq check and **dropped** rather than delayed — the UDP socket is shared by all
clients. Only the first drop of a run is logged. Known gap: this sits after `voice::receive`, so it
bounds fan-out, not per-datagram decrypt; a source-address bucket ahead of decrypt would be needed.

Auxiliary TCP sockets (file, screen, attach) have no packets-per-second cap: a packet is payload
there, so a cap would be a throughput cap. They are bounded by `max_upload_size`, `MAX_ATTACHMENT_SIZE`,
`max_client_parallel_uploads`, `VIEWER_CHANNEL_BOUND`, `SOCKET_BUFFER`, and by requiring a token
minted over the main connection.

The bucket and `last_image` survive rekeys and channel switches. Unauthenticated connections are
bounded by `max_unauth_clients`/`max_auth_time`. All these keys are live and gated by
`spam_protection`.

Image requests are spaced by `IMAGE_REQUEST_DELAY` on the server (`last_image`), served late
rather than refused (the client never retries), and a miss is answered with empty data so the
caption shows `[ unavailable ]`. The client keeps at most `MAX_IMAGE_FETCHES` in flight; a queued
request whose caption left the screen is not sent.

### Broadcast and channels

`server::send_to_all` sends to every authenticated client; the client decides which pane. There is
no channel filter on the server.

- `Message` / `ImageDisplay` carry `channel: Option<Option<String>>`: `Some(None)` lobby,
  `Some(Some(name))` channel, `None` all panes. Foreign messages go to the parked pane
  (`App::park_entry`, trimmed to `HISTORY_LIMIT`). Foreign pictures are cached, not decoded; the
  caption is parked as `Picture::Deferred`.
- `VoiceJoin`/`VoiceLeave` carry `channel: Option<String>` and are ignored outside that channel.
- `send_to_others` (typing) still filters on the server.
- Every channel's text reaches every client, so channels are places to talk, not confidentiality
  boundaries. A fresh picture is sent once per client.

### Images, cache, voice messages

Images:

- `MAX_ATTACHMENT_SIZE` (8 MB) bounds bytes on the wire, not decode cost. Decode through `ImageReader`
  with `MAX_IMAGE_DIMENSION` and `MAX_IMAGE_ALLOC`; **never** `load_from_memory` (its 512 MB
  default caps one picture, not a flood). `PacketCode::Image` stays in the spam window.
- Decoded results are `Animation` (`Vec<ImageFrame>`). GIF, animated WebP and APNG use
  `AnimationDecoder`; everything else is one frame. `MAX_ANIMATION_FRAMES`/`MAX_ANIMATION_ALLOC`
  cap animations; running out truncates rather than fails. Frames below `MIN_FRAME_DELAY` get
  `DEFAULT_FRAME_DELAY`.
- `server_images/` is encrypt-then-MAC under per-picture keys HKDF'd from `server_image_key` and
  the content hash. Uploads are MAC'd as they arrive (`ActiveFileshare::mac`, tag appended by
  `crypto::disk_tag`); fileshares are not, because their key is ephemeral and they stream out.
- **Chunking is part of the format:** `UPLOAD_CHUNK_SIZE` is 1,000,000 bytes (10^6, not 2^20), so
  it is 1953.125 grids of 512 bytes. A `RexStream` finalized mid-file advances the counter, so
  `read_attachment` must break the file exactly where the upload did. Do not assume one `update` over
  the whole file is equivalent.
- `ImageDisplay` carries a hash and optional payload. Known pictures are sent as `data: None`
  (clients fetch if missing); unknown ones go whole. The client cache is keyed by content and
  scoped by the server fingerprint (`options::get_fingerprint`), so one server cannot probe
  another's cache. The hash is re-taken over received bytes, never trusted. Cache files are
  encrypted/authenticated under `fingerprint || hash`; an invalid file is deleted so the next fetch
  refills it. `MAX_ATTACHMENT_CACHE` evicts by mtime.
- Replayed pictures load when looked at (cache first, then `AttachmentDataRequest`), using
  `PRELOAD_SCREENS` lookahead. With `auto_show_images` off, live and replayed pictures appear as
  `[ show ]` captions (`Picture::Absent`) and clicks go through the cache first.

Voice messages reuse the image machinery (`/record`, `/play`):

- `Clip` in `network/voice/message.rs`: `MAGIC` (`WHY2VOX\x01`) plus wincode
  `{ channels, frames: Vec<Vec<u8>> }`, one Opus packet per 20 ms frame. `Clip::decode` validates
  channel count, frame count (`MESSAGE_MAX_FRAMES`), non-empty packets, and Opus's 1275-byte
  ceiling.
- The server parses the container but decodes no audio. `VoiceNote { hash, duration }` is measured
  server-side. `MAX_VOICE_MESSAGE_SIZE` is 6 MB.
- Voice messages have their own packets/history field (`VoiceMessage*`, `Record::voice`,
  `StoredMessage::voice`), not an image flag. `messages::stored()` stays pictures-only;
  `messages::voice(hash)` answers clips; `AttachmentDataRequest` serves either.
- Recording bypasses denoise/VAD/AGC (only mic volume applies). It taps the call when one exists;
  otherwise it opens `input_device` on a blocking thread. Encoding happens in the capture callback
  with `try_lock`, never blocking realtime audio.
- Playback opens its own output stream and decodes packet-by-packet. One clip at a time;
  `PLAY_GENERATION` makes later commands win. Known gap: playback is not in the AEC reference.
- `Ctrl+R` is push-to-talk: a tap is a toggle. Releases require kitty keyboard protocol; otherwise
  auto-repeat timing (`KEY_TAP`, `KEY_REPEAT_GAP`, `KEY_RELEASE_GAP`) is used. `/record` is the
  typed toggle; `Esc` discards; `MESSAGE_MIN_FRAMES` (300 ms) suppresses accidental taps.

### Screen sharing

Capture (`network/screen/client/capture.rs`, the only blocking CPU loop in the client):

- Backend chosen at runtime, never assumed. The polling path starts immediately; the recorder
  probe runs beside it and takes over only after proving a frame (`UPGRADING`). The probe never
  gates the share. The portal is ready once its PipeWire stream reaches `Streaming` (compositors
  may send no frame on a still desktop); other recorders must deliver `RECORDER_FIRST_FRAME`.
  `RECORDER_PROBE_TIMEOUT` only applies when polling cannot start at all. Errors name both
  backends.
- Wayshot path recycles its Wayland connection after `WAYLAND_LEAK_BUDGET` because `libwayshot`
  leaks a shm buffer per capture. `ShmCapture` reads compositor bytes directly (one memfd, row
  copies, `BGRx` unswizzled) and falls back to libwayshot for unsupported formats.
- Monitor selection is client-local: `/screen [INDEX|NAME]` stores it; the `Screen` packet only
  toggles sharing. Selection lasts only for the share (reset on end/session loss). `/screen MON`
  during a share swaps capture without ending it (`MONITOR_GENERATION`); naming the current
  monitor or omitting it is a toggle. On Wayland a picked monitor pins the polling path so the
  portal cannot re-ask.
- On Wayland the recorder is `portal.rs`, not xcap's: it waits for portal consent, offers
  `maxFramerate` capped at the share fps, passes BGRx through, recycles buffers, and closes the
  session on stop.
- Frames wait for their slot rather than being dropped, and slots are scheduled off each other
  (`due += min_interval`). A drain thread keeps only the newest frame so capture overlaps encode.
- Env: `WHY2_CAPTURE_BACKEND` (`recorder`/`legacy`), `WHY2_CAPTURE_PROBE_TIMEOUT`.
- Share audio is captured per application on Linux: `screen/client/audio_pipewire.rs` links every
  other `Stream/Output/Audio` node into one capture stream and skips our own by PipeWire client
  (`client.id` resolving to this process; cpal's `cpal-pulseaudio-{pid}` and `PipeWire ALSA
  [{exe}]` names cover the node before that id arrives), so the call's playback — an ALSA client,
  not a Pulse one — is never in the share. If PipeWire is unavailable (or the sandbox forbids
  linking) it falls back to the default output's cpal monitor with the AEC; Windows always uses
  the WASAPI loopback + AEC. `WHY2_SHARE_AUDIO` (`pipewire`/`monitor`) pins the path.

Latency is bounded by shedding, not buffering:

- `screen::cap_socket_buffers` caps `SO_SNDBUF`/`SO_RCVBUF` at `SOCKET_BUFFER` on all four ends so
  a full link is felt by the encoder. Audio uses `try_send` and drops backlog
  (`AUDIO_BACKLOG_TARGET`) instead of queueing latency.
- `RateControl` adapts bitrate from send blocking and Linux `TCP_INFO` backlog/RTT, backing off
  and creeping back under a ceiling. openh264 needs its layer-0 max bitrate moved with the
  target; `Software` passes real timestamps.
- Viewers report their backlog over a separate reverse REX stream (`ScreenPacketCode::Feedback`);
  the sharer takes the larger of its own delay and the server's. The report is backward/forward
  compatible.
- Each viewer has its own task and `VIEWER_CHANNEL_BOUND` queue, so a slow viewer is shed on its
  own socket. A shed viewer waits for a keyframe (`needs_key`) rather than receiving broken
  references. Attach builds the viewer immediately on the last cached keyframe and then snaps to
  live on the next IDR. Dropping a viewer aborts its task (it may be stuck in `write_all`).

Encoding:

- RGBA/BGRA → I420 runs on the CPU through openh264's AVX2 packed path (~0.78 ms at 1080p).
  There is deliberately no GPU converter.
- Hardware encoders are tried first: Vulkan Video on Linux, Media Foundation on Windows,
  VideoToolbox on macOS; openh264 camera mode is the fallback. The wire format stays Constrained
  Baseline (no CABAC, B-frames, or 8×8 transform) because the viewer uses openh264. Every IDR
  carries SPS/PPS.
- `encoder::Budget` skips frames (never forced keyframes) to hold `H264_BITRATE` (8 Mbps, a
  ceiling). Only the Linux GPU backend has been run on hardware; Windows/macOS are type-checked
  only, and failure falls back safely.
- Software encoder uses `UsageType::CameraVideoRealTime`, not screen-content mode; `skip_frames`
  stays on.

Viewer (`screen/client/video.rs` + `yuv_to_rgba.wgsl`): wgpu surface, three `R8Unorm` planes,
BT.601 conversion and scaling in the fragment shader. Allocate planes at decoder stride and trim
padding in the shader. Never write through an sRGB view (double gamma). Letterbox rather than fill.

### Voice

- VAD noise floor falls fast and rises slowly (`NOISE_FLOOR_RISE`), and only from frames below the
  current open threshold, so joining mid-sentence cannot teach the gate that speech is noise.
- AEC (`voice/client/aec.rs`) subtracts WHY2's own playback from the captured monitor mix on
  Windows and on the Linux fallback; Linux normally captures per application through PipeWire and
  never sees our nodes. It is a fixed digital delay plus gain, not acoustic cancellation. Use
  coarse-to-fine correlation (`AEC_SEARCH_DECIMATION`) and accept peaks by `AEC_PEAK_SIGMA`, not
  absolute correlation. Gate NLMS adaptation by `AEC_ADAPT_RATIO`; keep `AEC_STEP` tiny; roll
  back to the best weights (`AEC_ROLLBACK_*`) instead of resetting. Align dropped capture chunks
  with `skip_reference`; track reference-ring underruns as drift and slide the offset. Failures
  pass audio through untouched. Known gaps: attached-share playback and voice-message playback
  are not in the reference (the Linux PipeWire path excludes them with the rest of the process).

### Crypto, roles, accounts

- `crypto/kex.rs`: hybrid P-521 + ML-KEM-768, combined via HKDF to derive the `why2` grid
  key/nonce and HMAC key. Rekey every `REKEY_INTERVAL` (10 min). TOFU pins the server identity;
  `WHY2_SKIP_TOFU` disables it (build-time, dev only). Passwords use Argon2 server-side.
- `role.rs`: `User`/`Moderator`/`Owner`; declaration order is permission order (`role >= ...`).
  `roles!` generates variants, names and palette entries together. Roles cross the wire and the
  user file as their names. The live connection's role is re-read per packet.
- Moderation targets accounts, not sessions: `resolve_user` accepts a username or live id;
  `session_of` finds the account's session. Ban/pardon/role work offline; kick/banip/mute need a
  connected id.
- Colors are server-owned: `PacketCode::Colors` stores a color name per account in
  `server_users.toml`. The server paints every message from the account's current color; clients
  neither store nor send colors. `colors::COLORS` is the wire order; names survive reordering.
- Profiles (`bio`, `pronouns`, `website`, `status`) live in `[user.profile]` in
  `server_users.toml`; `UserProfile::KEYS` is the single list of typed fields. `online_status` is a
  boolean beside them (missing reads as `true`): `send_list` is the same for every client and
  leaves hidden accounts out of its online section, a hidden account broadcasts no `Join`/`Leave`
  at login, and a change of visibility sends every client the matching `Join`/`Leave`. The
  client's own username comes in `Accept`, not `Join`, so the hidden client can be skipped too.
  The client only asks and displays. Bounds (`max_profile_bio`,
  `max_profile_field`) refuse rather than truncate; URLs must be http/https. `avatar` is a hash
  outside `KEYS`; the client crops/re-encodes, the server only checks the header (`is_avatar`) and
  `MAX_AVATAR_SIZE`. Avatars are ordinary stored pictures,
  retained by `messages::stored` when a profile names them. The server icon is the same machinery
  with the hash in `config::server_icon`, owner-set, requested rather than pushed.

### Config and history

- Config lives under `WHY2_CONFIG_DIR` (default `~/.config/WHY2`): `client.toml`, `server.toml`,
  `server_users.toml`, `server_bans.toml`, `server_keys/`. At-rest keys (`server_history_key`,
  `server_image_key`, `client_cache_key`) live in the config root, 0600, not in `server_keys/`
  (which is the identity). `server_keys/` is 0700; the root is not.
- Lobby history is off by default (`persistent_messages`), stored as `server_messages.bin`
  (wincode `Vec<Record>`, not TOML), encrypted and authenticated with a fresh nonce per write
  under `history_key`. A missing/tampered/unreadable file loads as empty; the old file is moved to
  `.old` before being ignored. Format changes need a frozen old `RecordVn` + conversion in `load`.
- `Record` is not `StoredMessage`: colors are looked up per sender on replay, so recoloring an
  account repaints its history.
- Every message has a server-assigned id (`Record::id`, `StoredMessage::message_id`), allocated
  under the `HISTORY` lock and shared by lobby + channels. Timestamps are server-assigned unix
  seconds, optional via `message_timestamps`; ids remain the ordering.
- Replies (`reply: Option<u64>`) name a stored lobby message; the TUI resolves the target at wrap
  time. A reply quote is always exactly one row.
- `/delete ID` removes your own message, or a lower-ranked user's as a moderator; pictures nothing
  else names go too. `/edit ID TEXT` is author-only, text-only, non-empty, and subject to the same
  limits as a message. Both broadcast `Deleted`/`Edited`.
- History is paged newest-first (`history_page`, `MAX_HISTORY_SIZE`); the cursor is a message id
  found by binary search and stays stable across inserts/trims/deletes. Only the lobby persists;
  channels die with their last member.
- Chat entries are striped (`App::stripe_bg`, `message_stripes`), derived from the terminal
  background; mentions win over stripes; the logo treats painted cells as claimed.

### Client TUI

- All tuning constants live in `tui/consts.rs`. Non-knobs stay local: theme palettes, math symbol
  tables, command list, `include_str!` assets.
- Every user-facing string is a key in `chat/locales/en.toml` (`t()`, `tn()`), never a literal.
  English is embedded and is the fallback; other locales come from the config dir or `BUILTIN`.
  Machine translations are marked ` [AI]`. Placeholders are whole-sentence, never glued. Glyphs
  and padding stay in code. Command triggers, color/role names and server config text are not
  translated.
- One event loop: `tui::run` selects over crossterm events, `ClientEvent`, dial results, and a
  33 ms redraw tick. `App::apply` is pure state mutation; the tick draws only when `App::dirty`.
  Never `println!` after the TUI starts. Fatal events call `App::quit`, not `process::exit`.
- The client enters the alternate screen immediately; login is a TUI box (`login.rs`) with three
  stages (address, username, password). Only address is client-driven. `auto_connect` prefills
  and dials. A lost session returns to the box; `Reconnect` replays only credentials that worked,
  bounded by `RECONNECT_ATTEMPTS` and gated on `retrying`. `/logout`, `Esc`, registration and a
  rejected username forget the pair. `Command::Exit` ends the process.
- `tui::install_panic_hook` is mandatory: `panic = "unwind"` only restores the terminal for a
  main-thread panic while the guard is alive; spawned-task panics need the hook.
- Messages are wrapped by `state::wrap_line`, not `Paragraph`; `App::scroll == None` sticks to the
  bottom. Config-driven styling is read through `App::theme`; after `config::client_write` call
  `App::reload_theme`, never `Theme::reload` alone.
- Pictures are animations with one frame; the tick is the clock. Only visible/preloaded pictures
  are stepped. Rebuild protocols around their existing `StatefulProtocolType` so kitty replaces
  rather than duplicates. Pictures are drawn last; overlays copy their cells out and restore them.
  `AlwaysUpdate` is only for restored box cells; picture rows use a second cursor save so the rest
  of the row is still diffed. Replaced rows are tracked via previous-frame overlay rects.
- Mouse capture takes away terminal selection and URL clicks, so the client implements both.
  Selection is stored in wrapped-view rows, copies via OSC 52, and only starts on drag. URL
  opening accepts only http/https and opens on the machine running the client.
- Toasts and the typing indicator live in the pane's bottom border, never in history.
  `App::expire_notice`/`expire_typing` only dirty when something actually changes.
- Chrome colors are `Color::Rgb` from `theme::PALETTES[ACTIVE]`, never ANSI names or indexed
  colors. Palettes paint their own background; `why2` is absolute and carries its own ANSI table.
  A new palette is one `PALETTES` entry. “Unpainted” checks must accept both `theme::background()`
  and `Color::Reset`.
- `InputBuffer` is the sole input state; the palette is driven from `command::COMMAND_LIST`.
  Signature hints are the second palette mode. Path completion reads the disk (`ArgValues::Paths`),
  offers directories with a trailing separator, hides dotfiles until `.` is typed, preserves
  on-disk spelling, and expands `~` through `misc::expand_home`.
- Every scrollable surface draws `draw::draw_scrollbar`; it overwrites the border, not a column,
  and is hidden when everything fits. The thumb is positioned by hand.
- A channel switch parks the current pane in `App::panes` (keyed by name, `""` lobby); it is
  pruned when the channel empties. Nothing is replayed by the server. A lost session clears all
  panes.
- The roster is maintained by `Join`/`Leave` events, never polled; the server pushes the initial
  roster after `Accept`. Never request a list after a channel switch or kick (it lands inside
  `min_message_delay` and can earn a spam warning). `/list` is the explicit resync.
- Offline users are a separate box fed by `OfflineUser` entries in `List`, gated by
  `show_offline_users`. A `None` offline list means “not sent”, not “none”.
- The voice panel merges server roster (`voice_roster`) with local activity (`voice_activity`);
  never write `App::voice` directly. Latency is `Option` (blank when unknown). A per-user mute is
  only drawn while listening.
- Typing is a statement with a lifetime: resend at most every `TYPING_INTERVAL`, expire after
  `TYPING_TIMEOUT` (twice the interval). Only the tick sends, driven by `InputBuffer::revision`.
  Empty lines and commands reset the flag. The server re-states the interval, drops late notices,
  and muted users send none.
- Block command output is a tree (`tui::branch`), with a right-aligned dim id column. Do not use
  tables.
- `/settings` is a modal overlay writing through to `client.toml` and live atomics; negative keys
  invert in exactly one place per direction. Device lists come from
  `voice::client::list_devices`/`audio_hosts`; store the cpal device id. A device switch rebuilds
  both streams within the VAD tick, dropping the old pair first; failure restores the previous
  pair.
- `/server settings` is the same overlay in server mode; rows are held until save, and the ack is
  the whole config so refusals snap back. The client sends server keys with their comments and
  only writes existing keys with existing types. `SERVER_RESTART_SETTINGS` marks startup-only keys
  (`server_ip`, `server_port`, `enable_voice_chat`, `server_username`, `log_level`). The restart
  button disconnects everyone and re-execs (`misc::restart`); it refuses while edits are unsaved
  and requires two presses.
- User text goes through `tui/markup.rs` (fenced/inline code, emphasis, links, `$…$` math), and
  `Theme::render` returns rows. The parser never consumes an unclosed delimiter. Fenced blocks are
  rows padded to the pane and are not re-wrapped. Line markers apply only at row start and own
  their wrapped rows. Links show their target; a link whose text is its URL is shown once.
- `tui/math.rs` lays out a deliberate TeX subset in cells. Display math owns rows; inline math is
  linear (Unicode superscripts or `^(…)`, `a/b`) and falls back to linear when too wide. Unknown
  commands lose only the backslash. Depth is bounded by `MAX_DEPTH`. `render_math` toggles it.

### Server entrypoint and commands

`bin/server.rs` (`tokio::main`) wires `network::server`, `network::file::server`,
`network::screen::server`, `network::voice::server`. `command.rs` holds slash commands;
`options.rs` parses CLI arguments.

## Server logging

The server uses `log` + `simple_logger` (server-only). Two rules:

- Identify a client by address only. Never log usernames, message text, channel/file names,
  passwords, keys, tokens or hashes. Log the server's vocabulary and shape: packet code
  (`PacketCode::name`, variant name only; `PacketCode` deliberately has no `Debug`), byte/char
  counts, limits, roles, connection counts.
- Log an auxiliary connection as the main connection that requested it. `server::log_addr(&id)`
  walks `CONNECTIONS` and must never be called while a guard on that map is held.

`log_level` defaults to `info` (operator view); `debug` adds per-packet and handshake detail. It is
read once when the logger is built, so it is restart-only and `bin/server.rs` calls
`config::init_config` before building the logger. `warn` is a client refusal; `error` is the
server's own failure.

## Concurrency rules

- **Tasks, not threads.** Use `tokio::spawn`. The only blocking exceptions are `spawn_blocking`
  (capture loop, Argon2, version check, upload hashing) and cpal's internal audio threads.
- **Realtime callbacks never touch the network.** cpal and winit callbacks `try_send` onto tokio
  channels; tasks do the I/O.
- **Never hold a lock across an `.await`**, including `DashMap` `Ref`/`RefMut` on `CONNECTIONS`
  and `ACTIVE_FILESHARES`. Collect into locals in a scoped block, drop the guard, then await.
  `if let Some(x) = map.get(..)` keeps the guard for the whole block; a plain `if` condition drops
  it before the body.
- **Force-closing a connection means aborting its task.** Connections store `AbortHandle`s
  (`Connection::task`, `file_streams`, `screen_stream`); `spawn_with_abort` registers the handle
  via oneshot. `remove_connection` aborts last, after all awaits — it is often called by the task
  it aborts.
- Dropping an `OwnedWriteHalf` shuts down the write side, which is how viewer/attach sockets close.
