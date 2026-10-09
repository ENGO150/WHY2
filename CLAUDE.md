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
The GPU video encoder adds nothing to that list: Vulkan is loaded at runtime, and Media Foundation and
VideoToolbox are part of their OS.

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

There is deliberately **no standing benchmark for the capture pipeline** — the per-stage
instrumentation, the headless comparator and the converter and encoder benchmarks that produced
the numbers below were development scaffolding and were removed once the work landed. Anything measuring capture cost
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
- **`server::send_to_all` goes to every authenticated client, and the client decides what belongs in which
  pane.** It takes the packet and nothing else — there is no channel filter on the server. A packet whose
  meaning depends on a channel names it instead, and the client files it:
  - **`Message` and `ImageDisplay` carry `channel: Option<Option<String>>`**: `Some(None)` is the lobby,
    `Some(Some(name))` a named channel, and `None` means every pane (nothing sends that yet). A line for the
    channel being read goes into `App::messages` as before; any other goes into that channel's *parked* pane
    (`App::park_entry`, trimmed to `HISTORY_LIMIT` like `push_entry`). This is the whole point of it: a
    message said in a channel we are not standing in used to never reach us, so it was missing from that
    pane on the way back until a reconnect. A pane for a channel we never visited is created on the spot
    and pruned like any other.
  - **A parked picture is cached, never decoded.** `network/client/mod.rs`'s `ImageDisplay` arm checks the
    channel against `options::get_channel()` first; a foreign one stores the bytes if they came (the same
    rule `auto_show_images` off follows) and raises `ClientEvent::ImageParked`, which parks a caption as
    `Picture::Deferred` (`Absent` with `auto_show_images` off). It loads when that pane is looked at, the
    same way a replayed picture does. If we switched into the channel while the event was on its way, the
    arm pushes the caption into the live pane instead — parking it under the channel being read would be
    overwritten by the next `switch_channel`.
  - **`VoiceJoin`/`VoiceLeave` carry `channel: Option<String>`** and the client ignores one for a channel it
    is not in, before it touches the roster or the audio consumers — adding a consumer for somebody in
    another channel would play them.
  - **Be honest about what this costs.** Every channel's text reaches every client on the server, so a
    channel is a place to talk, not a way to keep anything from anybody's client; and a fresh picture's
    payload goes out once per client on the server rather than once per client in the channel.
    `send_to_others` (the typing indicator) still filters on the server — a notice nobody is looking at is
    worth nothing parked.
- **The two costs an image puts on somebody else are bounded explicitly, because neither is bounded
  by the 8MB `MAX_IMAGE_SIZE` the server accepts.**
  - **Decoding is limited on the client** (`network/client/image.rs::decode_image`). `MAX_IMAGE_SIZE`
    bounds the bytes on the wire and says nothing about what they unpack to: a 292KB PNG decodes to
    400MB, and `ImageDisplay` is **pushed rather than asked for**, so every client in the channel
    decodes whatever was posted (clients elsewhere only cache it), one unbounded `tokio::spawn` per packet. `image`'s own default
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
    A request for a picture nothing names is **answered with empty data** rather than silence: it
    decodes to nothing, so the caption goes to `[ unavailable ]`, and it frees the client's in-flight
    slot (below), which silence would hold forever.
  - **The client keeps at most `MAX_IMAGE_FETCHES` fetches in flight** (`App::take_image_requests`,
    drained by the redraw tick; `App::fetched` frees a slot on any `ImageData`). The server serves one
    per `IMAGE_REQUEST_DELAY` on the connection's own read loop, so every queued fetch is also that
    long of the client's *other* packets waiting behind it — scrolling fast through a history of
    pictures used to queue one per caption it passed. A queued request whose caption has left the
    screen before its turn is not sent at all: the caption goes back to `Picture::Deferred` and asks
    again if it comes back into view. A request with no waiting caption in the pane (the profile
    box's avatar) is always sent. A channel switch turns the parked pane's `Waiting` captions back to
    `Deferred` for the same reason — `deliver_image` only searches the pane being looked at, so an
    answer arriving meanwhile would never reach them.
  - **`server_images/` is sealed encrypt-then-MAC, and it is read back in the chunks it was written
    in.** `crypto::image_keys` HKDFs a key, nonce and MAC key per picture out of `server_image_key`
    salted with the hash the file is named after, so nothing about the pair is kept anywhere and two
    pictures never share a keystream. The tag is required for the same reason the history's is: a
    stored picture is decrypted and pushed to every client on the server, so bare CTR would put
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
    `PacketCode` per recipient and every connection has its own keys, so an 8MB picture on a
    20-client server is 20 clones and 20 independent REX encryptions — there is no shared ciphertext
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
  - **A replayed picture is loaded when it is looked at, from the cache or else from the server.**
    With `auto_show_images` on, every replayed caption comes up as `Picture::Deferred`
    (`[ loading... ]`, the same line `Waiting` draws) rather than `Absent` (`[ show ]`) — offering a
    button for a picture that is going to fill itself is the one thing it must not do, and
    `request_image` would refuse the click anyway. Nothing is checked at replay time: it used to
    `stat` the cache per picture and leave the misses as buttons, so an uncached picture was never
    shown unasked. **The load waits for the picture to be on screen**: decoding every one up front
    cost a `MAX_IMAGE_ALLOC` decode, a fit and a held protocol per picture for a paneful nobody had
    scrolled to yet — and fetching every miss would be a whole history's pictures off the server.
    **"On screen" means within reach, not strictly in view** (`state::in_reach`): the pane is treated
    as `PRELOAD_SCREENS` screens taller in both directions, so a picture is fetched, decoded and fitted
    while it is still a scroll away and is already whole when it arrives. Loading it only once it was
    in view meant every picture popped in under the reader's eyes while they scrolled — captions
    appearing, then the rows opening up — which read as a pane that was broken. The margin is a whole
    screen rather than a row count because a scroll step is a fraction of one and PageUp is one; it is
    bounded, so this is still a paneful or two of pictures and never the whole history. The same window
    is what `take_image_requests` keeps a queued fetch for, and animations are still stepped only while
    actually on screen (`advance_animations`) — a preloaded GIF needs its protocol, not its clock.
    `App::load_visible` flips a `Deferred` caption that has come within reach to `Waiting` and puts its
    hash in `App::image_loads`, which `tui::run`'s tick hands to `client::fetch_image` — the same
    cache-then-server path a click takes, one task per picture that is actually being looked at; a
    miss joins the capped fetch queue above. Each
    hit arrives as an ordinary `ClientEvent::ImageData`, which is why `deliver_image` fills
    `Picture::Absent` as well as `Waiting`: an answer nobody clicked for is what a cache hit *is*. It
    deliberately does **not** fill a `Deferred` line — that one has not asked yet, and filling it
    first would spend the answer on a caption that is still off screen. A refusal (`None`) still only
    marks a line that actually asked.
  - **A decoded picture is built for the terminal only while it is within reach, and put down again when
    it leaves** (`App::load_visible`, called from `draw_messages` with the same width, offset and
    viewport the pane was just drawn with). Fitting a picture to the pane is a resize of the whole
    thing and the `StatefulProtocol` then holds that copy, so doing it in `rewrap` — as it used to —
    meant every picture in the scrollback paid both, and a terminal resize re-fitted all of them in
    one pass. `rewrap` now only *reserves* the rows, which is `fit_size` arithmetic over the frame's
    dimensions and no pixels at all; `fit_image` derives from the same function, so the rows a picture
    claims and the size it is drawn at cannot drift apart. An unloaded picture keeps its frames and
    its `Unloaded` — the protocol type and background — because that is the id the terminal knows it
    by: rebuilding through `StatefulProtocol::new` with the old type *replaces* the picture
    kitty-side, while a fresh `new_resize_protocol` would leave the old one behind in the terminal
    every time it was scrolled past.
  - **`auto_show_images` (client.toml, default on) makes a live picture behave like a replayed one.**
    With it off, `network/client/mod.rs`'s `ImageDisplay` arm decodes nothing: the line goes up as a
    caption with a `[ show ]` button (`ClientEvent::ImageOffer` → `push_caption(.., Picture::Absent, ..)`)
    and every replayed caption comes up `Absent`, so it is a button too. The two
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
- **`network/client/` / `network/server/`** — connection-level logic (handshake, auth, message
  dispatch) for each side, each a `mod.rs` holding the listen loop and the packet match beside the
  pieces it is built out of: the client's key exchange, TOFU verdict and dial in `client/handshake.rs`
  and its picture decoding in `client/image.rs`; the server's in `server/handshake.rs` and
  `server/connection.rs`. `network/file`, `network/screen`, `network/voice` are protocol
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
    rescue, since nothing would have failed. **The portal is the exception**: it is ready once its
    PipeWire stream reaches `Streaming`, frame or none. KWin and mutter only send a frame on damage, so
    a still desktop delivered nothing inside `RECORDER_FIRST_FRAME` and the probe threw away the only
    backend those compositors have — neither offers screencopy, so the share died on the polling path's
    error instead. A negotiated stream from a real portal is not xcap's silent X11 recorder.
    The portal also asks for `cursor_mode` embedded where it is offered, matching the polling path
    (which overlays the cursor) and giving a damage-driven compositor something to send when the
    pointer moves. `RECORDER_PROBE_TIMEOUT` now only applies where the
    polling path could not start at all and the recorder is the last backend left rather than an
    upgrade — that is the one case worth blocking for. When both fail, the error names both
    (`screen.error.no_backend`): the polling path's alone blamed the compositor's screencopy support on
    a desktop where the portal was the one that was meant to work.
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
    `last_raw`: nothing was missed and the picture has not moved.
  - **The wayshot path reads the compositor's bytes itself instead of asking libwayshot for an
    image** (`ShmCapture`). `screenshot_single_output` costs ~6 ms of CPU a frame before the encoder
    sees anything: a fresh memfd, an in-place BGRA→RGBA swizzle, and then an `RgbaImage` built with
    `put_pixel` one pixel at a time, a division and a modulo each. `ShmCapture` keeps one memfd (sized
    from the probe frame plus `SHM_ROW_SLACK` a row, which costs nothing until written — memfd pages
    are only allocated when touched), hands it to `capture_output_frame_shm_fd`, maps it once and
    copies the rows out as the compositor laid them — `Xrgb8888` is `PixelOrder::Bgra` in memory and
    goes to the encoder unswizzled. Measured A/B on the same content (Hyprland, 1080p, 24 fps of
    change), the whole share went from ~41% of a core to ~26%. Anything it does not recognise — a
    rotated output, a 10-bit format, a frame larger than the file — returns `None` and the loop drops
    to libwayshot's own path for the rest of that share. The leak budget above still applies: the
    frame objects are libwayshot's either way.
  - **Which monitor is shared is a client-local choice**, not part of the protocol: `/screen [MONITOR]`
    (a 1-based index or a monitor name) stores it in `screen::client::options::set_monitor` and
    `capture::get_target_monitor` resolves it; the `Screen` packet still only toggles the share, and the
    palette offers the monitor names (`ArgValues::Monitors` → `capture::monitor_names`, cached for
    `MONITOR_LIST_TTL` because the popup asks on every keystroke). `command.rs` resolves the parameter
    to a monitor *name* through `capture::resolve_monitor` before storing it, so an unknown monitor is
    invalid usage on the spot rather than a share that starts and dies, and so `/screen 2` and
    `/screen DP-2` are recognised as the same monitor. **The pick lasts exactly as long as the share
    does** — it lives only in that atomic-style global, and every path that ends a share puts it back to
    `None` (the `Screen { token: None }` arm in `network/client/mod.rs`, `state::reset_session` for a lost
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
    the selection away and ask again. Only where the polling path runs, though — a compositor without
    screencopy (KWin, mutter) falls through to the portal, whose picker is then the only way to choose.
  - **On Wayland the recorder is ours, not xcap's** (`client/portal.rs`, `PortalRecorder`). xcap's
    never worked on Hyprland at all: its `select_sources` subscribes to the portal's `Response` and
    never waits for it, so `Start` arrives before the picker is answered and fails with `Sources not
    selected`, and every share quietly stayed on the polling path. `request` subscribes first, calls,
    then blocks on the answer. It also never says how many frames it wants.
    xdg-desktop-portal-hyprland reads `maxFramerate` out of the negotiated format and captures at
    exactly that rate; without the property the negotiation fixates on the portal's own default — the
    monitor's refresh rate, clamped to its `screencopy:max_fps` (120). Every one of those frames is an
    SHM screencopy, a GPU→CPU readback inside the compositor's render loop, so it would have stalled
    the compositor up to 120 times a second for the 30 we encode. Our format
    offers `maxFramerate` as a range topping out at the share's own fps; SPA's range intersection
    keeps a default that lies inside the result, so the portal's 120 loses to our 30. A compositor
    whose portal ignores the property is no worse off than before.
    It also does what xcap's could not: it passes `BGRx` through as `PixelOrder::Bgra` instead of
    swizzling every pixel on PipeWire's thread, connects through `OpenPipeWireRemote` rather than the
    default daemon, copies out of the buffer into a recycled `Vec` (`LatestFrame::buffer`, at most
    `SPARE_FRAMES` kept) rather than allocating 8 MB a frame, honours the chunk's stride, and
    **ends the stream and closes the portal session** on stop — xcap's left its PipeWire thread and its
    session running for the life of the process, one per share. X11, Windows and macOS still go
    through xcap; the session type decides in `open_recorder`.
  - **A frame that arrives early waits for its slot rather than being dropped** (`run_recorder`). The
    FPS budget used to `continue` past any frame inside `min_interval` of the last encode, which was
    harmless while xcap's Wayland recorder delivered 120 a second; against a source that delivers
    exactly the share's rate it throws away every frame that lands a millisecond early and halves the
    share. It now sleeps until the frame is due and then takes whatever is newest. **The slots are
    scheduled off each other, not off the encode** (`due += min_interval`): timing the next slot from
    when the last encode *finished* adds the encode to every period — 33 ms + ~12 ms is 22 fps, which is
    exactly what a 30 fps source measured until it was fixed. A slot more than a whole interval late
    restarts the schedule from now rather than bursting to catch up.
  - **The recorder is drained on a thread of its own, and that is not buffering for its own sake.**
    xcap delivers frames over a `sync_channel(0)` — a rendezvous — so its capture thread sits blocked
    in `send` for the whole of our colour conversion, GPU readback and H.264 encode, and the frame
    period is capture *plus* encode rather than the larger of the two. That costs little where a
    grab is cheap; on Windows it is most of the budget, because xcap's `texture_to_frame` pays a
    fresh `CreateTexture2D` staging allocation, a GPU→CPU `Map`, a zeroed `vec!`, a row-wise memcpy,
    a **whole extra `to_owned()` clone** and a scalar per-pixel BGRA→RGBA swizzle for every single
    frame — which is why a Windows share ran at a visibly lower rate than a Linux one on the same
    hardware. `drain_frames` moves the `Receiver` onto its own thread and keeps the newest frame in
    a one-slot `LatestFrame` (a `Mutex<Option<CapturedFrame>>` and a `Condvar`), so the next grab overlaps
    the encode instead of queueing behind it. Keeping only the newest is the same shedding rule the
    rest of the path runs on — an unread frame is already stale — so the slot replaces the old
    `try_recv` drain rather than adding a queue. **Known ceiling:** this makes the period
    `max(capture, encode)`; xcap's per-frame Windows cost itself is untouched and still caps the
    rate at high resolutions. Cutting that means not going through `texture_to_frame` — a reusable
    staging texture and handing the BGRA straight to the shader, which the converter could swizzle
    for free — and that is a fork of xcap's Windows backend, not a change here.
  - `WHY2_CAPTURE_BACKEND` (`recorder` / `legacy`) pins a backend; `WHY2_CAPTURE_PROBE_TIMEOUT`
    overrides the probe deadline in seconds. Both exist so a machine where the heuristic picks
    wrong is one env var away from the old behaviour.
- **The share's latency is bounded by shedding, not by buffering, and every queue on the path has to
  agree with that.** The pipeline already drops rather than waits where it matters —
  `FrameEncoder::submit` does not encode a frame while the network channel is full, so the frame
  the encoder never saw breaks no reference chain (`dispatch`'s tail-drop and forced IDR is only the
  fallback now) — but that only fires once a send actually blocks, and the two places it
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
  - **The bitrate adapts to the sharer's own uplink** (`client/rate.rs`, `RateControl`). Dropping an
    encoded frame and forcing an IDR was a death spiral on a link slower than the share: an IDR is
    several times a P-frame, so it took longer to send, the next frames found the channel full, and
    they forced IDRs of their own — measured over a ~3.5 Mbps WireGuard link, every frame on the wire
    was a 120–170 KB IDR at ~3 fps, and the server's per-viewer queue held ~2.4 s of them. The send
    loop reports the bytes it wrote and how long `write_all` blocked (`rate::record`), and on Linux
    the queue a new frame joins (`rate::probe`: `TCP_INFO`'s unsent bytes at the current target plus
    `tcpi_rtt` over `tcpi_min_rtt`, the minimum over a window so an IDR draining is not a queue).
    A window blocked for most of its length, or queueing past `RATE_DELAY`, backs off to
    `RATE_BACKOFF` of what went through; the episode's first window caps the ceiling at the target and
    a blocked one at what drained, and growth returns under that ceiling quickly and creeps past it
    (`RATE_CREEP`), only while the target is actually being spent (`RATE_USED`). Every backend takes
    the new target mid-stream (`Backend::set_bitrate`), and `Budget` follows it. **openh264 needs its
    ceiling moved with the target, on layer 0**: its frame-skip check (`CheckFrameSkipBasedMaxbr`) reads
    the layer's `iMaxSpatialBitrate`, which is fixed at the bitrate the encoder opened with, and
    `ENCODER_OPTION_MAX_BITRATE` on `SPATIAL_LAYER_ALL` never reaches it. Raising the target alone made
    openh264 skip in runs — measured at 1600x900, a ramp from 2.5 Mbps came out at 3–29 fps a second
    instead of 30, which was a software share's spiky ~17 fps. `Software` also passes real timestamps
    (`encode_at`): its rate control is defined in time, and a zero timestamp is read as one frame
    interval after the last call however long ago that was. The bitrate lives in a
    static so a backend or monitor switch keeps it; `rate::start` resets it per share.
    **The viewers' legs are reported back by the server** (`ScreenPacketCode::Feedback`), because a
    viewer whose download is slower than the sharer's upload is invisible from the sharer's socket:
    measured over WireGuard, the sharer pushed 6 Mbps without a single send blocking while a 3.5 Mbps
    server→viewer leg queued ~2 s behind it. Each viewer task measures the backlog a frame joins
    before it writes it (`Backlog`: how long the frame sat in that viewer's queue plus `tcpi_rtt` over
    `tcpi_min_rtt`, and the socket's unsent bytes, each the minimum per interval), and a reporter task
    per share (`feedback`) sends the worst viewer's figures every `FEEDBACK_INTERVAL`, with `shed` set
    if any viewer had a frame dropped. The sharer reads them on the share socket's otherwise unused
    read half (`rate::report`), turns the unsent bytes into time at its own target, and the controller
    takes the larger of its own delay and the server's, and treats `shed` like a full link.
    - **The report runs on its own REX stream** (`crypto::init_reverse_stream`, the token hashed under
      its own label): the upload's stream is keyed by the same token, and two directions on one
      keystream would be CTR keystream reuse.
    - **It is compatible both ways.** `Feedback` is the last variant, so the existing ones keep their
      encoding; an older sharer never reads that direction, and the reporter is its own task so a
      sharer that never drains it costs nothing but that task blocking; an older server simply never
      sends one. A non-Linux server reports queueing in its own viewer queue only (`tcp_backlog` is
      `None` there).
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
      encoder repeats in front of one), which `FrameEncoder::submit` guarantees by time
      (`KEYFRAME_INTERVAL`) — the encoders' own interval counts frames, and a still or throttled share
      encodes so few of them that it once left an IDR minutes away. Forwarding the P-frames instead would put frames on the wire whose
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
    - **The sharer is told when a viewer cannot keep up**, through the server's backlog report (above),
      rather than through backpressure as when forwarding was inline — the share slows down for the
      slowest viewer instead of being shed for it. Shedding remains for what the report cannot fix in
      time.
- **RGBA → I420 runs on the CPU, through openh264's own SIMD path** (`YuvScratch::fill`,
  `read_rgba8`/`read_bgra8`). The conversion was once the single most expensive stage of the share
  (~25 ms a frame at 1080p) because it went through `read_rgb`, openh264's per-pixel float
  `write_yuv_by_pixel`; openh264 0.9.8 has an AVX2 path for packed RGBA and BGRA (and an integer
  scalar one elsewhere) that `read_rgb` never reaches. Measured at 1080p on an i5-12400F: **0.78 ms**
  AVX2, ~2.4 ms scalar.
  **There is no GPU converter any more, and that is deliberate.** A `wgpu` compute shader did this
  while the CPU path was the float one, and it lost on every count once it was not: 1.64 ms on an
  *idle* GPU, for planes bit-identical to `read_rgba8`'s, and every frame was an 8 MB upload, a
  dispatch on the same queue as whatever is drawing the screen and a blocking readback — so sharing
  a GPU-bound game put our work in line behind the game's and the game's behind ours. Bringing one
  back would have to beat 0.78 ms of CPU *under that load*, not on an idle card.
- **The share is encoded on the GPU where there is one, and openh264 is the fallback**
  (`network/screen/client/encoder/`). `encoder::open` tries the platform's hardware encoder first —
  Vulkan Video on Linux (`vulkan.rs`, through `ash`), a hardware Media Foundation transform on Windows
  (`media_foundation.rs`), VideoToolbox on macOS (`video_toolbox.rs`) — and `FrameEncoder` (capture.rs)
  drops to `encoder::Software` the moment one fails to open *or* fails mid-share, forcing a keyframe on
  the way, and does not try the GPU again for the rest of that capture. Every backend takes the same
  I420 `Planes` from `YuvScratch` (the 0.78 ms conversion above stays on the CPU; the GPU backends
  interleave it into NV12 as they copy it into their own buffer) and hands back one Annex B access unit.
  - **The wire does not change, and that is the whole constraint on the GPU side.** The viewer decodes
    with openh264, whose decoder is documented as **Constrained Baseline only**, so every backend asks
    for Constrained Baseline (Baseline where an API has no separate name for it): no CABAC, no B-frames,
    no 8x8 transform. That gives back most of a hardware encoder's compression edge, but a GPU-encoded
    share is decodable by every client already in the field and the server needs nothing. Every IDR
    carries its SPS and PPS in front of it — `server.rs`'s `is_keyframe` and the attach-time keyframe
    cache depend on that — so the Vulkan backend prepends the sets the driver wrote, VideoToolbox's are
    taken off the format description, and Media Foundation's go through `ParameterSets::complete`,
    which only adds them to an IDR that arrived without.
  - **What it buys is CPU.** Measured on an RX 6650 XT (RADV) at 1080p30: ~0.8 ms of the capture
    thread's CPU a frame against ~10 ms for openh264, ~3 ms wall per encode, and ~2 dB more PSNR at the
    same 8 Mbps; on a live 1600x900 desktop the whole capture process went from ~9% of a core to ~5%.
    The CPU time left is the colour conversion and the copy, not the encode.
  - **Linux uses Vulkan Video rather than VAAPI**, and not for taste: VAAPI has no encoder on NVIDIA at
    all, and a Mesa built without `vaapi` (Gentoo's default USE) has none on AMD either, while RADV, ANV
    and NVIDIA's driver all expose `VK_KHR_video_encode_h264`. The driver also writes the SPS/PPS
    (`vkGetEncodedVideoSessionParametersKHR`) and the slice headers, so no bitstream writer lives here.
    `ash` loads `libvulkan` at runtime, so this adds no build dependency; a machine without a usable
    loader or encoder simply falls back. The session is one reference deep — two DPB layers
    ping-ponged, POC type 2, `max_num_ref_frames = 1` — which is all a low-latency P-only stream needs.
    Uploads go through a staging buffer on a transfer-capable queue and a semaphore into the encode
    queue, since an encode queue is not guaranteed to copy. RADV reports `maxLevelIdc` as 1.0 for this
    profile, so a reported 1.0 is read as "unreported" rather than as a limit (`level`).
  - **A GPU encoder holds the bitrate by skipping frames, like openh264 does** (`encoder::Budget`). The
    hardware rate controls keep every frame and overshoot on content that 8 Mbps cannot carry — 17 Mbps
    on full-motion 4K — where openh264 drops frames instead; `Budget` is a one-second token bucket that
    skips a frame (never a forced keyframe) while it is overdrawn, which keeps `H264_BITRATE` the
    ceiling `SOCKET_BUFFER` is sized for. A skipped frame is not encoded at all, so the reference chain
    is never broken.
  - **Media Foundation's hardware MFTs are asynchronous**, so `run_async` drives them off their event
    queue (`METransformNeedInput`/`HaveOutput`, polled with a timeout so a stalled MFT falls back
    instead of hanging the capture) and queues an early output so one call still returns one access
    unit. VideoToolbox is flushed with `complete_frames` after every frame, which keeps it synchronous.
  - **Only the Linux backend has been run on hardware.** The Windows and macOS backends were
    type-checked against their targets (`-Zbuild-std` with mingw for Windows; the macOS module in a
    scratch crate, since the full client needs the SDK) but not executed — a failure there costs the
    GPU, never the share, because of the fallback.
- **The software encoder runs in openh264's camera mode, not its screen-content mode** (`encoder::Software`,
  `UsageType::CameraVideoRealTime`). Screen-content mode is built for desktops, and on a game it
  falls apart: measured on 90 real frames of a game at 1600x900 and 4 Mbps, its rate control blew
  the budget and fell back on `skip_frames`, so **11 of every 30 frames came out** and what the viewer
  saw averaged 27.8 dB PSNR, with encodes spiking to 61 ms. Camera mode on the same frames put out all
  30 at 35.3 dB, at the same 4 Mbps and a 17 ms worst case. It is not worse on a desktop either: on
  scrolling text it measured 49.1 dB against 41.7 and encoded faster. What it costs there is
  bandwidth — ~3 Mbps where screen mode spent ~0.7 — but that is still inside `H264_BITRATE`, the
  rate every share is already allowed to reach. `skip_frames` stays on, since openh264's rate control
  cannot hold a bitrate without it. Slices plus `num_threads` cut the average encode by a third and
  were left out: they did not lower the worst case, and they take cores from whatever is being
  shared.
  **`H264_BITRATE` is 8 Mbps, and it is a ceiling, not a rate.** On the same game frames 4 Mbps was
  visibly blocky in motion (35.3 dB) and 8 Mbps measured 38.7 dB, with all 30 frames still coming out
  and the encode only ~2 ms slower (16 ms average, 21 ms worst case) — rate control spends more bits,
  it does not do more work. A desktop never reaches it: scrolling text sat at ~3 Mbps under every cap
  from 4 to 16, so the higher ceiling only costs bandwidth while the picture is moving. What bounds it
  from above is `SOCKET_BUFFER`: a 128 KB window carries 1 MB per round trip, so 8 Mbps still fits a
  ~128 ms RTT path, and every step past it shrinks the paths a share can cross without shedding.
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
  Ranks are handed out with `/server role <user> <role>` (owner only; the server refuses granting above
  your own rank, retitling yourself, or touching a peer or superior). A granted role applies to the
  session it lands in — the server updates the live `Connection` and tells that client, whose
  `App::role` is what the palette and `/help` read — so the per-connection role is re-read on every
  packet rather than latched at login.
- **A moderation target is an account, not a session.** `/server ban` and `role` take
  a username or a live session id, resolved to a username by `server::resolve_user` — the same lookup
  `/profile` uses — and then find the account's session, if it has one (`server::session_of`; there
  is at most one, since `user_connected` refuses a username already in `CONNECTIONS` at login).
  That is what lets an owner ban or retitle somebody who is offline, since an offline account has no
  id. A ban is answered with the ban list, as a pardon is, since an offline target leaves nothing
  else to show that it worked. `/server kick`, `banip` and `mute` are still by id: they only mean
  anything for somebody who is connected, and the server keeps no last-seen address to ban.
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
  - There is no migration for entries that predate the color and profile keys: `colors()` reads a
    missing key as no colour, and `write_user_field`/`write_profile_field` create the subtable the first
    time anything is stored for it.
- **A profile is the account's, and the client keeps none of it.** `/profile` opens your own and
  `/profile USER` somebody else's; the fields are `bio`, `pronouns`, `website` and `status`, plus the
  picture. Like the colors, the client only asks
  (`PacketCode::ProfileRequest`) and the server answers with the whole thing (`Profile`), so there is no
  second copy anywhere and nothing in `client.toml` about it. The ack to a save is the whole profile
  **again** rather than an "ok", which is what makes a refused description snap back in the row instead of
  sitting there looking applied — the same shape `ServerSettings` has.
  - **A profile is named, not numbered** (`server::resolve_user`): a username is what an account *is*,
    while an id is only a session it happens to have open — an account nobody is connected as has no id at
    all. So the parameter is resolved as a username first and as the id of a live session second, which is
    what makes `/profile alice` and `/profile 3` the same profile without an id ever being the only way to
    reach one.
  - **It lives in `server_users.toml`, in a `[user.profile]` subtable.** A file of its own would buy a
    second read path, a second lock and a second migration for what is the same kind of per-account text
    the colors already are. The nesting is what keeps it apart from the credentials: `password` sits in that
    same entry, so anything reading or writing "the profile" walks a subtable that cannot contain the hash.
  - **The overlay does it, in a third mode** (`settings::Mode`, which replaced the `server: bool` — two
    bools would be four states and two of them nonsense). An own profile is held until `[ Save ]`/Ctrl+S
    like the server rows; somebody else's is `readonly` — no button, no edit key, and the box says
    `Esc close` and nothing about changing anything, since the client is not the one who decides what it
    may see.
  - **A field is prose, so the row is a preview and the foot is where it is read.** The row shows the value
    truncated beside its label (and the caret while it is typed), while `draw::description_lines` puts the
    whole thing through `state::wrap_line` into the description foot — the same foot the server's comments
    use, sized for the longest, so the text gets the box's width rather than one truncated row of it. An
    empty field says so in the foot (`No pronouns.`, `profile.empty.<key>` in the locale), falling back to
    one built from the label for a field the locale does not name.
  - **`UserProfile::KEYS` is the only place the fields are spelled.** The wire struct, the `[user.profile]`
    keys (`config::users::PROFILE_KEYS` *is* that list), the rows the box is built from and the server's
    per-field checks all walk it in the same order, so a new field is one entry, one struct field and one
    `field_mut` arm — and the file, the packet and the box cannot disagree about what a profile has.
    The row labels come from the locale (`profile.<key>`, so `bio` says `Description`); a key the locale
    does not name is shown capitalised, which keeps a new field one entry here and nothing else.
  - **`status` is the stored line, not presence.** What somebody is up to *until they change it* belongs on
    the account like the rest of the profile; online/away/DND does not — it dies with the connection, so it
    would live on `Connection`, ride on `OnlineUser`/`Join` and belong in the sidebar rather than in a box
    somebody has to open. Putting presence in the TOML would leave a crashed client "online" in a file
    forever.
  - **A website is refused where it is typed** (`settings::commit_edit`), the way `/color` refuses a colour
    a code cannot carry rather than storing it and ignoring it later. The scheme rule is
    `misc::is_web_url` — `http`/`https` only, since the value is eventually handed to a system opener —
    and it lives in the library because the server has to apply the same rule to a client's word and
    `state::url` (a clicked link in the pane) is the same question asked of a typed word. The length bound
    stays with each caller: `MAX_URL` for a word in the pane, `max_profile_field` for the stored field.
  - **`⏎` on somebody else's website opens it**, which is the one thing a read-only profile still does.
    There is no mouse hit-testing inside the overlay, so a click cannot reach a row — but the selection
    already names one, so the keyboard needs no rects; it goes through the same `tui::open_link` and
    `Opening {url}` toast a clicked link in the pane does.
  - **What a client may store is bounded at the door.** `profiles` (server.toml) switches the whole thing
    off with `InvalidFeature`; `max_profile_bio` and `max_profile_field` are **refused rather than
    truncated**, and control characters are stripped where the packet arrives, since a row is one line and
    the foot is wrapped. A refusal is **all four fields or none** — the save is one packet and the ack is
    the whole profile, so storing the fields that happened to fit would leave the box showing a profile the
    server does not hold. The keys are live-read, the request is charged to the packet bucket like anything
    else, and the log gets an address, a field name, a character count and `own`/`peer` — a field's *name*
    is the server's own vocabulary, its content is the user's and never reaches the log.
  - **The picture is a hash, not a field, and it is the chat pictures' own machinery.** `UserProfile::avatar`
    is an `Option<[u8; 32]>` sitting deliberately *outside* `KEYS`: the four fields are prose somebody types,
    checked by length and refused together, while a picture is bytes that had to be uploaded first. So
    `ProfileSave` never carries it (`set_profile` writes the typed fields only, and the server ignores
    whatever avatar a save arrives with). It is set from the box itself: an own profile opens on an `Avatar`
    row (`settings::Value::Avatar`) that is typed as a path, completed off the disk by the same
    `palette::paths` `/image` uses (listed in the description foot, ↑↓ to pick, Tab to take it), checked
    where it is typed by the same `check_upload` `/upload` and `/image` go through, and held until
    `[ Save ]` like the fields — clearing it drops the avatar. On save it is its own request
    (`take_avatar_save`), sent beside `ProfileSave` rather
    than in it, and only the half that actually changed goes out. There is no `/avatar` command. What it names is an ordinary stored picture in `server_images/`, keyed by content like any
    other, so the whole path already exists: `AvatarRequest` mints an upload token
    (`ConnectionType::Avatar`) exactly as `/image` does, the client fetches it back through `ImageDataRequest`
    and the cache, and a picture that is *already* stored costs no upload at all (`ImageDuplicate`, the same
    answer `/image` gets).
    - **The retention rule is what had to change**, and it is the one thing a second copy would have bought
      instead. `server_images/` was owned by the history alone — a picture died with its entry — so an avatar
      kept there is a picture two different things name. `config::messages::stored` is that predicate
      (`has_image` **or** `users::names_avatar`), and it is what the fetch arm serves on and what an upload
      dedups against; `push`'s orphan filter and `sweep_images` both keep what a profile names. That is also
      why setting or dropping an avatar sweeps: the picture it *replaced* may have nothing left naming it.
    - **The ceiling is `MAX_AVATAR_SIZE`, and it is checked on both roads in.** An upload is refused over it
      in `file/server.rs` like an oversized image, and naming an already-stored picture is refused the same
      way (`file::image_size`) — otherwise an 8MB chat picture could be pinned as an avatar forever, which is
      the one thing the dedup shortcut would let a client do for free.
    - **An avatar is a square, cut by the client and only checked by the server.** Cropping rather than padding,
      because a fill colour is a background the file would carry into every terminal theme. The client does the
      work (`network::client::image::make_avatar`): the centred square, scaled to at most `AVATAR_DIMENSION`
      (`ANIMATED_AVATAR_DIMENSION` for an animation, which is every frame at once), written as a PNG or, when it
      moves, a GIF. It goes through `decode_image`, so the decode is bounded like any picture's, and the
      source is allowed `MAX_IMAGE_SIZE`: `MAX_AVATAR_SIZE` is checked against the *cut* copy, which is
      parked in the temp dir (`misc::avatar_temp`) because the upload sends from a path, and removed once it
      is sent or the server already has it. A failure there comes back as `ClientEvent::AvatarFailed`.
      **The server decodes nothing.** Cutting there would mean decoding untrusted pictures on the server, and
      re-encoding one changes the hash it is named, keyed and MAC'd under. It checks the shape off the header
      instead (`misc::is_avatar` — a PNG's IHDR or a GIF's screen descriptor, which is why the client only ever
      writes those two), on the first chunk of an upload and on the stored copy when the dedup shortcut names
      one — a chat picture was never cut, so that road would otherwise pin any shape as an avatar.
    - **It is answered with the whole profile, from wherever it lands.** The `AvatarRequest` arm answers the
      two cases it can settle at once; an upload's answer comes from `file/server.rs` when the last chunk is
      in, which is why that path sends `Profile { save: true }` rather than announcing anything — an avatar is
      the only picture on the server that is *not* pushed to a channel, since nobody sees it until they open
      the box.
  - **The picture is drawn inside the box, and the box reserves the rows before it exists.**
    `Settings::picture` holds the fitted picture, `picture_of` which hash it is (so a save's ack does not put
    down a picture it is about to want again), and `picture_area` where the draw path put it;
    `Settings::picture_rows` is what `draw_settings` takes off the rows' room. It is drawn in `draw_avatar`,
    **after** `draw_pictures` — a graphics-protocol picture is not cells, so it has to go on last, the same
    reason the pane's do, and being inside a box that is itself restored over the pane's pictures means it
    must go on after that restore. `AVATAR_ROWS` is its budget, which is why `fit_size`/`fit_image` take a
    row count instead of reading `IMAGE_ROWS` themselves, and `App::advance_avatar` steps it off the same
    tick as the pane's animations — an animated avatar plays, the pane's clock simply does not reach into the
    overlay.
    - **A pane picture sharing a terminal row with it takes that row away.** A picture is sent one escape per
      row, and a pane picture's row is written across the whole of its width — straight through the box and
      over the avatar's cells — whenever that row is sent again (an animation frame, a box that moved). The
      avatar's own first cell has not changed, so the diff never sends it back and the avatar comes out with
      rows missing. `draw_pictures` therefore returns the rows whose first cell differs from the last frame's
      (`App::picture_rows_drawn`), and `draw_avatar` marks the same rows of the avatar so they go out after
      them — the avatar is further right, so within the row it is written last. The mark alternates between
      one and two cursor saves (`App::avatar_marks`), because a pane GIF rewrites its row every frame and the
      same mark twice would read as unchanged.
- **The server icon is an avatar the server owns** (`/server icon [PATH]`, owner only; no path drops it).
  It is cut by the same `cut_avatar`, checked by the same `is_avatar` and `MAX_AVATAR_SIZE`, stored in
  `server_images/` like any picture, and fetched back through `ImageDataRequest` and the cache — only the
  owner differs: the hash lives in `server_icon` in the config root (`config::server_icon`), not in a
  profile, and `messages::stored`, `orphans` and `sweep_images` keep it for that reason.
  - **It is asked for, not pushed.** `Accept` does not carry it; a client that wants it sends
    `ServerIconRequest`, and the library's `listen_server` deliberately does not, so WHY2-Desktop (which
    asks for itself, as `ClientEvent::ServerIcon`) is not asked twice. The TUI asks from the redraw tick
    once `Authenticated` lands (`App::icon_request`). A change goes to every client as
    `ServerIcon { save: false }` (the setter gets `save: true`, which is the only one the TUI also prints).
    It is a hash only, so the picture itself is fetched like an avatar's: `App::set_server_icon` puts the
    hash in `image_loads` (cache first, then `ImageDataRequest`), and the `ImageData` arm hands it to
    `deliver_icon` before the avatar/caption routing — one picture can be all three, which is why
    `ImageFrame` is `Clone`.
  - **The TUI draws it as the top panel of the sidebar**, titled with the server's name, `ICON_ROWS` tall,
    and only once it has decoded and the sidebar has `ICON_MIN_HEIGHT` rows — until then, and for a server
    with no icon, the sidebar is what it always was. `draw_sidebar` returns where the picture goes
    (`App::icon_area`), and **the picture itself goes through `draw_pictures`**, as one more entry beside
    the pane's placements: the settings box can reach over the sidebar, and everything about restoring a
    box over a picture row (the copied-out cells, `replace_rows`, `resend_rows`) applies to it unchanged.
    It never shares a row's escape with a pane picture, since the pane ends where the sidebar starts. It is
    stepped from the same tick as the avatar (`advance_icon`, only while it has an area), and the avatar's
    load and step are the same two functions (`load_fitted`, `step_fitted`) at a different row budget.
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
    it, what they said and the picture if it was one; the colors are looked up per sender in `page()`
    and put on the wire `StoredMessage` there. So a replay is painted the way the sender looks **now**
    — somebody who recolours themselves recolours everything they ever said, which is what the colors
    living on the account rather than in the packet means once there is a file involved. It also means
    the file cannot hold a colour that no longer exists and `/color` never has to rewrite a history.
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
    history rather than refusing to start. An older format is unreadable like any other.
  - **A message can name the message it replies to** (`reply: Option<u64>`, a message id, on
    `MessageRequest`, `Record`, `StoredMessage`, `PacketCode::Message` and `ClientEvent::Message`), set by
    `/reply ID MESSAGE` (`/re` is still the private-message answer). The server refuses a reply naming a
    message `messages::exists` does not know with `InvalidUsage` — so a reply can only name a stored lobby
    message, and with `persistent_messages` off every reply is refused.
    - **The TUI resolves the target at wrap time, not when the reply arrives** (`App::rewrap` indexes the
      pane by message id and hands the target to `Theme::render`). A target that arrives later (an older
      history page) or goes away (`/delete`) is therefore picked up on the next rewrap for free; one that
      is not in the pane is drawn as its `#id`. The quote is **always exactly one row**
      (`Theme::reply_row`, cut with `…`), which is what keeps `prepend_history`'s and `remove_entry`'s row
      arithmetic exact while a quote changes under them. A reply to one of our own messages is tinted like
      a mention.
  - **Every message has a server-assigned id** (`Record::id`, `StoredMessage::message_id`,
    `PacketCode::Message::message_id`), which is how a message is named from outside — deleting one is
    what it is for. It is not the sender's `id`, which is a session. One counter serves the lobby and
    every channel (`History::next`, seeded from the last stored id + 1), and it is taken **under the
    `HISTORY` lock** — `store` allocates the id itself and `next_id` covers a message that is not kept —
    so the history's ids rise in the order its records do. Unique per process plus the stored history is
    enough: a restart ends every session and a client's panes die with it, so an id reused for a channel
    message after a restart can never match anything a client still shows.
  - **A message carries when it was sent, as the server saw it** (`timestamp: Option<u64>`, unix seconds, on
    `Record`, `StoredMessage`, `PacketCode::Message` and `ImageDisplay`). It is taken once per message by
    `messages::timestamp()` and the same value goes on the wire and into the record, so a replay shows the
    time the live line did. `message_timestamps` (server.toml, live) turns it off, and then it is `None`
    everywhere — `page()` strips a stored one too, so turning it off hides the old ones without touching the
    file.
    **The ids stay the order**: the history is never sorted by time, so a clock that jumps cannot reorder
    it or break `History::position`. The client formats it in local time (`Theme::timestamp`, `chrono`),
    with the date only when it is not today, behind `show_timestamps` (client.toml, a `/settings` row).
    A file starts with `MAGIC`, which is how it is recognised — not
    `deserialize_exact`: the decrypted plaintext is padded to whole grids, so exact decoding rejects every
    history, and doing that once emptied a real one. A history that will not parse is copied to
    `server_messages.bin.old` before it is ignored, since the next message would otherwise overwrite it.
  - **A stored message can be deleted** (`/delete ID` → `PacketCode::DeleteRequest`,
    `config::messages::delete`): your own, or as a moderator one by a lower rank than yours — the same
    "no peer or superior" rule kick and mute use, with the author named by the record's username, so
    it survives a reconnect. It is removed from `HISTORY` and the file is rewritten, and a picture
    nothing else names goes with it. A refusal (no such message, or not yours to delete) is
    `InvalidUsage`. A channel message, text or image, is never stored and so cannot be deleted.
    - **A success is broadcast to every client** as `PacketCode::Deleted { message_id }` — only the
      lobby is stored, and every client holds a lobby pane, live or parked — and
      `App::delete_message` removes the entry outright rather than leaving a tombstone. In the pane
      being looked at, `App::scroll`, the selection and `history_anchor` move up by the rows it took
      (the wrap cache keeps each entry's first row for exactly this); in the parked lobby pane only
      the anchor needs it.
    - **A stored message can be edited by its author and nobody else** (`/edit ID MESSAGE` →
      `PacketCode::EditRequest`, `config::messages::edit`). Unlike deleting, rank gives no right to it: a
      moderator may remove somebody's words, but putting new ones in their mouth is not moderation. Only a
      text record can be edited (an image line's text is the filename), and an empty edit is refused —
      that is what `/delete` is for. The new text goes through the same control-character strip,
      `max_message_length` and `min_message_delay` as a typed message, since it is fanned out to every
      client like one. A success sets `Record::edited` and is broadcast as `PacketCode::Edited`;
      `App::edit_message` rewords the entry in place (sharing `update_message` with the hearts, which
      shifts the scroll and selection by the rows it gained or lost) and the trailer shows a dim
      `(edited)`. A reply quoting it picks up the new text on the rewrap the generation bump causes.
    - The id is drawn as a dim `#N` right-aligned on the last row of the entry (`Theme::message_id`,
      a row of its own when the last one has no room), on live and replayed lines alike, image
      captions included — `ImageDisplay` and its four events carry it (`show_message_ids`,
      client.toml, default on, a `/settings` row). It trails the line rather than leading it so the
      timestamp and the username line up down the pane.
  - **Every other chat entry is striped** (`App::stripe_bg`, a background only), which is what
    separates one message from the next now that a wrapped one is several rows. The tint is per
    wrapped row in the wrap cache (`App::tint`), beside the mention tint it replaced — a mention wins
    over a stripe. Only `Entry::striped` kinds count, so the client's own output (`/help`, transfers)
    neither takes a stripe nor shifts the parity. `App::stripe` flips for each striped entry trimmed
    off the top at `HISTORY_LIMIT`; counting from the top alone would flip every stripe in the pane
    on each new message once the pane is full. `draw_logo` treats the stripe as unpainted, or the
    watermark would vanish from every other message.
    **The stripe is the one chrome colour taken from the user's scheme**, on purpose: a shade has to
    be relative to the background it sits on, and any fixed `Rgb` is a colour cast on somebody's
    terminal. `init_picker` asks for the background (OSC 11, ratatui-image's
    `terminal_background_color_osc`, in the same query it already makes) and `theme::stripe` moves it
    `STRIPE_LIFT` towards white, or towards black on a light background. A terminal that does not
    answer gets `STRIPE_FALLBACK`. `message_stripes` (client.toml, default on, a `/settings` row)
    turns them off.
  - **Only the lobby has one.** A channel exists exactly as long as somebody is in it, so there is
    nothing to keep it against; `server::listen_client`'s `Message` arm stores only while
    `channel.is_none()`.
  - The append-trim-write runs under the one `HISTORY` lock, so two clients talking at once cannot
    drop each other's message or leave a half-written file behind.
  - **The history is sent a page at a time** (`send_history`, `messages::page`): the newest
    `history_page` messages right before `Accept`, and each older page when the client asks for it
    with `HistoryRequest { before }`. Every client starts in the lobby, so a channel switch has nothing
    to replay. A page is also cut at `MAX_HISTORY_SIZE` bytes, but always holds at least one message
    so the cursor always moves.
    - **The cursor is a message id**, found by binary search (`History::position`) since the ids rise
      with the records. It stays pointing at the same message while newer ones arrive, older ones are
      trimmed and one in the middle is deleted — an offset from either end, or a position, would not.
      The client treats it as opaque and only hands it back.
    - **The client asks when the top of the lobby pane comes within `PRELOAD_SCREENS` + 1 screens**
      (`App::load_visible`) — the same lookahead the pictures use, so an older page is usually in
      before the reader hits the top and waits on it — one page in flight at a time (`history_pending`), sent by the redraw
      tick. `App::history_anchor` is where the first replayed entry sits, so an older page goes in
      *under* the `Message history (n):` heading rather than above the connect lines; `n` is the
      number the server keeps, not the number shown. `prepend_history` moves `App::scroll` and the
      selection down by the rows it added, so the view stays on what it was showing. Paging stops at
      `more: false`, at the pane's `HISTORY_LIMIT`, and once the anchor itself has been evicted; an
      answer that lands while we are in a channel is dropped, and asked for again on the way back.
    The packet keeps the username, the text and the colors, but **no id**: the session
    that said it is gone and whoever holds that id now is somebody else. The client replays it as
    `state::Entry::History` — an ordinary chat line rendered through `Theme::render` (so
    `disable_colors` reaches it like any other message) minus the id column, under a
    `Message history (n):` heading that is what separates it from what is being said now.
  - **An image line carries the sender's username color, the way a message does.** It is the username
    color *only*: an image line's text is the filename, which is the client's own wording and not
    something the sender typed, so there is no message color to keep and the packet's stays `None`.
    It is looked up where the line is built — `page()` for a replay, and
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
  the redraw interval, popup row counts, the markup/math limits. A new one goes
  there rather than at the top of the file that reads it: half of them are read by `draw.rs` as well
  as by the module that owns the behaviour, and as file-local consts they were being reached for
  across modules (`palette::MAX_ROWS`) which is a const module with extra steps.
  What deliberately stays out is anything that is not a knob: `theme.rs`'s palette, `math.rs`'s
  symbol tables, `command.rs`'s command list, and the `include_str!`s (`draw.rs`'s logo and the
  viewer's `.wgsl` shader).

  **Every word the client shows lives in `chat/locales/en.toml`, not in the source** (`i18n.rs`,
  `client_base`). Code asks for it by key — `t!("event.joined", username)` fills `{username}` and
  returns a `String`, `t!("key")` alone a `&'static str`, `tn!("tui.copied", lines)` picks a plural
  form by count and binds it as `{count}` — so new user-facing text is a key in that file, never a
  literal. English is embedded and is the fallback for every key; `language` in client.toml picks
  another, read from `<config dir>/locales/<language>.toml` first and from the built-in `BUILTIN`
  table second (a shipped translation is one file plus one line there). Every built-in locale
  except `en` and `cs` is machine-translated, and says so with ` [AI]` on its `meta.name`, which is
  what the `Language` row shows; a human-checked translation drops the suffix. It switches live from the
  `/settings` `Language` row (`i18n::set_language`): each locale is parsed once and leaked, so `t!` can
  keep handing out `&'static str`, and the switch relabels the box and rewraps the pane through
  `App::reload_theme`. Lines already pushed as `Entry::Line` keep the language they were written in. Key rules:
  - **A placeholder is a whole sentence's**, never glued from fragments: "Invalid usage!" and "Invalid
    action!" are two keys, not "Invalid {thing}!", because other languages inflect the noun. A styled
    span inside a sentence (an image caption's username) is placed with `i18n::split` around the
    placeholder rather than by assuming it comes first.
  - **Padding and glyphs stay in the code**: a box title is stored as `Connect` and drawn as ` Connect `,
    a toggle as `on` and drawn as `● on`, and label columns are measured from the translated text
    rather than padded to an English width.
  - **What people type or the server sends is not translated**: command triggers, colour and role names,
    `@everyone` and `server.toml`'s own keys and comments. The command table (`command.rs`) holds keys
    for the descriptions and argument names only, resolved where they are drawn.
  - Diagnostics that never reach the TUI (the GPU converter's and the viewer window's fallback reasons,
    `expect` messages) stay as literals.

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
  - **A picture is handed to the terminal last, and a box is put back on top of it**
    (`draw::draw_pictures`, called at the end of `draw` after every overlay). A graphics-protocol picture is
    not cells: ratatui-image writes one escape into the **first cell of each row**
    (`CellDiffOption::ForcedWidth(1)`) which draws that whole row of kitty unicode placeholders, and marks
    every other cell of the row `Skip` so nothing overwrites them. That is two things the cell diff cannot
    see, and both were bugs:
    - A box drawn over a picture erases the placeholders it covers — kitty drops a placement the moment its
      placeholder cells go — and **the diff cannot put them back**: the picture's first cell is unchanged
      from the frame before, so nothing is re-emitted. The picture kept the hole and the box's glyphs sat in
      it until the terminal was resized, which re-fits every picture and repaints the screen, which is why
      resizing was the only cure.
    - And anything that re-rendered the picture *while* the box was up — the pane scrolling because a line
      arrived (`Profile saved.` does exactly that), or an animation frame — wrote the whole placeholder row
      straight across the box, whose cells are unchanged in the buffer and so are never rewritten. The box
      came back eaten wherever a picture was behind it.
    **Suppressing a covered picture is not the answer** — it was tried, and hiding a whole picture because a
    box clips a corner of it is worse than the artefact it avoids. So the picture is always drawn, and what
    the box has on those cells is copied out first (`overlay_cells`) and written back over it afterwards.
    A picture therefore shows everywhere the box is not, which is all a row-at-a-time protocol allows: the
    cells to one side of a box are the row's own, and the ones under it are the box's.
  - **The box's cells are written back with `AlwaysUpdate`, and the picture's row with a changed symbol.**
    Those are not interchangeable, which is the whole subtlety:
    - The restored box cells are unchanged in the buffer, so the diff would skip them and the picture's row
      write — which *has* changed — would rub the box out. They are ordinary single-width text, so
      `AlwaysUpdate` is exactly right, and the first cell of a picture row is a lower column than any of
      them: within one frame the row is written and the box then lands on top of it.
    - The picture's own first cell must **not** use `AlwaysUpdate`. That option makes the diff measure the
      cell from its symbol, and the symbol is an escape sequence tens of columns wide, so the diff skips the
      rest of that row — which is precisely where a box that has just closed left its glyphs. `replace_rows`
      prepends a second `\x1b[s` instead (saving the cursor twice is the same as saving it once), so the cell
      *reads* differently, is written again under its forced width of 1, and the rest of the row is still
      diffed: the row's own write clears what the box left inside the picture's columns and the ordinary diff
      clears what it left outside them.
  - **`App::overlays_drawn` hands back the previous frame's rects** as it stores this frame's, which is how
    "a box was over this picture and is no longer" is known — the only case that needs the row replaced.
    That includes a box that is **still** over it but has changed shape: a box that shrinks (the profile
    box's path list emptying as a path is typed) leaves glyphs on the cells it gave up exactly like one
    that closed, so the replace fires whenever the rects differ, not only when none cover the picture any
    more — and it skips a row whose first cell is still the box's, since that row's picture is not drawn.
    **One frame is not enough for that**: the rows written on the frame the box changes shape still leave
    its old glyphs up, and it is the frame *after* — where the first cell reads plain again and is emitted
    once more — that clears them (which is why a focus change, a bare redraw, used to be the cure). So
    `draw_pictures` sets `App::dirty` whenever the rects changed, and the next tick draws that frame.
    The rects come from the boxes themselves: `draw_palette`/`draw_settings`/`draw_login`/`draw_tofu` each
    return the popup they drew (`Rect::ZERO` when the terminal had no room for one) and `draw` collects them.
    Recomputing that geometry would be a second copy of arithmetic that depends on the rows, the description
    foot and the terminal size, and the two would drift.
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
  - **Capturing the mouse takes the terminal's URL click away too, so the client opens them itself**
    (`App::link_at`, `tui::open_link`). The terminal still *detects* a URL under the pointer — the hover
    underline is its own — but while mouse reporting is on it never receives the click, so ctrl+click
    does nothing; shift+click, which every terminal reserves as the bypass, is the one path that still
    reaches it. A click that was not a drag therefore resolves the cell to the whitespace-delimited word
    it is on (`word_at`, over the same wrapped rows the selection slices) and hands it to
    `xdg-open`/`open`/`start` with its output on `/dev/null`, since anything it printed would land on the
    frame. **It is the word that decides, not the markup**: a URL somebody simply typed is the common
    case and is clickable exactly like a `[text](url)` one, which is also why `markup` draws a link's
    target rather than hiding it. Only `http`/`https` are opened — every other scheme is somebody else's
    text handed to a system opener — and the punctuation a link only leans on is trimmed, except a
    closing bracket the link opened itself (`…/Foo_(bar)`). Over SSH this opens on the far machine, the
    way a clipboard library would have written to the far clipboard; shift+click is the local path.
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
    choice — and even those are fixed under a palette that carries an `ansi` table (`theme::ansi`).
  - **The chrome palette is switchable, so the colours are accessors, not consts** (`theme::dim()`,
    `theme::accent()`, …, read from `theme::PALETTES[ACTIVE]`). `theme` in client.toml stores a palette's
    `id`, `Theme::load` selects it (an unknown id is the first entry, `why2`, the original look), and the
    `/settings` `Theme` row cycles it live through `App::reload_theme`. Every palette **paints its own
    background** (`theme::base()` over the frame and each popup), since its foregrounds are only readable
    on the background they were designed for; `background: None` (the terminal's) is still supported but
    no built-in palette uses it. **`why2` is absolute**: its background and its `ansi` table — the 16
    colors a `/color` code names — are a snapshot of the author's kitty scheme, so it looks identical on
    every terminal. The other palettes have `ansi: None` and leave user colors to the terminal. Anything
    that asks "is this cell unpainted" therefore has to accept `theme::background()` as well as
    `Color::Reset` — the row tints and the logo both do — and the stripe is derived from the palette's
    background before the terminal's. A new palette is one entry in `PALETTES`, still all `Color::Rgb`.
  - `tui/input.rs`'s `InputBuffer` is the single source of truth for the input line (there is no
    global partial-input state), and `tui/palette.rs` drives the slash-command popup straight off
    `command::COMMAND_LIST` — never duplicate the trigger table. The popup has two modes
    (`PaletteMode`): a filtered command menu while the command word is still being typed, and a
    single-row signature hint highlighting the parameter the caret is on once it is finished.
  - **A path parameter is completed off the disk** (`ArgValues::Paths`, `/upload` and `/image`). It is the
    one vocabulary that depends on what has been typed so far rather than being a fixed list, so
    `vocabulary` takes the half-typed value: everything past the last `/` is the name being matched and
    what precedes it is the directory that is read. A directory is offered with its separator on the end,
    which is what makes Tab walk into it — the completion re-runs `update`, and the next listing is the
    directory's own. Dotfiles are offered only once a `.` is typed, and `MAX_PATHS` bounds a directory
    nobody wants every entry of. The value is matched case-insensitively like every other one but is
    **read and inserted as it is spelled on disk**, so `hint` keeps the typed text raw and lowercases only
    for the comparison. `misc::expand_home` is what makes `~` mean anything — the listing and the upload
    itself both go through it, since a path the palette offers has to be one the command can open. What it
    cannot do is a path with a space in it: the parameter the caret is on is found by splitting on
    whitespace, the way every other value is.
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
    this — a channel switch asks for nothing, so the scrollback only exists client-side. A parked pane
    keeps filling while we are away, since messages and pictures reach every client tagged with their
    channel (`App::park_entry`, see `send_to_all` above).
    What keeps that bounded is the same rule the sidebar runs on: a channel exists exactly as long as
    somebody is in it, so `App::prune_panes` drops the parked pane of a channel that no longer has
    anybody in it (after every roster re-derivation, and in the `ChannelDestroyed` arm). The lobby is
    never pruned, and the pane being read is not in the map to prune. A lost session throws the whole
    map away in `App::disconnected` — unlike a switch, nothing there is coming back to.
  - The sidebar is fed by events, never by polling, and **the roster is maintained rather than
    re-asked**: `Join` and `Leave` each name a whole user, so the arms add and drop them in
    `App::online` themselves. That is what `Join`'s `id` is for — without it a join could only ask,
    and a join on an N-client server cost N roster walks and N full per-connection encrypted
    replies, since every client asked at once. A joining client is always in the lobby, so its entry
    is `channel: None`; `Leave` re-derives the channels from what is left.
    The one thing no event can carry is the roster we arrive to, and the server pushes that unasked
    right behind `Accept` (`send_list`) the way it already pushes the history and the voice clients,
    so the client asks for nothing at login either — `App::refresh_online` and the silent request the
    redraw tick drained are gone, and `/list` is the only thing left that asks. It is also the
    explicit resync, which is why the `List` arm always refreshes the sidebar and only *echoes* under
    `list_requested`.
    **Nothing else may quietly ask for one, and a channel switch least of all**: a `List` there
    would land inside the server's `min_message_delay` window right behind the `/channel` packet and
    earn a `SpamWarning` (three of those disconnect). **`Leave` must not either, for the same
    reason**: it is broadcast to the kicker as well, so a `/kick` would put the request directly
    behind its own `ServerKick` packet and warn the moderator for spam.
    The channel list is maintained from the globally
    broadcast `ChannelCreated`/`ChannelDestroyed` packets plus whatever the last `List` showed —
    a channel exists exactly as long as somebody is in it, so the lobby is not one and is not
    listed. (`ChannelDestroyed` is only sent on a `/channel` switch, never on a disconnect, so
    re-deriving in the `Leave` arm is also what retires a channel whose last member dropped.)
  - **The offline list is a second box, not a second kind of row in the first one**
    (`draw::draw_offline`, `App::offline`). An account nobody is connected as has no session id, no
    channel and no device — three of `OnlineUser`'s four fields — so it crosses the wire as
    `OfflineUser` (a username) in `PacketCode::List`'s own `offline` field rather than as a flag on
    an online entry. A sentinel id would be worse than redundant: the roster's dedup guard, `Leave`
    and `/pm` are all keyed on the id, and every one of them would have to learn which ids are
    fiction.
    - **It is `server_users.toml` minus whoever is connected** (`send_list`, `config::users::all`),
      gated by `show_offline_users`. What the gate protects is not cost but disclosure: with it on,
      connecting tells you every account that has ever existed on the server, not just who is here.
    - **`Join` and `Leave` maintain it the way they maintain the roster**, which works only because
      there is no guest path — `listen_client`'s auth either registers the user or verifies a stored
      password, so everybody in `CONNECTIONS` has an entry in the file and a leaver is by definition
      a registered user. A guest mode would break that `Leave` line specifically.
    - **The client has to know the difference between "none offline" and "not sent"**, which is why
      `ClientEvent::List` carries the `Option` rather than an `unwrap_or_default`: with the gate off
      the `Leave` arm would otherwise invent an offline user and materialise a box the operator
      turned off. `App::offline_listed` is that bit.
    - **`Online` is the box that gets sized and `Offline` takes the rest.** `max_clients` bounds the
      first, so it fits; the second is every account on the server and grows without limit, so it is
      the one that has to truncate. A registration by somebody else is invisible until they
      disconnect or somebody types `/list` — nothing is broadcast on register.
  - **The voice panel is the channel's roster, not our own voice session**, so somebody who never
    types `/voice` still sees who is in it. `PacketCode::VoiceJoin`/`VoiceLeave` (named
    `ChannelJoin`/`ChannelLeave` before — they were always the *voice* pair, while
    `ChannelCreated`/`ChannelDestroyed` are the text-channel one) go to every client, naming the channel
    they happened in, and a client drops the ones for a channel it is not standing in.
    What once made them invisible was the client, which handled them only under `client_voice` and only
    to add and drop audio consumers. Both arms now also raise a `ClientEvent`, and
    `PacketCode::VoiceClients` — the whole roster, self excluded — is sent on login as well as on a
    channel switch and on joining voice, so a client that never joins still learns who was already
    talking when it arrived.
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
  - **The typing indicator is a statement with a lifetime, not an event.** `TYPING_INTERVAL` (3s) is the
    fastest a client re-states it and `TYPING_TIMEOUT` (6s) is how long a receiver believes it; the timeout
    is deliberately twice the interval, so one dropped or throttled packet does not make the indicator
    blink. Both live in `consts.rs` rather than in `tui/consts.rs` — the pair is protocol, and the two sides
    only agree because the relationship holds.
    - **The key handler never sends it.** `InputBuffer::revision` is bumped by every content-changing edit
      and by nothing else (cursor motion, history recall and scrolling are not typing), `handle_terminal_event`
      compares it across one key event and calls `App::typed`, and that only raises a flag. The redraw tick
      is what sends, which coalesces a burst of forty keystrokes into one packet and keeps the socket off
      the key path — the same reason `image_requests` exists, the event loop owning the write half and the
      sequence counter. An overlay (the connect box, `/settings`, TOFU) never touches `App::input`, so the
      revision cannot move while one of them owns the keyboard.
    - **An empty line or a leading `/` is not a message**, and both *reset* `typing_sent` rather than merely
      failing to set the flag: a command is not something to announce (`/pm` least of all, which would tell
      a channel you are writing at one person), and clearing the line — which is what submitting does — has
      to leave the next burst free to send at once instead of waiting out the interval it never used.
    - **It is charged to the packet bucket like anything else.** At one per 3s it is nothing against
      `max_packet_rate`, so it needs no exemption, and being charged is what bounds a client that lies about
      the interval. `min_message_delay` does not reach it — that rule is `PacketCode::Message` alone.
    - **The server re-states the interval rather than trusting it** (`Connection::last_typing`, the
      `IMAGE_REQUEST_DELAY` pattern again, carried across a rekey and a channel switch for the same reason
      `last_image` is). What that bounds is not the client's cost but the **amplification**: a typing notice
      is fanned out to the whole channel with an independent REX encryption per recipient. It is dropped
      rather than held — a notice served late is a lie about the present — and a muted user broadcasts none.
      `send_to_others` excludes the sender, who does not need telling.
    - **Stopping is mostly free.** The `Message` arm clears that username on arrival — a message from
      somebody *is* the proof they stopped — `Leave` drops them like it drops the roster entry, and everything
      else (an emptied line, a `/`) simply expires. An explicit stop packet would double the traffic for a
      cosmetic gain.
    - **It is drawn in the pane's bottom border, never as a line**, beside the toast and the unread badge:
      it is not something that was said, and pushing it into `App::messages` would fight the wrap cache and
      the scroll offset. `set_typing` sets `dirty` only for a *new* name and `expire_typing` only when the
      set actually shrinks — a re-statement changes nothing on screen, and dirtying on it would repaint the
      pane every three seconds per typist.
    - `typing_indicator` gates **both directions** on the client (client.toml, default on, a `/settings` row):
      somebody who does not want to broadcast it does not want to receive it either. The server's key of the
      same name is live-read and refuses the broadcast outright.
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
    The markup reaches a private message too (`Entry::Private`) but deliberately not `Entry::Line`, which
    is the client's *own* output — `/help` and `/list` are not somebody's text and have nothing to parse.
    A private message is stored unrendered like `Entry::Message`, so `disable_colors` repaints it. Its
    `MessageColors` always describe the line as drawn: the name's colour belongs to the name shown, and
    the message colour to whoever wrote the text — so `PrivateMessageBack` carries the **recipient's**
    username colour beside the sender's message colour, and the client paints both unconditionally.
  - **The rest of the markdown is the same parser, which is what keeps it out of code.** Emphasis
    (`*italic*`, `**bold**`, `__underline__`, `~~strikethrough~~`, and `_italic_`) and `[text](url)` are
    delimiters in the *same* pass as the backticks and the dollars, not a second pass over the text
    afterwards — so a run inside `` ` `` or a fence is never seen at all, the fence having been consumed
    whole at its opening backtick. In the other direction an emphasis may still *span* a code span
    (``**bold `code` bold**``), which is why `find_run` skips a code or math span as one unit when it looks
    for the close: a delimiter found inside one would be consumed as code later and the emphasis would
    never close, leaking its modifier to the end of the message. That is also the whole rule for opening —
    a run opens **only when its close has already been found** (it is recorded with the position it will
    close at, and the parser emits the `Close` when it reaches exactly that index), so `*unclosed` is an
    asterisk somebody typed. Emphasis is a `Modifier` on a stack rather than a colour, so it composes with
    the sender's own colour, with a heading and with inline code instead of replacing any of them, and
    `_` alone demands word boundaries on both ends — `snake_case_word` is a word, not three italics.
    A run is at most `MAX_RUN` delimiters long, which is not cosmetic: measuring the run unbounded at
    every position makes a message of nothing but asterisks quadratic.
  - **The line-level markdown is applied where a row starts, not where a delimiter is found** (`marker`).
    Headings (`#`…`###`), `>` quotes, `-`/`*`/`+` and `1.` lists and `---` rules are only markers at the
    head of a row — so `` `x` # y `` is a hash somebody typed — and the parser cannot decide that on its
    own, since it does not know which text run begins a row. `render` tracks it instead: a marker is taken
    only while nothing has been drawn on the row yet, and a heading restyles the **whole** row rather than
    the run it was found in. A heading with no colour of its own takes `theme::HEADING`, the same rule
    math runs on, so a coloured message stays the sender's colour throughout.
    - **A marker also owns the rows its line wraps onto**, which is what `flush`'s hanging prefix is for:
      the quote bar or the list indent is repeated in front of every wrapped row and the wrap width comes
      down by its width, so a quoted paragraph reads as one quote instead of one bar and then loose text.
      An escaped marker (`\#`) comes back as `Segment::Raw` rather than as text, which is what stops it
      being re-read as a marker on the way out — the backslash is gone by then, so nothing else could
      tell the two apart.
    - A link is drawn as its text with the target dim beside it. There is no OSC 8 here and no clickable
      cell, so hiding the URL behind the text would be hiding where it goes; a link whose text *is* its
      URL is shown once.
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
(`PacketCode` enum) and both `network/client/mod.rs` and `network/server/mod.rs` (or the relevant
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
