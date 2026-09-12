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
cargo build --bin why2-server --no-default-features --features server,windows_resources --release
```

The client TUI is built on `ratatui` (feature `crossterm_0_29`, so it reuses the crossterm 0.29
already in the tree instead of pulling a second backend). The server build has no UI dependencies.

`chat`'s `build.rs` enforces that `client_base`/`client_voice`/`client_screen`/`client` and
`server` are never enabled together, and that the internal `chat` feature is never enabled
directly (only via `client*` or `server`) — it will `panic!` with an explanatory message if you
get this wrong. Set `WHY2_DEV_BYPASS=1` to skip that check when experimenting with unusual feature
combinations (never use it for real builds).

`build.rs` also embeds the Windows resources (`winresource`, the maintained fork of `winres`):
`chat/assets/why2.ico` — generated from the repo-root `icon.png` — plus a VERSIONINFO block and an
application manifest. This is skipped unless `CARGO_CFG_TARGET_OS == "windows"`, and a failure is a
`cargo:warning` rather than a build error, so a host without a resource compiler still builds. The
metadata is not cosmetic: an unsigned, metadata-less binary that opens sockets and captures the
screen is exactly the shape SmartScreen/Defender heuristics flag, and the manifest is where
`asInvoker` (no UAC prompt), per-monitor DPI awareness, the UTF-8 active code page and long-path
support are declared. `FILEVERSION`/`PRODUCTVERSION` come from `CARGO_PKG_VERSION` automatically;
`InternalName`/`OriginalFilename` follow the feature set, since one build script serves both
binaries. Real code signing is still the only thing that removes the warning outright.

**The `windows_resources` feature is why every server build command names it explicitly.** A build
script's `cargo:rustc-link-lib=static:+whole-archive` propagates to the final link of whatever
binary depends on the crate — verified with `cargo rustc -- --print=link-args` — so with the
resources unconditional, any downstream crate using `why2-chat` *as a library* on Windows would get
WHY2's icon, `CompanyName` and manifest force-linked into **its** exe, and a collision on resource
id 1 if it embeds its own. Cargo gives a build script no way to tell it is being built as a
dependency (`CARGO_PRIMARY_PACKAGE` is not passed to build scripts), so the switch is a feature: it
lives in `default` and in nothing else, which is what makes `default-features = false` a working
opt-out for a library consumer. A new binary build command has to add it; a new *library* consumer
must not.

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
(`cargo build --release` + the server feature combo above). The one exception is the screen
capture colour conversion, which is checked against openh264's own CPU conversion:

```bash
cargo test -p why2-chat --features client_screen --lib gpu:: --release
```

That test **passes trivially on a machine with no GPU** — `GpuConverter::new()` returning `Err` is
the case the CPU fallback exists for, so it returns rather than failing. Do not "fix" it into a
hard failure.

There is deliberately **no standing benchmark for the capture pipeline** — the per-stage
instrumentation and the headless comparator that produced the GPU-conversion numbers were
development scaffolding and were removed once the work landed. Anything measuring capture cost
again has to bring its own harness, and should compare whole-process CPU rather than per-stage
wall time: a push backend moves acquisition onto its own thread, so stage timings alone flatter it.

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
  one-shot `SharedKeys`-based or a stateful `crypto::RexPacketStream`), and `send_tcp`/`read_tcp`
  helpers.
  **Both encryption modes are authenticated, and neither one does it here** — `send_tcp`/`read_tcp`
  hand the packet to `crypto` and never touch a cipher themselves. One-shot goes through
  `crypto::encrypt_packet`/`decrypt_packet` (`why2`'s `AuthenticatedData`); stream mode goes through
  `RexPacketStream::seal`/`open`, which is encrypt-then-MAC around a `why2` `RexStream`: the tag is
  HMAC-SHA256 over `counter || len || ciphertext` with a stream-specific key derived from the session
  HMAC key and the transfer token (`derive_stream_mac_key`), and the wire framing is
  `[len][32-byte tag][ciphertext]`. The **counter in the tag is what makes it a stream** rather than a
  bag of individually-valid packets: the ciphertext MAC alone would accept a replayed or reordered
  frame (the key is static), and a CTR-mode receiver whose counter has moved on would decrypt it to
  garbage rather than reject it. `open` verifies **before** advancing the `RexStream`, so a forged
  packet costs the transfer nothing; a failure means the peer is not who it was and the caller ends
  the transfer. The bare `RexStream` survives in exactly one place, `file/server.rs`'s `disk_stream`
  — that is at-rest encryption of an upload, not a packet.
  **Everything here is async (tokio) — there is no sync path.** `send_tcp`/`send` take a
  `&mut OwnedWriteHalf`; `read_tcp`/`receive` take `&mut Streams<'_>`, the
  `(&mut OwnedReadHalf, Arc<tokio::sync::Mutex<OwnedWriteHalf>>)` alias in `consts.rs`.
  Sequence numbers are used to prevent replay/reordering; obfuscation (`obfuscate_data`, a simple
  XOR) is a distinct, non-cryptographic layer applied on top of the real encryption.
- **A client is rate-limited twice, by two different rules, because a typed message and a packet are
  not the same thing.** `min_message_delay` is a minimum *gap*, and it applies to `PacketCode::Message`
  alone: a person typing faster than that is spamming, so they earn a `SpamWarning` and
  `max_message_delay_violations` of them ends the session. Nothing else on the wire has that shape —
  `/list`, a channel switch, a file offer and the cache's `ImageData` walk all arrive in legitimate
  bursts, and holding them to a gap would warn a user about traffic they never authored. So everything
  else is bounded by *rate*: a token bucket per connection (`Connection::take_credit`, called from
  `network::receive`'s server block, the one funnel every authenticated TCP packet passes through),
  refilled lazily at `max_packet_rate` a second up to `max_packet_burst` — two `f32`s and an `Instant`,
  so it costs no timer task and no client-sized ring of timestamps.
  - **It throttles rather than warns**, which is the `IMAGE_REQUEST_DELAY` pattern generalised: an
    overdrawn packet is answered late, the wait sitting on **that connection's own read loop** so it
    costs nobody else, needs no new packet code and needs no client change at all. Disconnect is only
    the ceiling — since the sleep is already backpressure, credit can keep falling only if the client
    is pipelining without waiting, so `max_packet_rate_violations` counts *consecutive* throttled
    packets and the disconnect reason is `RATE`.
  - **`KeepAlive` and the rekey pair pay nothing**: they are the server's own schedule, not the
    client's traffic, and a throttled client's keepalive answer is stuck behind its own packets in the
    same stream and cannot be reordered — a stall long enough to miss one would disconnect it for the
    wrong reason. The ceiling is deliberately far short of that (40 packets at 15/s is ~2.7s against a
    30s keepalive window). `Message` *is* charged, for uniform accounting; `min_message_delay` is
    stricter by an order of magnitude, so the bucket never bites a message first.
  - **Voice has a bucket of its own, and it sheds instead of waiting.** `voice/server.rs`'s
    `Connection::take_credit` is the same arithmetic against `voice::consts::MAX_PACKET_RATE` /
    `MAX_PACKET_BURST`, charged per datagram after the seq check, and an overdrawn packet is dropped.
    It cannot be held: `listen_client_voice` is the **one UDP socket every client on the server speaks
    through**, so a sleep there would stall everybody's call rather than the sender's. The rate is the
    codec's and lives in `voice/consts.rs` rather than in `server.toml` — one frame per `FRAME_MS` is
    50 packets a second, and the headroom above that is jitter, not taste — while `spam_protection`
    still switches it off. What it bounds is amplification: a voice packet is fanned out to the whole
    channel, and the seq only has to *rise*, which costs a flood nothing. Only the first drop of a run
    is logged (`throttled`), since the flood it is reporting would otherwise be a flood of log lines.
    **Known gap:** this sits *after* `voice::receive`, so it bounds the fan-out and not the per-packet
    decrypt that same shared loop does for every datagram naming a known id — bounding that means a
    bucket keyed on the source address, ahead of the decrypt.
  - **The auxiliary TCP sockets (file, screen, attach) deliberately have none of this.** There a packet
    *is* the payload rather than a request, so a packets-per-second cap is a throughput cap in disguise
    — it would fight the encoder's own frame rate and duplicate the backpressure TCP already applies —
    and their costs are bounded in the units that fit them (`max_upload_size`, `MAX_IMAGE_SIZE`,
    `max_client_parallel_uploads`, `VIEWER_CHANNEL_BOUND`, `SOCKET_BUFFER`). They are limited at the
    door instead: every one of them needs a token minted over the main connection, and *that* request
    is charged to the bucket like any other packet.
  - The bucket is carried across a rekey and a channel switch for the same reason `last_image` is — a
    client that could refill it by switching channels would not be limited at all — and a
    `NonAuthenticated` connection pays nothing here, being bounded by `max_unauth_clients` and
    `max_auth_time` instead. All three keys are live (read at point of use, so they are not in
    `SERVER_RESTART_SETTINGS`) and gated by the same `spam_protection` switch as the message rule.
- **The two costs an image puts on somebody else are bounded explicitly, because neither is bounded
  by the 8MB `MAX_IMAGE_SIZE` the server accepts.**
  - **Decoding is limited on the client** (`network/client.rs::decode_image`). `MAX_IMAGE_SIZE`
    bounds the bytes on the wire and says nothing about what they unpack to: a 292KB PNG decodes to
    400MB, and `ImageDisplay` is **pushed rather than asked for**, so every client in the channel
    decodes whatever was posted, one unbounded `tokio::spawn` per packet. `image`'s own default
    (`Limits::default()`, which `load_from_memory` uses) caps a single decode at 512MB and does not
    bound the dimensions at all — that is a limit on one picture, not on a flood. Every decode
    therefore goes through `ImageReader` with `MAX_IMAGE_DIMENSION` and `MAX_IMAGE_ALLOC` set, and
    never through `load_from_memory`. The residual is `MAX_IMAGE_ALLOC` × however many images a
    sender can post, which is what the ordinary spam window bounds — `PacketCode::Image` is
    deliberately *not* in `receive`'s exemption list.
  - **A decoded picture is always frames, never one image** (`client::Animation`, a `Vec<ImageFrame>`).
    An animated GIF sent through `/image` is uploaded and stored as the bytes it is — nothing on the
    path re-encodes it — so the only thing that ever flattened it was the decode, which took the first
    frame and threw the rest away. `decode_image` therefore branches on the *format*: GIF, animated
    WebP and APNG go through `AnimationDecoder` and everything else (including a GIF of one frame, and
    a WebP or PNG carrying no animation) comes back through `ImageReader` as the single frame it is. A
    still is one `ImageFrame` and an animation is all of them, so every path below carries the same
    type and only the pane cares which it was handed.
    **The frame budget is its own, because `MAX_IMAGE_ALLOC` bounds one picture and an animation is a
    picture per frame** — 8MB of GIF on the wire is hundreds of them held at once. `MAX_ANIMATION_FRAMES`
    and `MAX_ANIMATION_ALLOC` cap what is kept, and a budget running out (or a frame that will not
    decode) **truncates rather than fails**: what has already been decoded still plays, since half a
    GIF beats no picture at all. A frame asking for less than `MIN_FRAME_DELAY` is asking for "as fast
    as possible" and gets `DEFAULT_FRAME_DELAY`, which is the answer every browser has given since
    Netscape.
  - **Fetching is spaced on the server** (`listen_client`'s `PacketCode::ImageData` arm). The
    request is a hash and the answer is a disk read, a whole `RexStream` decrypt and up to
    `MAX_IMAGE_SIZE` back on the wire, so it is the one packet where a client's cost and everybody
    else's are wildly different. It stays out of the spam window — a burst of clicked captions is
    not spam, and warning on it would disconnect ordinary users — and is held to one per
    `IMAGE_REQUEST_DELAY` by `Connection::last_image` instead. It is **served late rather than
    refused**: the client never retries, so a dropped request leaves its caption on `[ loading... ]`
    for the rest of the session. The wait sits on that connection's own read loop, which is the
    backpressure and costs nobody else. `last_image` is carried across a rekey and a channel switch
    — a client that could reset it by switching channels would not be limited at all.
  - **`server_images/` is sealed encrypt-then-MAC, and it is read back in the chunks it was written
    in.** `crypto::image_keys` HKDFs a key, nonce and MAC key per picture out of `server_image_key`
    salted with the hash the file is named after, so nothing about the pair is kept anywhere and two
    pictures never share a keystream. The tag is required for the same reason the history's is: a
    stored picture is decrypted and pushed to every client in the channel, so bare CTR would put
    attacker-flippable bytes into every one of their decoders. `file/server.rs` MACs an upload
    **as it arrives** — `ActiveFileshare::mac` is fed whatever the disk actually took, and
    `crypto::disk_tag` binds the length and appends the tag once the last chunk is in, which is why
    the tag is at the end rather than in front of the ciphertext. A fileshare gets none of this: its
    key is random and dies with the process, so there is no leaked copy of one to authenticate, and
    it is streamed back out chunk by chunk, which a trailing tag could not be checked ahead of anyway.
  - **`read_image` chunks by `UPLOAD_CHUNK_SIZE` because the write did, and this is not optional.** A
    `RexStream` that is `finalize()`d mid-file processes whatever part-grid is buffered as a block of
    its own and moves the counter on, so a reader only stays on the keystream if it breaks the file
    exactly where the upload did. `UPLOAD_CHUNK_SIZE` is a round 1,000,000 and `MEGABYTE` is 10^6, not
    2^20 — so a chunk is 1953.125 grids, not a whole number of them, and a single pass over the file
    decrypts everything past the first boundary to rubbish. This is why every stored image over 1MB
    used to come back undecodable. Either mirror the chunking or make the chunk a multiple of the
    512-byte grid; do not assume one `update` over the whole file is equivalent.
- **`cache.rs`** (feature `client_base`) — the client keeps every picture it has seen, which is what
  makes a replayed image appear at all without asking anybody, and what lets the server stop pushing
  the ones everybody already holds.
  - **`ImageDisplay` carries a hash and an optional payload.** `send_to_all` clones the whole
    `PacketCode` per recipient and every connection has its own keys, so an 8MB picture in a
    20-client channel is 20 clones and 20 independent REX encryptions — there is no shared ciphertext
    to reuse, so the only saving is not sending the bytes N times. The branch is the one the upload
    arm already computes: a picture `config::messages::has_image` does not know is one nobody can
    hold, and goes out whole (`file/server.rs`); one it does know has been posted here before, so it
    goes out as `data: None` and the repost costs the server neither the disk read nor the
    `RexStream` decrypt that used to feed the broadcast. A client that misses falls through to the
    existing `ImageData` fetch and pays what a history caption pays.
  - **The cache is keyed by content and scoped by the server's fingerprint**
    (`misc::get_image_cache_dir`, `options::get_fingerprint`, set at the handshake whether or not
    TOFU pinned it). Scoping is not tidiness: over one shared cache a server could name any hash in a
    caption and watch whether we fetch it, which is an oracle over every picture we have seen
    anywhere. Keying on the identity hash rather than the host also means a server that moves address
    keeps its pictures.
  - **The hash is re-taken over the bytes, never trusted from the packet.** `digest_and_decode` runs
    the SHA-256 and the decode in one `spawn_blocking` (both are CPU over the whole picture) and the
    bytes are an `Arc` so the cache keeps them without a second copy. A fetch answer whose digest is
    not the hash we asked for is displayed but not filed — otherwise a truncated transfer would be
    cached under a good name forever.
  - **It is encrypted and authenticated at rest** the same way the server's own `server_images/` is —
    `crypto::cache_keys` HKDFs a per-picture key, nonce and MAC key out of the one `client_cache_key`,
    salted with `fingerprint || hash`, so two servers' copies of the same picture
    share no keystream and one file discards the lot. Be honest about what the *encryption* buys: the
    key sits beside the ciphertext, so it protects a leaked copy (a backup, a snapshot) and nothing
    that already has the config dir.
    The **tag is not decoration**, and it is what the encryption alone could not do. A cached picture
    is decoded without being re-hashed — that is the whole point of the `fresh` split in the
    `ImageDisplay` arm — so bare CTR would put whatever is in that file straight into an image decoder,
    and `store` refuses to overwrite (the name is the content), so one bad file would poison that hash
    forever. Because the keys come from `fingerprint || hash`, the tag says three things at once: that
    we wrote it, that it is whole, and that it is the picture its name promises — a file copied out of
    another server's scope, or renamed to another hash, fails to verify rather than decrypting to
    somebody else's picture. `cache::load` **deletes a file that does not verify** so the next fetch
    refills it, which is also what makes a half-written file (a crash, a full disk) recoverable rather
    than permanent.
  - **Its only bound is `MAX_IMAGE_CACHE`.** The server's `server_images/` is owned by the history
    and a picture dies with its entry; a client cache is owned by nothing, so a write that takes it
    over the cap drops the oldest files by mtime, and every hit touches the file it read so what goes
    is what has not been looked at.
  - **A replay fills from the cache without a packet.** `App::apply` is pure state mutation and
    cannot do disk I/O, so the `History` arm in `network/client.rs` does it: first `cache::has` (one
    `stat` per picture, no key and no decrypt) to say which hashes we hold, so a caption that is about
    to fill itself comes up as `Picture::Waiting` (`[ loading... ]`) rather than `Absent`
    (`[ show ]`) — offering a button for a picture already on its way is the one thing it must not do,
    and `request_image` would refuse the click anyway. Then it sends the captions and
    walks the hashes in one task, decoding hits **one at a time** — a login is the one place dozens of
    pictures arrive at once, and a task each would be exactly the unbounded fan-out `MAX_IMAGE_ALLOC`
    exists to bound. Each hit arrives as an ordinary `ClientEvent::ImageData`, which is why
    `deliver_image` fills `Picture::Absent` as well as `Waiting`: an answer nobody clicked for is what
    a cache hit *is*. A refusal (`None`) still only marks a line that actually asked.
  - **`auto_show_images` (client.toml, default on) makes a live picture behave like a replayed one.**
    With it off, `network/client.rs`'s `ImageDisplay` arm decodes nothing: the line goes up as a
    caption with a `[ show ]` button (`ClientEvent::ImageOffer` → `push_caption(.., pending: false)`)
    and the history's cache prefetch is skipped, so every replayed caption is a button too. The two
    costs it declines are the ones the pushed path pays without being asked — the decode
    (`MAX_IMAGE_ALLOC` per picture, one per packet, for pictures nobody looked at) and the `ImageData`
    fetch that answers an offer. Bytes that arrived **anyway** are still hashed and cached: they are
    paid for, and dropping them would only mean fetching them back on the click.
    That is also why the click goes through the cache first (`client::fetch_image`, replacing the
    unconditional `ImageData` request in `tui/mod.rs`'s mouse-up arm) — with this setting off, the
    picture a click asks for is usually already on disk, and asking the server for it would cost a
    disk read, a whole `RexStream` decrypt and `MAX_IMAGE_SIZE` back on the wire for nothing. A miss
    comes back as `ClientEvent::ImageRequest` and joins `App::image_requests`, which the redraw tick
    sends — the event loop owns the write half and the sequence counter, so the fetch task cannot.
- **`network/client.rs` / `network/server.rs`** — connection-level logic (handshake, auth, message
  dispatch) for each side. `network/file`, `network/screen`, `network/voice` are protocol
  extensions with their own client/server submodules for file transfer, screen sharing (feature
  `client_screen`/`server`), and voice chat (`client_voice`/`server`) respectively — voice runs
  over UDP while text runs over TCP, on the same port.
- **`network/screen/client/capture.rs`** — the capture pipeline, and the only blocking CPU loop in
  the client (`spawn_blocking`). It is **backend-selected at runtime, never assumed**:
  - `capture_loop` prefers the OS-native streaming recorder (`xcap::Monitor::video_recorder()` —
    DXGI Desktop Duplication on Windows, `AVCaptureScreenInput` on macOS, xdg-desktop-portal +
    PipeWire on Wayland, a polling thread on X11) over the legacy polling path
    (`capture_loop_xcap` / `capture_loop_wayshot`), but it does not *wait* for it.
  - **The probe never gates the share.** The polling path starts immediately and the probe runs
    beside it on its own thread; the recorder takes over only once it has proven itself, via the
    `UPGRADING` flag that the polling loops watch in their `while` condition. Probing first was
    tried and is exactly wrong: on Wayland the probe is an xdg-desktop-portal request that blocks
    until somebody notices the picker, so a viewer attaching in that window sat in front of a
    black rectangle for tens of seconds while a working backend went unused. Handing over costs
    one keyframe mid-share; gating cost the entire opening of it. `UPGRADING` is deliberately not
    `running` — standing the polling path down is not ending the share.
  - **The probe still demands an actual frame**, because a recorder that *starts* is not a
    recorder that *works*: xcap's X11 recorder reports success and then delivers nothing, which
    without `RECORDER_FIRST_FRAME` would be a permanently blank share that no fallback could
    rescue, since nothing would have failed. `RECORDER_PROBE_TIMEOUT` now only applies where the
    polling path could not start at all and the recorder is the last backend left rather than an
    upgrade — that is the one case worth blocking for.
  - **The wayshot path recycles its Wayland connection on a memory budget**, and this is not
    optional tidiness. `libwayshot` binds a fresh `wl_shm` per capture and never releases it, so
    the compositor holds one full-screen buffer for every frame taken — measured against Hyprland
    at ~5.5 MB a frame, which is ~10 GB a minute at 30 fps and takes the whole machine down with
    it inside about a minute. The share does not leak it back: all of it is returned the moment
    the *client disconnects*, and `WayshotConnection::new()` costs 0.4 ms, so
    `capture_loop_wayshot` counts the bytes it has stranded and reconnects once they pass
    `WAYLAND_LEAK_BUDGET`. Sizing by bytes rather than by a frame count is deliberate — a 4K share
    strands memory four times faster than a 1080p one and has to recycle four times as often.
    Unlike the failure-driven reconnect beside it, this one forces no keyframe and clears no
    `last_image`: nothing was missed and the picture has not moved.
  - **Which monitor is shared is a client-local choice**, not part of the protocol: `/screen [MONITOR]`
    (a 1-based index or a monitor name) stores it in `screen::client::options::set_monitor` and
    `capture::get_target_monitor` resolves it; the `Screen` packet still only toggles the share, and the
    palette offers the monitor names (`ArgValues::Monitors` → `capture::monitor_names`, cached for
    `MONITOR_LIST_TTL` because the popup asks on every keystroke). `command.rs` resolves the parameter
    to a monitor *name* through `capture::resolve_monitor` before storing it, so an unknown monitor is
    invalid usage on the spot rather than a share that starts and dies, and so `/screen 2` and
    `/screen DP-2` are recognised as the same monitor. **The pick lasts exactly as long as the share
    does** — it lives only in that atomic-style global, and every path that ends a share puts it back to
    `None` (the `Screen { token: None }` arm in `network/client.rs`, `state::reset_session` for a lost
    session), so a bare `/screen` always starts on the default monitor.
  - **`/screen MONITOR` while a share is running swaps the capture over instead of ending it.** The
    server only ever knows *that* we are sharing, so nothing is sent: `set_monitor` bumps
    `MONITOR_GENERATION`, every capture loop watches it (`capture::switched`) and stands down, and
    `capture_loop` — a restart loop around `capture_backend` — opens the new monitor while `running`,
    the socket, the token and the audio capture all survive. The viewer pays one keyframe (the encoder
    is new) and nothing else. Naming the monitor already being captured, or passing none, is still the
    plain toggle: asking for what is already on the wire is the one case where a swap would mean nothing —
    and that path deliberately leaves the pick alone, since swapping to the monitor we are about to stop
    capturing would only restart the capture on its way out.
    `build_message` returns `None` for a swap, which is why `mod.rs::submit` needs a `Command::Screen`
    arm — its default arm panics on a command it does not know how to handle locally.
  - On Wayland a picked monitor also **pins the polling path**: the recorder there is an
    xdg-desktop-portal request whose picker chooses the output itself, so upgrading to it would throw
    the selection away and ask again.
  - `WHY2_CAPTURE_BACKEND` (`recorder` / `legacy`) pins a backend; `WHY2_CAPTURE_PROBE_TIMEOUT`
    overrides the probe deadline in seconds. Both exist so a machine where the heuristic picks
    wrong is one env var away from the old behaviour.
- **The share's latency is bounded by shedding, not by buffering, and every queue on the path has to
  agree with that.** The pipeline already drops rather than waits where it matters —
  `FrameEncoder::dispatch` tail-drops a frame the network channel cannot hold and forces an IDR so
  the next one stands alone — but that only fires once a send actually blocks, and the two places it
  could not fire were what a full-motion share (a video, not a desktop) turned into seconds of delay:
  - **The kernel send queue hid the backlog.** Linux autotunes `tcp_wmem` to 4 MB, so at
    `H264_BITRATE` the socket swallows megabytes before `write_all` ever stalls, and every one of
    those bytes is standing latency — about two seconds of it on a link that cannot carry the share.
    Nothing downstream can shed it either: it is bytes in a stream, not frames in a queue.
    `screen::cap_socket_buffers` caps `SO_SNDBUF`/`SO_RCVBUF` at `SOCKET_BUFFER` on all four ends of
    a share (the sharer's upload, the server's two, the viewer's download), which is what turns "the
    link is full" back into something the encoder can feel while the backlog is still one frame old.
    The size is the trade: the standing queue costs roughly one buffer per hop at the share's
    bitrate, while a receive buffer also pins the window, so sizing it much smaller would cap
    throughput on a high-latency path instead of the latency.
  - **The viewer's audio queue ratcheted and never came back.** Its consumer is a sound card and its
    producer is the sharer's, so the two run at the same rate forever: whatever depth one bad moment
    on the link pushed in stayed in. And because video and audio share one TCP stream, that depth was
    latency on the *picture* too — `spawn_audio_playback` blocking on a full channel held the reader,
    closed the receive window and backed the whole share up. So the reader `try_send`s (20 ms of
    sound is the cheaper loss) and the playback task throws the backlog away rather than playing
    through it, decoding only the newest frame. `AUDIO_BACKLOG_TARGET` is deliberately not zero: the
    queue is also the jitter buffer, and draining it flat would trade the latency for a gap on every
    late packet — the channel's own bound is the ceiling this replaces, not the depth it should sit at.
  - **Known gap: the bitrate does not adapt.** A chronically saturated link now sheds frames instead
    of queueing them, which is right, but the honest fix is for the encoder to lower its rate rather
    than for `dispatch` to drop and force an IDR — an IDR is several times a P-frame, so a link that
    is only just too slow pays for the drop twice. That needs a feedback signal the protocol does not
    carry yet.
  - **A slow viewer is shed on its own socket, not paid for by everybody else.** `screen::server`'s
    loop used to `send_frame` to each viewer inline, so the share ran at the slowest link on the
    server: one viewer stalling in `write_all` held the read of the sharer's *next* frame, and every
    other viewer waited behind it. Each attachment now owns a task and a `VIEWER_CHANNEL_BOUND`
    queue (`spawn_viewer`), and the `Viewer` entry carries the `RexPacketStream` and sequence
    counter into it — those are per viewer already, so nothing is shared across the split. The share
    loop `try_send`s and never awaits a viewer, so the rate is the sharer's.
    - **A shed viewer waits for an IDR rather than being handed a broken chain.** The server has no
      encoder and no way to ask the sharer for a keyframe, so a dropped frame cannot be made good —
      it can only be *not compounded*: `needs_key` holds that viewer's last picture and skips video
      until `is_keyframe` sees a NAL the decoder can stand up on its own (type 5, or the SPS the
      encoder repeats in front of one), which the encoder's `intra_frame_period` guarantees within
      `FORCED_INTRA_INTERVAL`. Forwarding the P-frames instead would put frames on the wire whose
      references that viewer never received, and its decoder drops those anyway (`display.rs`) —
      the freeze is the same picture without the bandwidth.
      Audio is shed by itself and needs none of this: a 20 ms frame is self-contained, so the queue
      being full costs exactly that frame.
    - **An attach opens on the share's last keyframe rather than on black.** A viewer used to be
      built by the share loop, on the next frame to arrive, and then had to wait for the IDR after
      that before anything was decodable — so attaching cost up to `FORCED_INTRA_INTERVAL` of black
      on a busy desktop, and twice that on a still one, which reads as a share that is broken rather
      than one that is starting. `SHARES` (a `DashMap` keyed by sharer id, put up by the share loop
      and taken down by `ScreenTransferGuard`) carries two things the accept loop can reach: the last
      access unit `is_keyframe` accepted, and the viewers built since the loop last looked.
      `screen::server::attach` — called from `bin/server.rs`'s `ConnectionType::Attach` arm, where
      the socket actually arrives — builds the `Viewer` there, pushes that cached keyframe into its
      queue, and leaves it in `pending` for the share loop to adopt on its next frame. **Building it
      at the attach is half the fix**: a still desktop sends one frame every `FORCED_INTRA_INTERVAL`,
      so a viewer that only exists once one arrives cannot be shown anything before then, cache or no
      cache. Nothing new crosses the wire and the sharer is not asked for anything — the server has no
      encoder and no way to request a keyframe, which is the same constraint `needs_key` lives under.
      The cached picture is up to `FORCED_INTRA_INTERVAL` stale and the frames since it went to
      somebody else, so a new viewer starts `needs_key` **like a shed one** and snaps to live on the
      next IDR — that is also why a fresh `Viewer` is `needs_key: true` rather than `false`, which
      used to put P-frames on the wire for a viewer with no reference picture to decode them against.
      The keyframe is cached *after* the forward, so a viewer adopted this iteration is not handed the
      frame it is about to be sent. A muted sharer's placeholder is all IDRs, so the cache fills with
      those too and an attach to a muted share opens on the placeholder.
    - Dropping a `Viewer` **aborts** its task rather than closing the channel: the task it is
      standing down is by definition one that may be parked in `write_all` on a socket that will
      never drain, and it would not reach the next `recv` to notice. That socket is being discarded
      either way — the viewer detached, or re-attached under a new token, which is a new stream. The
      loop's `retain` therefore matches on the *token* as well as the id: a re-attachment is a
      different `Viewer` for the same client, and the new one arrives through `pending`.
    - **Known gap: the sharer is no longer told when a viewer cannot keep up.** Forwarding inline at
      least backpressured them; now nothing does, so a share sized for a link nobody has is simply
      shed at the server, per viewer, forever. This is the same missing feedback signal the bitrate
      gap above needs.
- **`network/screen/client/gpu.rs` + `rgba_to_i420.wgsl`** — RGBA → I420 on the GPU via a `wgpu`
  compute shader. This is not decoration: measured on the capture pipeline, the colour conversion
  was **the single most expensive stage, larger than acquisition and the H.264 encode together**
  (~17 ms/frame at 1600x900), because openh264 implements `RGB8Source` only for packed 24-bit RGB
  — an RGBA screen grab falls into the per-pixel scalar `write_yuv_by_pixel`. The shader cuts that
  to ~1.4 ms and roughly halves whole-process CPU.
  - The shader reproduces openh264's **own** BT.601 limited-range integer coefficients so the
    stream's colours do not shift with the backend. The two agree to within 1 LSB of luma and 2 of
    chroma (the CPU path is float and averages chroma without rounding) — hence the test asserts a
    tolerance, not equality.
  - It packs four samples per `u32` in both planes, so it requires `width % 8 == 0 && height % 2
    == 0`; `GpuConverter::supports` guards that and anything else uses the CPU path.
  - **Every failure degrades rather than breaks**: no adapter, a rejected shader, an unsupported
    resolution or a mid-session device loss all switch `Converter` to the CPU permanently and keep
    the share alive. `WHY2_CAPTURE_CONVERTER=cpu` pins the CPU path.
- **`network/screen/client/video.rs` + `yuv_to_rgba.wgsl`** — the viewer half, a `wgpu` surface
  that replaced `pixels` (which is no longer a dependency). The decoder's Y/U/V planes are uploaded
  as three `R8Unorm` textures — **1.5 bytes per pixel instead of the 4 the old RGBA path pushed**,
  with no CPU `write_rgba8` pass at all — and the fragment shader does the BT.601 conversion, the
  chroma upscale and the scale-to-window in one draw.
  - Planes are allocated at the decoder's **stride**, not its width, and the shader trims the
    padding. The span therefore reaches the *centre* of the last real texel: mapping `u = 1` to
    `width / stride` lands on the texel boundary, where a linear sampler mixes in half a texel of
    padding — a visible smear down the right-hand column. `row_padding_never_reaches_the_picture`
    is a regression test for exactly that, and it caught it once already.
  - **The picture is never written through an sRGB view.** The fragment shader emits
    display-referred sRGB already — BT.601 output is gamma-encoded video, not linear light — so an
    sRGB surface format encodes it a *second* time on write. That is not a subtle shift: mid grey
    lands on 188 instead of 128 and dark grey on 124 instead of 51, lifting every shadow while the
    primaries stay put, which reads as a washed-out grey picture. `present_format` strips the
    suffix and `render` creates the swapchain view with it (declared in `view_formats` when the
    surface itself is sRGB). The headless colour tests cannot catch this — they render to
    `Rgba8Unorm`, non-sRGB by construction, so they passed while a real window was visibly wrong;
    `the_picture_is_never_written_through_an_srgb_view` is the check that does.
  - The viewer letterboxes; the old `ScalingMode::Fill` silently distorted any share whose aspect
    did not match the window.
  - `YuvRenderer` knows nothing about windows, so the conversion is rendered offscreen and checked
    without a display — that is how the colour tests run headless in CI.
- **The capture gate's noise floor tracks down fast and up slowly, and a frame it would open on never
  feeds it at all** (`voice/client/mod.rs`'s VAD). The floor is an EMA over the frames the gate is
  *closed* for, which is the right window — while somebody is speaking the gate is open and the floor
  is frozen — but it says nothing about the frames *before* the gate has ever opened. `build_input_stream`
  is called when a client joins voice, so the floor starts at `INITIAL_NOISE_FLOOR`, a guess; somebody
  who joins mid-sentence is speaking into a closed gate, and a symmetric EMA learned that voice as the
  noise floor. `NOISE_OPEN_MULT` then put the open threshold above the speech that had just taught it,
  and it stayed there for as long as they kept talking — the gate opened only once they stopped (the
  floor decaying back to the room) and started again, which is exactly what it looked like from the
  other end. So a rise is `NOISE_FLOOR_RISE` (an order of magnitude slower than the fall) and only from
  a frame under the current open threshold: the pauses between syllables are enough to pull the floor
  down to the room within a few hundred ms while the speech itself can no longer push it up. Genuine
  noise — a fan starting — is still tracked, a few seconds later rather than half a second later, which
  is the trade. It costs nothing on the settled case: once the gate has opened, the floor was already
  frozen for the whole of it.

- **`network/voice/client/aec.rs`** — keeps WHY2's own playback out of the shared screen audio. The
  share captures the output sink's monitor (or the WASAPI loopback), which is the *finished* mix, so
  the voice channel is in it and a viewer who is also in that channel hears themselves come back a
  second later. Nothing in PulseAudio or cpal can leave one application out of a monitor, so the
  client subtracts itself instead.
  - **This is not acoustic echo cancellation, and the difference is what makes it tractable.** There
    is no microphone and no room: the monitor is the digital mix, so our contribution appears in it
    as literally the samples we wrote, offset by the sink's buffer and scaled by the per-stream
    volume — a fixed delay plus a scalar. The voice output callback is the only producer
    (`push_reference`, tapped *after* the output gain and the soft clip, so it is exactly what the
    sink received) and the screen capture owns the only consumer, in `screen/client/audio.rs`, which
    cancels each chunk before Opus. The tap is installed by `start` and costs one atomic load per
    callback while nobody is sharing.
  - **The delay search must not demand a strong correlation.** Whatever is being shared is in the
    capture too and is routinely the louder half — a video playing over a quiet voice channel drags
    the correlation at the *correct* lag down to 0.1 or below, so any fixed floor either rejects the
    right answer or accepts every wrong one. What separates them is not the peak's height but how
    far it stands above the other lags, which are uncorrelated and scatter around zero with a known
    spread; the search accepts a peak only at `AEC_PEAK_SIGMA` above that. An earlier two-pass
    version correlated block-energy *envelopes* first and refined the best few — it was cheaper, but
    the envelope of a loud share swamps the echo's, and it failed exactly when it was needed. (The
    decimation below is not that: it correlates the waveform throughout, only at a coarser rate.)
  - **The search is coarse-to-fine because its cost lands on the capture task**, and that is what a
    share's opening used to leak. At full rate it is `search_range` × `window` multiply-adds — 59
    million at the shipped settings, measured at 30 ms — run inline between socket awaits, while the
    capture callback drops a chunk whenever `chunk_tx` (~160 ms) is full. So every attempt punched a
    hole in the very audio the next attempt would correlate, and the interval between attempts had to
    be half a second to afford it: a share could spend seconds unlocked, passing raw echo through for
    all of it, which is what a viewer attaching heard. A peak is only being *located*, though, and a
    delay does not need sample resolution to be located: `decimate` box-filters both sides down by
    `AEC_SEARCH_DECIMATION`, which costs its square and lands within one decimated sample, and a
    full-rate pass across that one sample resolves the lag exactly — so what the filter is handed is
    unchanged. Measured against the full search over speech-like noise under an interfering share,
    the two pick the same lag and the peak's sigma agrees to within 4%, for 2 ms instead of 30.
    Decimating by 8 is cheaper still and was rejected: sigma is 10% down there, and sigma *is* the
    decision — that would buy the speed by refusing marginal locks. The cheap search is also what
    pays for `AEC_SEARCH_INTERVAL` dropping to 100 ms, which is the rest of the opening: the gap
    between attempts is the worst case a lock can be late by once somebody speaks.
  - **The filter only learns from audio our own echo is actually a part of** (`AEC_ADAPT_RATIO`), and
    this is what makes it survive anything else being played. Whatever is being shared is in the capture
    and is in the reference not at all, so it reaches the adaptation as a disturbance in the error that
    no step size averages away — and NLMS divides the update by the *reference's* energy, so a quiet
    voice channel under a loud video scales pure noise **up** and walks the weights off the echo. The
    ERLE check then reset them, the search re-locked, and it went round again: the cancellation coming
    and going with nothing to show for it, working on a quiet channel and failing the moment a video
    started, was that loop and not a tuning problem. There is nothing to track while the evidence is
    bad, though — this echo path is a fixed delay and a scalar, not a room — so `process` compares the
    echo it expects (`gain²` × the tap window's mean square) against the capture's, and **scales the
    NLMS step by that fraction**. The prediction deliberately comes from the *search's* gain rather than
    from the estimate the filter just produced: a diverging filter emits more, so judging it by its own
    output would open the gate wider the worse it got.
    A hard gate on the same fraction was the first cut and it works — it is what turned "they hear
    themselves whenever a video plays" into "a little, sometimes" — but it is backwards in both
    directions: it throws away every sample below the line and spends the full step on every sample
    above it. Misadjustment goes as the step times the disturbance-to-echo ratio, so scaling by it
    instead holds the damage *constant* at any share loudness, and lets the filter keep creeping
    forward under a video rather than stopping dead and waiting for silence.
  - **The ERLE check is what `AEC_ADAPT_RATIO` still gates**, because it is the only way that number
    means anything. Measured across a loud share, perfect cancellation and no cancellation at all both
    come out at 0 dB — the echo is a rounding error in the total either way — so two windows are not
    comparable unless there was something of ours to remove in both.
  - **A filter that scores badly is put back, not thrown away** (`best`, `AEC_ROLLBACK_MARGIN`). A reset
    is far more expensive than it looks: it passes the capture through untouched — raw echo — for as
    long as the search needs, and comes back with the single tap the search seeds, which is where the
    filter started. While a share is playing it cannot climb back out of that, so the lock churn *was*
    the residual echo: a little of it for as long as the video ran, then gone once the filter could
    converge again. So each scoring window is compared against the best that lock has managed, the
    weights behind that best are kept, and a window `AEC_ROLLBACK_MARGIN` dB worse (or one that is
    adding energy outright) restores them instead of giving up the delay.
    **Only a filter that is adding energy counts as a failure**, though, and this is the whole
    difference between the rollback helping and it making things worse. ERLE swings with what is being
    said as much as with the filter, so scoring under an earlier peak is ordinary; counting those
    towards a reset made the lock *more* fragile than the plain check it replaced — three unremarkable
    windows below one good one and the share went back to raw echo while the search ran, which showed
    up as the echo returning on a **quiet** channel rather than only under a video. The standard also
    forgets `AEC_ROLLBACK_DECAY` dB every window, so a best taken under conditions that no longer hold
    cannot sit there rejecting perfectly good filters. Only `AEC_ROLLBACK_LIMIT` energy-adding windows
    in a row mean the delay itself is wrong rather than the filter, and only then is there a reset.
  - **The NLMS step is deliberately tiny** (`AEC_STEP`). The search hands the filter a least-squares
    gain at the right lag, so it only has to track drift, while the shared audio sits in the error
    signal as a loud disturbance that a large step turns into weight jitter. Raising it makes things
    worse, and measurably: against a share twice as loud as the echo, 0.002 removed 21 dB, 0.0005
    removed 30 dB and the 0.0001 it settled on removed 34 dB. Cancellation degrades gracefully from
    there as the share gets louder relative to the voice — 40 dB at parity down to 16 dB at eighteen
    times it — while damage to the shared audio stays flat at about -44 dB. Those came off a
    synthetic loopback harness (a known delay and gain) that was development scaffolding and is not
    in the tree; anything re-tuning these has to bring its own.
  - **Reference and capture are aligned by count — one reference sample per captured frame — so a gap in
    only one of them shifts every later sample.** The capture callback drops a chunk whenever
    `chunk_tx` is full, which is routine: the capture task blocks on `tx.send().await` to the network,
    so chunks are dropped exactly when the link is struggling — which is exactly when the echo is worst.
    `aec::skip_reference` is how the callback reports it, and `process` drops the same number of frames
    out of the reference so the lock survives instead of being re-found a second later. A dropped chunk
    is not a `DESYNC`; a *lost reference sample* (a full ring in `push_reference`) still is, because
    there is no way to know where in the stream it went missing.
  - **The reference ring running dry is drift, not silence, and it is tracked rather than reset on.** The
    voice output callback pushes every frame it writes, silent ones included, so an empty ring means our
    consumer has run past their producer — the two devices' clocks differ. The zero still goes into the
    delay line (there is nothing else to put there), but `next_reference` counts it, and the `Locked` arm
    slides `offset` back by the same amount: every real sample behind a phantom sits one place closer to
    the newest end, so the whole tap window follows it and every weight stays on the sample it was fitted
    to. Without this, an undetected trickle of one-sample shifts walks the filter off its own echo and the
    cancellation comes and goes on no schedule at all — which is exactly what it did. Running out of lead
    to slide into is the one case that still resets.
  - **A reset does not drain the reference ring.** The alignment is what it throws away, and the search
    derives that again from wherever the two sides sit — the audio itself is still perfectly good. Only a
    **rate change** drains it, the one event that makes those samples wrong rather than merely unaligned.
  - **Every failure degrades instead of breaking.** No voice session means an empty ring, which
    reads as silence and subtracts nothing; a voice output device that is not the monitored sink
    leaves our audio out of the capture entirely and the filter converges to zero on its own; and
    while the delay is unknown, or the running ERLE check finds the filter adding energy rather than
    removing it, the capture is passed through untouched rather than damaged.
  - **Known gap:** the reference is the voice output stream only. `screen::client::audio`'s own
    playback — what you hear while attached to somebody else's share — is a separate cpal stream and
    is not in it, so sharing while attached still leaks that share's audio into yours.
- **`crypto/kex.rs`** — hybrid key exchange: classical ECC (`p521`) + post-quantum ML-KEM,
  combined via HKDF (`crypto/mod.rs::get_correct_key`, `derive_stream_nonce`) to derive the actual
  `why2` grid key/nonce and HMAC key (`SharedKeys = (why2 key, HMAC key)`) from the raw shared
  secret. `crypto/password.rs` (feature `server`) handles Argon2 password hashing.
  Rekeying happens periodically (`consts::REKEY_INTERVAL`, 10 minutes) to bound the damage from any
  single session key.
  TOFU (trust-on-first-use) server key pinning is expected; `WHY2_SKIP_TOFU` env var (baked in at
  build time via `build.rs`) disables that check for local/dev testing only.
- **`role.rs`** — `Role`, the server rank (`User`/`Moderator`/`Owner`). **The ordering is the permission
  check**: every gate is `role >= Role::Something`, so the variants are declared lowest-first and derive
  `Ord` from that order. The variants, the names they are typed/stored/shown by and the list the palette
  offers are generated together by the `roles!` macro at the top of the file — a new rank is one line in
  that list and nothing else, and the three cannot drift apart. It crosses the wire as itself
  (`Accept`, `ServerRole`), so a role that does not exist is not a value the protocol can carry.
  A rank is its name everywhere it is read or written — typed into `/server role`, and stored in
  `server_users.toml` — so there is one spelling of it and nothing to convert between.
  Ranks are handed out with `/server role <id> <role>` (owner only; the server refuses granting above
  your own rank, retitling yourself, or touching a peer or superior). A granted role applies to the
  session it lands in — the server updates the live `Connection` and tells that client, whose
  `App::role` is what the palette and `/help` read — so the per-connection role is re-read on every
  packet rather than latched at login.
- **The chat colors are the server's, and the client neither stores nor sends them.** `/color` and `/ucolor`
  are unchanged from where the user stands, but `color_handler` only asks: it sends
  `PacketCode::Colors { username, color }` — which of the two, and the code — and the server stores it under
  the user's entry in `server_users.toml` (`config::users::set_color`). The packet coming back is the whole
  answer, since there is nothing for the client to keep; it is what prints "Color set successfully.".
  - **A message is painted on the way out.** `listen_client`'s `Message` arm drops whatever colors the
    packet arrived with and fills in `config::users::colors(&username)` — a sender no more names its own
    colour than it names its own id or username, all three of which the server has always filled in. A
    client sends `MessageColors` empty. The same lookup is what an image line's `username_color` now comes
    from (the `Upload`/`Image` arm), so `PacketCode::Image` no longer carries one at all; the colour is
    taken at the request and rides the token to the upload socket as it did before.
  - That is what makes the colour **global across devices** without a second copy anywhere: `client.toml`
    has no colour keys (only `disable_colors`, which is a local preference), and there is no session value
    for them either — the client never learns its own colours, because nothing it draws needs them. Every
    line in the pane, its own included, is painted from the colors the *server* put on that packet.
  - **A code is stored as its name.** `colors::COLORS` (the 16 names, in wire order) moved out of the
    client binary into the library for this: the wire carries a position in that list, but a file an
    operator opens should say `username_color = "red"`, and a name survives a reordering of the list that a
    position would silently repaint. `colors::name` is also the validation — a code off the wire is a
    client's word, and anything outside the table stores as `"none"` rather than being refused. The
    client's own `colors.rs` is left with the crossterm half alone, deriving each `Color` from the name via
    `Color::try_from` (every one of the 16 is a name crossterm parses, which is what made the pair table it
    used to hold redundant) and re-exporting `colors::code` so a typed name is resolved in one place.
    `to_color` goes straight through that lookup: `ansi_(n)` and `rgb_(r,g,b)` are colours a code cannot
    carry, and are refused where they are typed rather than accepted and then ignored on every message.
  - **`config::users::migrate()`** (called from `bin/server.rs` right after the logger, and marked in the
    code to go with the next version bump) writes the two keys into entries that predate them, in one pass
    over the document. `colors()` would read a missing key as no
    colour anyway — the point is that every entry has the same shape and the file states what is settable.
    A legacy *flat* entry is left alone; `write_user_field` turns one into a subtable the first time
    anything is stored for it.
- **`config/mod.rs`** — TOML config for client (`client.toml`) and server (`server.toml`), plus
  server user store (`server_users.toml`), server ban list (`server_bans.toml`) and server keypair
  storage (`server_keys/{private,public}`), all under `WHY2_CONFIG_DIR`
  (defaults to `~/.config/WHY2`, baked in by `build.rs` unless overridden at build time).
  **The at-rest keys live in the config root, not in `server_keys/`** (`server_history_key`,
  `server_image_key`, `client_cache_key`, created on first use by `kex::media_key`). That directory is the server's
  *identity*, and none of them are derived from it — that is the point of them — while a client has no
  identity there at all and never creates the directory. The root is where both binaries' files
  already sit side by side, distinguished by an owner prefix. The file is 0600
  wherever it lands, which is what protects the bytes; the root is not 0700 like `server_keys/` is,
  so the name is visible to other local users and the content is not.
- **`config/messages.rs`** (feature `server`) — the lobby's message history, off by default
  (`persistent_messages`), kept as the last `max_persistent_messages` messages. **It is the one
  thing under the config dir that is not TOML**: `server_messages.bin` is a `wincode`-encoded
  `Vec<Record>`, the same encoding the packets use, so a message is stored the way it is sent instead
  of being flattened into text.
  - **`Record` is not `StoredMessage`, and the difference is the colors.** What is kept is who said
    it, what they said and the picture if it was one; the colors are looked up per sender in `all()`
    and put on the wire `StoredMessage` there. So a replay is painted the way the sender looks **now**
    — somebody who recolours themselves recolours everything they ever said, which is what the colors
    living on the account rather than in the packet means once there is a file involved. It also means
    the file cannot hold a colour that no longer exists, `/color` never has to rewrite a history, and an
    existing history can be carried across the format change by dropping a field rather than a file.
    The lookup is per sender, not per line: a colour is a couple of reads of `server_users.toml`, and a
    history is routinely a handful of people saying `max_persistent_messages` things.
  - **It is encrypted at rest, authenticated, under a key nobody has to manage.**
    `crypto::history_keys` HKDFs the `why2` grid key and the HMAC key out of `kex::history_key` — 32
    random bytes in `server_keys/history_key`, written the first time the history is touched — and
    `store`/`load` go through `crypto::encrypt_packet`/`decrypt_packet`, the same `AuthenticatedData`
    encrypt-then-MAC the one-shot packets use.
    **The key is the history's own and is deliberately not derived from the server identity.** It was
    once HKDF'd from the ECC and ML-KEM private keys, which bought no strength — everything lives in
    the same directory either way — and cost two things: the history could not outlive a rotation of
    the identity, and a static ML-KEM pair had to stay on disk for it alone after the handshake moved
    to ephemeral keys. A key of its own is also the way to *discard* the history: delete the file and
    the ciphertext beside it is scrap, which is what `history_key` does when it finds no key or a
    truncated one. Baking a key in at build time was the other alternative and is wrong twice over —
    it ships inside the binary, so every operator running the same artifact shares one key, and it
    changes on rebuild, so an upgrade silently drops the history.
    Be honest about what this buys: the key sits next to the ciphertext, so it protects a *leaked
    copy* of the file (a backup, a snapshot, a support bundle) and nothing that already has the
    config dir. Authentication is not optional decoration either — the history is replayed straight
    into every client's chat pane, so bare CTR would put attacker-flippable bytes on that path.
    The nonce is fresh on **every** write, which `encrypt_packet` gives for free: the file is
    replaced rather than appended to, so one nonce reused across rewrites of a growing buffer is
    textbook keystream reuse.
  - **The in-memory `HISTORY` is the working set**, and the file is the copy of it that survives a
    restart: it is read once, on first touch, and only ever written after that. A missing, truncated,
    tampered, unrecognisable file, or one written under another server's keys, all load as an empty
    history rather than refusing to start.
    The one older format that is *not* thrown away is the one with the colors still in the record
    (`migrate`, marked in the code to go with the next version bump): it is read as the shape it is and
    the colors dropped, since `all()` puts them back on every line from the account anyway. It converts
    **in memory only** — the next message rewrites the file, and until one arrives a restart simply
    costs the same read again, which is cheaper than a rewrite on a path that has not been asked to
    write anything yet.
  - **Only the lobby has one.** A channel exists exactly as long as somebody is in it, so there is
    nothing to keep it against; `server::listen_client`'s `Message` arm stores only while
    `channel.is_none()`.
  - The append-trim-write runs under the one `HISTORY` lock, so two clients talking at once cannot
    drop each other's message or leave a half-written file behind.
  - The history is sent once, as `PacketCode::History`, immediately after `Accept` (`send_history`)
    — every client starts in the lobby, so a channel switch has nothing to replay and asks for
    nothing. The packet keeps the username, the text and the colors, but **no id**: the session
    that said it is gone and whoever holds that id now is somebody else. The client replays it as
    `state::Entry::History` — an ordinary chat line rendered through `Theme::render` (so
    `disable_colors` reaches it like any other message) minus the id column, under a
    `Message history (n):` heading that is what separates it from what is being said now.
  - **An image line carries the sender's username color, the way a message does.** It is the username
    color *only*: an image line's text is the filename, which is the client's own wording and not
    something the sender typed, so there is no message color to keep and the packet's stays `None`.
    It is looked up where the line is built — `all()` for a replay, and
    `config::users::colors(&username).username_color` beside each `PacketCode::ImageDisplay` for the
    clients watching live (`file/server.rs` after an upload, the `Upload`/`Image` arm for a picture the
    history already holds) — so a picture looks the same before and after a restart without anything
    carrying a colour for it. Nothing on the upload path does: `PacketCode::Image` has no colour field
    and neither does `ConnectionType::Image`, since the colour is a lookup at the point of use and a
    token is not the place to park one. A fileshare puts up no line and has none, which is why
    `persistent` is all `download` needs to tell the two apart.
    It reaches a consumer of the crate through the events, not only through the packet: a library user
    only ever sees `ClientEvent`, so every one of the four a live picture can put a line up with carries
    it — `ImageDisplay`, `ImagePending` (the caption a cache miss puts up before the fetch),
    `ImageOffer` (the same line with the button, `auto_show_images` off) and `ImageFailed`. `ImageData`
    deliberately does not: it is keyed by hash and only fills a caption one of those already created.
    The TUI reads it too — `Entry::Image` keeps it and `Theme::render` names the sender in their own
    color, falling back to the chrome's accent when they have none, so a caption and the messages
    around it agree on who is talking.
- **`bin/client/`** — the client entrypoint (`mod.rs`), the full-screen TUI (`tui/`, ratatui over
  the crossterm backend), and color handling (`colors.rs`).

  **The TUI's tuning knobs all live in `tui/consts.rs`**, the way the library's do in `consts.rs` and
  the two protocol extensions' do in `network/{screen,voice}/consts.rs` — pane and layout sizes,
  the redraw interval, popup row counts, the button labels, the markup/math limits. A new one goes
  there rather than at the top of the file that reads it: half of them are read by `draw.rs` as well
  as by the module that owns the behaviour, and as file-local consts they were being reached for
  across modules (`palette::MAX_ROWS`, `tofu::CHALLENGE`) which is a const module with extra steps.
  What deliberately stays out is anything that is not a knob: `theme.rs`'s palette, `math.rs`'s
  symbol tables, `command.rs`'s command list, and the `include_str!`s (`draw.rs`'s logo, the two
  `.wgsl` shaders, `gpu.rs`'s `WORKGROUP`, which must match a literal in the shader beside it).

  **The client renders through one event loop — there is no printing anywhere else.**
  `tui::run` (`tui/mod.rs`) is a single `tokio::select!` over four sources: the
  `crossterm::EventStream` (keys, resize, mouse wheel), the `mpsc::Receiver<ClientEvent>`, the
  finished dial attempts of the connect prompt (`login::ConnectResult`), and a 33 ms redraw tick.
  `ClientEvent` handling (`App::apply`, `tui/event.rs`) is **pure state
  mutation** — it appends `Line`s to `App::messages`, updates the sidebar/voice roster or sets
  `should_quit`, and never touches the terminal. The tick sets nothing and draws only when
  `App::dirty` is set, which is what keeps `VoiceActivity` (one event per voice packet) from
  repainting hundreds of times a second. Consequences for new code:
  - Never `println!`/`print!` after the TUI is entered. Anything with something to say sends a
    `ClientEvent` (from the network layer) or calls `App::push*` (from a locally handled command in
    `mod.rs::submit`). There is no pre-TUI phase left: `run_client` enters the alternate screen
    before anything else happens, and the only write left to the normal screen is `App::quit_message`,
    printed after the guard is dropped. The version check reports through `ClientEvent` like everything
    else, and runs in a task so it cannot hold up the first frame.
  - **Getting in happens inside the TUI** (`tui/login.rs`, `draw::draw_login`). `App::login` is `Some`
    from the first frame until the server accepts us (and again the moment a session is lost), and it
    is one box asking three times
    (`login::Stage`: `Address`, `Username`, `Password { register }`) — the address, the username and
    the password are the same field, relabelled. While it is up it owns the keyboard (behind only the
    TOFU prompt, which is the one thing that may still be answered over it), and **the input bar and
    the sidebar are not drawn at all** — there is nothing to type into or list yet, so `draw::draw`
    gives the input row zero height.
    Only the address step is client-driven: `login::connect` dials in a task of its own (the frame keeps
    being drawn while a dead address times out) and reports back over the connect channel;
    `tui::mod::connected` spawns `client::listen_server` and marks the box `connected`, which is what
    turns `Esc` from "cancel the dial" into "quit". A refused connection stays in the box as an error
    instead of ending the process, and the attempt counter is what makes a cancelled dial's socket get
    dropped rather than land on the user. `auto_connect` prefills the field and dials without a
    keystroke — it is not a separate code path.
    **The server drives the other two steps.** `ClientEvent::Username`/`Register`/`Login` call
    `Login::ask`, which relabels the field, clears it and stores the server's rules as the hint;
    `UsernameRejected`/`PasswordRejected` set `Login::error` (which `ask` deliberately does *not* clear —
    the rejection arrives immediately before the re-prompt and still has to be read); `Authenticated`
    drops `App::login` and hands the keyboard to the input bar. **Those arms have to set `App::dirty`
    themselves** — unlike the rest of `App::apply` they push nothing into the history, so without it the
    box silently stops repainting. An answered step is not sent from the prompt: `login::Action::Submit`
    hands the text to `mod.rs::submit` like any other line, and `options::get_login_state()` turns it
    into the right packet.
  - **A lost session comes back to the box instead of ending the client.** `ClientEvent::Quit` (which
    `listen_server` also sends when the socket simply dies) and `ReconnectFailed` call
    `App::disconnected(reason)`: it rebuilds `App::login` with `Login::again` at the `Address` stage —
    address prefilled, reason as the error, **attempt counter carried over** so a dial cancelled before
    the drop cannot land on the new prompt — clears the history/sidebar/voice/overlays, and resets the
    session state that lives outside `App` (`state::reset_session`: sequence numbers, login state,
    channel, `ACTIVE_UPLOADS`, the voice/screen flags whose tasks watch them). The dead write half
    belongs to the event loop, so the reset only sets `App::drop_stream` and `tui::run` drops it.
    The one disconnect that still ends the process is the one the user asked for: `submit` sets
    `App::leaving` on `Command::Exit`, and the `Quit` arm honours it.
  - **And a session the user did not end dials itself back** (`login::Reconnect`). The box it comes
    back to already has the address, so the only thing standing between a dropped link and the chat is
    the username and password being typed again — which is what `Reconnect` keeps: the two answers are
    recorded as they are submitted (`login::take_input`) and only promoted to credentials on
    `Authenticated`, so what is replayed is a pair that demonstrably worked. `App::disconnected` arms it,
    the redraw tick dials once `RECONNECT_DELAY` is up, and the `Username`/`Login` arms
    put the stored answer in the field for that same tick to send — nothing new crosses the wire, and a
    reconnect walks the ordinary login path packet for packet.
    **The box says so while it is doing it**, since nobody asked for this dial: the status row is
    `Reconnect::status` (`Connection lost, reconnecting… (2/5)`) rather than `Login::waiting`'s
    `Connecting…`, and the box is `busy` for the wait as well as the dial, so `Esc` cancels the whole
    thing. The drop reason is what the row underneath would have said, so it is left in `Login::error`
    for the one case it is still worth reading: the retries running out, where nothing dialled at all.
    What bounds it is that every failure is one of `RECONNECT_ATTEMPTS`, counted across the *whole*
    attempt (a refused dial in `tui::connected`, a failed handshake, a server that drops us again) and
    reset only by an `Authenticated`, and that a dial may replay two answers and no more — a server that
    keeps asking is not answered forever. The replay is gated on `retrying`, which is the same flag the
    status row reads, so it covers exactly the dials *we* started: once the tries run out the flag goes
    down with them, and the address the user then types is a login they answer themselves rather than
    one that fills itself in. It is given up on rather than retried when the answer stops
    being ours to give: a `/logout`, an `Esc` on the prompt, a `Register` step (the account is gone, so
    there is nothing to replay) or a rejected username all `forget` the pair outright. A `Command::Exit`
    never reaches this — it ends the process.
  - C libraries that write to fd 2 (cpal/ALSA, openh264/xcap) corrupt the frame; the existing
    `gag::Gag::stderr()` wrappers in `network/voice/client` must stay.
  - `tui::install_panic_hook` is called first thing in `main` and is **not optional**. The release
    profile now unwinds (`panic = "unwind"`, set workspace-wide because the server must survive a
    panicking connection task and Cargo cannot scope `panic` per-binary), so `TerminalGuard::drop`
    does run — but only for a panic on the main thread while the guard is alive. A panic in a
    spawned task or before the guard exists still leaves the alternate screen up, and the hook is
    the only path out of it.
  - Fatal events (`TofuError`, a user-asked-for `Quit`) do not `process::exit` from the draw path. They call
    `App::quit(code, message)`; the loop breaks, the guard restores the terminal, and `run_client`
    prints the message on the normal screen.
  - The message pane is wrapped by `state::wrap_line` (cached per width + history generation) rather
    than by `Paragraph`, so the scroll offset is exact. `App::scroll == None` means stuck to the
    bottom.
  - **A picture in the pane is an animation with one frame in it, and the redraw tick is its clock.**
    `Fitted` holds every frame at the height `IMAGE_ROWS` allows (cut down once, in `App::fit` — an
    animation is all of its frames in memory at the same time), plus which one the protocol currently
    holds and when the next is due; `App::advance_animations` is called from `tui::run`'s 33 ms tick,
    steps whatever is due and sets `App::dirty`, which is the only thing that repaints the pane. The
    tick is therefore also the floor on the frame rate — a GIF asking for 10 ms is played at the tick
    and not faster — and a frame that comes due late is **skipped rather than shown late**, so the pace
    stays the animation's own. Nothing bumps `generation`: every frame of a GIF is the same size, so the
    rows it reserves cannot change and the wrap does not need redoing.
    - **Only the pictures on screen are stepped.** Fitting a frame and handing it to the terminal is the
      whole cost of an animation, and a pane of scrolled-past GIFs would pay it for every one of them
      every tick. The placements it filters on are the last frame's, which is exactly what was drawn.
    - **The protocol is rebuilt around its own `StatefulProtocolType`, never asked for again from the
      `Picker`.** That type carries the id the terminal knows this picture by, so reusing it *replaces*
      the picture kitty-side; a fresh protocol per frame would leave a new image behind in the terminal
      thirty times a second. There is no public way to hand an existing `StatefulProtocol` a new source
      image, so `protocol_type_owned()` + `StatefulProtocol::new` is how the id survives the frame.
    - A pane that was not drawn for `ANIMATION_CATCHUP` starts again from now instead of winding through
      every frame it missed.
  - **Capturing the mouse takes the terminal's own drag-select away, so the client provides one**
    (`App::selection`, `mouse_capture = true`). A press in the message pane anchors it, a drag extends
    it and the release copies — but a press is **not** a selection until a drag arrives (`dragged`),
    which is what keeps a click on an image caption a click. Both ends are stored as **wrapped-view
    rows, not terminal rows**, so scrolling during or after a drag moves the highlight with the text
    instead of leaving it on the cells the pointer happened to cross; a drag past either edge scrolls
    the pane rather than stopping at it. What is copied is sliced out of the same wrapped lines that
    are highlighted (`state::slice_cells`, cells rather than characters, so a wide glyph is taken
    whole), so the copy cannot disagree with what is on screen.
    The copy is **OSC 52** (`tui::copy_to_clipboard`) rather than a clipboard library: it needs no
    X11/Wayland system dependency, and over SSH a local clipboard would be the wrong machine's. The
    terminal either takes it or ignores it — there is no answer to read, so a terminal that refuses
    OSC 52 writes copies nothing and the client cannot tell. `theme::SELECTION` is the chrome's sky blue
    taken down to a background, and a background **only** — every glyph keeps its own colour, so a
    username stays the colour it is being copied as. The accent at full strength would have to repaint
    the text dark to stay readable on it, which is exactly what a selection must not do.
  - **A copy says so in the pane's bottom border, not in the history** (`App::notify`/`App::notice`,
    `draw_messages`' second `title_bottom`). The pane is the conversation, so something the user *did*
    does not belong in it as a line — and a toast that expires takes no row away from what was said.
    Nothing pushes it out again, so `tui::run`'s redraw tick calls `App::expire_notice`, which costs a
    frame only on the pass it actually expires on.
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
  - **Anything that scrolls says so**, via `draw::draw_scrollbar` — the message pane, the slash-command
    popup (commands *and* value lists) and the `/settings` box in both its modes. It overwrites cells of
    the box's own right border between the corners rather than claiming a column, so no list gets
    narrower for having one, and it is skipped entirely while everything fits: a visible bar always means
    there is more off-screen. The thumb is placed by hand instead of by ratatui's `Scrollbar`, which
    never quite lands on either end of the track — being *at the bottom* is the one thing the bar has to
    state unambiguously. The scrollbar is drawn after the block, and the caller passes the box rect (not
    `block.inner`) plus the same `first`/`visible` the rows were built from — in the palette that `first`
    is computed once in `draw_palette` and handed to `entry_lines`/`value_lines` precisely so the two
    cannot disagree.
  - **A channel switch parks the pane it is leaving instead of clearing it** (`App::switch_channel`,
    matching WHY2-Desktop's `paneByChannel`). `App::messages` is the channel being read and
    `App::panes` holds the others' scrollback keyed by name (`""` is the lobby), so stepping out of the
    lobby and back shows what was said in it rather than an empty pane; the scroll position and the
    unread count are the *view's*, and reset on every switch. Nothing is replayed from the server for
    this — a channel switch asks for nothing, so the scrollback only exists client-side.
    What keeps that bounded is the same rule the sidebar runs on: a channel exists exactly as long as
    somebody is in it, so `App::prune_panes` drops the parked pane of a channel that no longer has
    anybody in it (after every roster re-derivation, and in the `ChannelDestroyed` arm). The lobby is
    never pruned, and the pane being read is not in the map to prune. A lost session throws the whole
    map away in `App::disconnected` — unlike a switch, nothing there is coming back to.
  - The sidebar is fed by events, never by polling. `App::refresh_online` (a `PacketCode::List`
    request drained on the redraw tick) is only set for things that genuinely change the roster —
    `Authenticated` and `Join` — `Join` carries only a username, so the roster has to be asked.
    **A channel switch must not trigger one**: it would land
    inside the server's `min_message_delay` window right behind the `/channel` packet and earn a
    `SpamWarning` (three of those disconnect). **`Leave` must not either, for the same reason**: it
    is broadcast to the kicker as well, so a `/kick` would put the `List` request directly behind
    its own `ServerKick` packet and warn the moderator for spam. It does not need one — `Leave`
    names the id, so the arm drops that user from `App::online` itself and re-derives the channels
    from what is left. The channel list is maintained from the globally
    broadcast `ChannelCreated`/`ChannelDestroyed` packets plus whatever the last `List` showed —
    a channel exists exactly as long as somebody is in it, so the lobby is not one and is not
    listed. (`ChannelDestroyed` is only sent on a `/channel` switch, never on a disconnect, so
    re-deriving in the `Leave` arm is also what retires a channel whose last member dropped.)
  - **The voice panel is the channel's roster, not our own voice session**, so somebody who never
    types `/voice` still sees who is in it. `PacketCode::VoiceJoin`/`VoiceLeave` (named
    `ChannelJoin`/`ChannelLeave` before — they were always the *voice* pair, while
    `ChannelCreated`/`ChannelDestroyed` are the text-channel one) already went to the whole channel;
    what made them invisible was the client, which handled them only under `client_voice` and only
    to add and drop audio consumers. Both arms now also raise a `ClientEvent`, and
    `PacketCode::VoiceClients` — the whole roster, self excluded — is sent on login as well as on a
    channel switch and on joining voice, so a client that never joins still learns who was already
    talking when it arrived. Nothing new crosses the wire for this.
    - **The two sources are kept apart and merged on the way to the panel.** `App::voice_roster` is
      the server's truth (who is in voice in our channel) and `App::voice_activity` is the last
      `VoiceActivity` tick from the local voice session (who is *speaking*, and their ping); only
      the first arrives while we are not in voice, and only the second knows anything about sound.
      `App::rebuild_voice` builds `App::voice` out of both, which is why nothing writes `App::voice`
      directly any more — a `VoiceActivity` that replaced it wholesale, as it used to, would drop
      every roster entry we have no stream for. Our own row comes from the activity (the roster
      never names us) and is added only while `voice_enabled`.
    - `VoiceUser::latency` is an `Option` for the same reason: a roster entry we are not receiving
      is in voice, we simply have no ping for them, and `0ms` would be a lie rather than a blank.
      A per-user mute is likewise only drawn while we are the one listening.
    - The roster is dropped on a channel switch (it is per channel, and the server sends the new
      one unasked, straight behind the `Channel` packet) and on a lost session. A disconnect
      needs no `VoiceLeave`: `Leave` is broadcast to every channel and names the id, so the arm
      drops them itself — the same reason it maintains `App::online` by hand.
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
    `Screen share volume` (`screen_volume`, `client_screen` only) is the same kind of row pointed at a
    different atomic: `screen::client::options::get_screen_gain`, read once per output callback in
    `screen::client::audio::spawn_audio_playback` and soft-clipped like the voice mix. It is **playback
    only** — an attached viewer turning a share down changes nothing for the sharer or for anybody else,
    and it is deliberately separate from `output_volume` so a loud share can be ducked without also
    ducking the voices being talked over.
    Device lists come from one `spawn_blocking` call to `voice::client::list_devices` when the command
    is typed (`mod.rs::audio_devices`, gagged stderr), never from the draw path. **That list has to come
    from the voice client itself**: it enumerates `voice::client::audio_hosts` — the ALSA host that
    `audio_host()` pins for latency, then the sound server's own host — and the client later opens the
    chosen device out of the same hosts. Listing anywhere else (`cpal::default_host()` in the client
    binary, as it used to be) hands the picker names from PulseAudio while the opener looks them up in
    ALSA, and every switch fails. `client.toml` stores the **cpal device id** (`alsa:plughw:CARD=1,DEV=0`,
    `pulseaudio:alsa_input…`), which carries its host and is unique; the description is display only
    (`Settings::device_label`) because ALSA hands the same one to a dozen PCMs. `is_usable` drops the
    ALSA PCMs that are noise (`null`, `hw:`, `surround*`, `iec958`) and, once a sound server is running,
    the raw cards too — the server holds those and ALSA can only report them busy.
    Picking a device bumps `voice_options::mark_devices_changed`, and the voice session's VAD task
    rebuilds both cpal streams (`voice::client::replace_streams`) within its 100 ms tick: the UDP socket,
    `CONSUMERS` and the jitter buffers survive, so the call does not drop. The old pair is dropped
    **before** the new one is built — a PCM is exclusive, so the device that is kept across the switch
    (usually one of the two) would refuse a second open. A device that will not open puts the previous
    pair back, points `input_device`/`output_device` at it again and reports
    `ClientEvent::VoiceDeviceFailed`, which re-reads those two rows (`Settings::refresh_devices`).
  - **The same overlay is also the server's config**, in a second mode (`Settings::server`). `/server settings`
    sends `PacketCode::ServerSettings { settings: None, save: false }` and opens nothing — the box is opened
    by the answer (`ClientEvent::ServerSettings`), because the client is not the one who decides whether it
    may see it: the server checks `role >= consts::SERVER_SETTINGS_ROLE` and answers `InvalidUsage` otherwise.
    One packet code carries all four messages (request, the whole config, a save, the ack), and the ack is the
    **whole config again** rather than an "ok", so a key the server refused snaps back in the rows instead of
    sitting there looking applied. Nothing in the client names a server key: `config::server_settings` walks
    `server.toml` itself and sends each key with its `# section` heading and trailing comment, so a key added
    to the default config appears in the overlay with no client change. In the other direction
    `config::server_settings_write` accepts only keys the file already has, with the datatype they already
    have — the client does not get to invent keys or retype them. This mode is the one place the overlay does
    **not** write through: server rows are held (`Item::changed`, marked `●`) until `[ Save ]`/Ctrl+S, which
    only hands the changed rows to `tui::run`'s key path — the overlay never touches the socket itself.
  - **The selected row's description is wrapped in the foot of the settings box, not put in the title bar.**
    `server.toml`'s comments are whole sentences and are the only thing saying what a key does, so a title
    bar — which shares its width with the title and can only truncate — loses the half that mattered;
    `draw::description_lines` wraps it with `state::wrap_line` across the full inner width instead, under a
    rule. The foot is sized for the **longest** comment in the box rather than the selected one, so the box
    does not grow and shrink as the selection moves, and it is dropped entirely when the terminal has no
    room for both it and the rows. Client rows carry no comment, so `/settings` proper is unchanged. The
    scrollbar is handed the rows' own height (not `popup`), because the description is not part of what it
    is measuring.
  - **A saved server key is live the moment it is stored — except the four the server only reads while
    starting up.** Every other key is read at its point of use through `config::read_config` (which is
    cached, so a write is visible to the next read), so a limit or a length raised in the overlay applies
    to the next packet. `consts::SERVER_RESTART_SETTINGS` names the ones that do not: `server_ip` and
    `server_port` (the listener is already bound), `enable_voice_chat` (the UDP server is spawned once in
    `bin/server.rs`) and `server_username` (latched into `options::set_server_username`). `server_settings`
    stamps `ServerSetting::restart` from that list, and the client marks those rows `↻`, adds
    `· restart required` to the description under them and says so in the history when such a row is saved — the save itself is
    never refused, the value is stored either way. Adding a key that is only read at startup means adding
    it to that const; the handshake budget used to be one of those (a `LazyLock` over `max_clients` +
    `max_unauth_clients`) and was turned into `server::max_handshakes()` rather than listed, because the
    connection limit beside it was already live and having half of one pair need a restart is a trap.
  - **And the overlay can do the restart those keys are waiting for.** A second button under `[ Save ]`
    sends `PacketCode::ServerRestart` (owner only, checked server-side like every other server-settings
    packet); the server disconnects everybody gracefully and then **re-execs itself** —
    `misc::restart()`, `exec` on unix so the pid and whatever supervises it survive, spawn-and-exit
    everywhere else. Re-reading the config in place was the alternative and is not the same thing: the
    startup-only keys are startup-only precisely because their effect is bound at startup (a listener, a
    spawned UDP task, a latched username), so the only honest way to apply them is to start up again.
    The restart is deliberately *awkward*: it is refused while any row is unsaved (the restart would throw
    those edits away unread — the description under the button says to save first), and one press only
    arms it while the next fires it, since it ends the session for every client on the server. Nothing
    comes back on the socket — the client's own connection dies with the server and lands in the connect
    box like any other drop. `bin/server.rs` binds with a short retry (`BIND_ATTEMPTS`) rather than
    exiting on the first `EADDRINUSE`, because the non-`exec` path starts the replacement beside a
    process that may still be holding the port.
  - Chat messages live in `App::messages` as `state::Entry::Message` (username/id/text/colors), not as
    rendered `Line`s — `Theme::render` turns an entry into the rows it occupies on every wrap, so a
    `show_id` or `disable_colors` change repaints the messages already in the pane. Anything that rewrites
    config-driven styling must call `App::reload_theme` (which bumps the wrap-cache generation), never
    `Theme::reload` on its own.
  - **What somebody typed goes through `tui/markup.rs`, and that is why `Theme::render` returns rows
    rather than one logical line.** Everything a user wrote is parsed there — Discord's fenced ```` ``` ````
    blocks and inline `` ` `` code, plus `$…$`/`$$…$$` math — and the parser **never consumes what it cannot
    close**: an unterminated fence is backticks somebody typed, not a block that swallows the rest of the
    message. A backslash takes the markup off the character after it, and a delimiter that was not found
    once is not searched for again (a message of nothing but backticks would otherwise cost a scan per
    backtick).
    A fenced block is **rows, not text**: they are padded to the pane so the block reads as a box, which
    is exactly why they cannot be handed to `state::wrap_line` afterwards — code is broken where it runs
    out of cells, not at the last space before it, and the padding must not be re-wrapped. That padding is
    also why `draw::draw_logo` treats a painted background as a claimed cell: blank cells that are part of
    a box are not free ones, and the watermark used to come through them.
    The markup reaches a private message too (`Entry::Prefixed`, a client-written prefix in front of a
    user-written tail) but deliberately not `Entry::Line`, which is the client's *own* output — `/help`
    and `/list` are not somebody's text and have nothing to parse.
  - **`tui/math.rs` lays TeX out in cells, and it is a subset on purpose.** A terminal has one font size
    and a fixed grid, so what is rendered is the part of the notation the grid can carry: a `Block` is a
    rectangle of cells plus the row the next one lines up with, and every step (a fraction over its rule,
    a root under its bar, an operator with its limits, a `\left(` stretched to what it holds) is a
    combination of those. Nothing is ever placed by counting rows from the top, which is what keeps a
    fraction inside an exponent inside a root aligned with its neighbours.
    - **Display math (`$$…$$`) owns its rows and is laid out in two dimensions; inline math has to fit on
      the row it was typed on**, so it is set linearly instead — a script becomes a Unicode superscript
      where one exists (`x²`, `aᵢⱼ`) and `^(…)` where it does not, and a fraction becomes `a/b` rather
      than silently taking two rows off the message. A display that would be wider than the pane falls
      back to the same linear form and is wrapped: a truncated formula is worse than a plain one.
    - **Unknown commands cost their backslash and nothing else** — `\foobar` sets as `foobar` — so an
      environment this does not implement (`\begin{matrix}`, and anything else with a 2D structure of its
      own) comes out as the words it was written with rather than as a hole. The scripts of `\int` stay at
      its side while `\sum`'s go over and under it (`BIG`), because that is where each one belongs.
    - **The parser's depth is bounded (`MAX_DEPTH`) because nothing else bounds it**: the string is off the
      network, and a message of ten thousand open braces would otherwise recurse until the stack ended.
    - Math that the message gave no colour of its own is `theme::MATH`; math inside a coloured message
      keeps the sender's colour, the way the rest of their text does.
    - **`render_math` (client.toml, default on, a `/settings` row) turns the whole of it off**, and with it
      off a dollar sign is a dollar sign: `parse` never opens a math segment, so the formula is shown as
      it was typed rather than as an approximation of itself. It is deliberately separate from the code
      markup, which has no switch — a fenced block is what the sender meant either way, while a formula a
      terminal cannot set faithfully is a matter of taste. `Theme` caches the key like the rest of them,
      so toggling the row repaints the messages already in the pane through `App::reload_theme`.
  - Transient prompts belong in the chrome, not the history. The username/password steps live in the
    connect box and vanish once answered; nothing pushes them into `App::messages`. Block commands (`/help`, `/list`, `/files`, …) end without a trailing
    blank line — the styled headings already separate them.
- **`bin/server.rs`** — headless server entrypoint (`tokio::main`), wires together `network::server`,
  `network::file::server`, `network::screen::server`, `network::voice::server`.
- **`command.rs` / `options.rs`** — in-chat slash commands (`/pm`, `/channel`, `/voice`, etc.) and
  CLI argument parsing respectively.

When adding a new packet type or handler, changes typically need to touch: `network/codes.rs`
(`PacketCode` enum) and both `network/client.rs` and `network/server.rs` (or the relevant
file/screen/voice submodule).

## Server logging

The server logs through `log` + `simple_logger` (both `server`-only dependencies — the client has no
logger and prints nothing outside the TUI, see above). Two rules decide every line, and new code has
to keep to them.

- **A line identifies a client by its address and by nothing else.** No username, no message or
  private-message text, no channel or file name, no password, key, token or hash ever reaches the
  log — those are the users' and a server operator's log is not where they belong. What is logged
  beside the address is the server's own vocabulary and the shape of what happened: the packet's
  control code (`PacketCode::name`, which exists for this and returns the variant name *only* —
  `PacketCode` deliberately does not derive `Debug`, which would print the fields with it), a byte
  or character count, a limit that was hit, a role, a count of connections. `Role` crosses into the
  log as itself for the same reason it crosses the wire as itself.
- **An auxiliary connection is logged as the main connection that asked for it.** An upload, a
  download, a screen share, a viewer attachment and a voice session are each a socket of their own on
  an ephemeral port that nothing else in the log ever names, so keying a line on it would produce
  lines nobody can tie to anything. `server::log_addr(&id)` resolves a client id to its main
  connection's address and is what those paths log — `file/server.rs` collects the address up front
  when it collects the keys, `screen/server.rs` takes it once per share, `bin/server.rs` takes it in
  the accept loop the moment a token is matched, and `voice/server.rs` takes it per event (its own
  address is the UDP one). **`log_addr` walks `CONNECTIONS`, so it must never be called while a guard
  on that map is held** — the established pattern of collecting into locals and dropping the guard
  applies to it like to any other read.

`log_level` (`server.toml`, default `info`) is the verbosity, parsed as a `LevelFilter` and falling
back to `info` on anything unrecognised. `info` is the operator's view — a connection's life
(accepted, key exchange, authenticated, closed with the reason), every transfer, every share, every
moderation action and every settings save. `debug` adds the per-packet line (one per received
packet, its code only), the handshake's steps, rekeys and the shedding decisions. The key is read
once, when the logger is built, so it is in `consts::SERVER_RESTART_SETTINGS` — and because the
logger has to exist before anything has something to say, `bin/server.rs` calls `config::init_config`
*before* it, ahead of the version check that was first.

Levels are not decoration: `warn` is a client being refused something (a limit, a bad password, an
undecodable packet, a spam violation, a viewer shed) — routine, and not the operator's problem;
`error` is the server's own (a bind that failed, a stored image or history that will not verify).

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
