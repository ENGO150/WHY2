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
(`cargo build --release` + the server feature combo above). The one exception is the screen
capture colour conversion, which is checked against openh264's own CPU conversion:

```bash
cargo test -p why2-chat --features client_screen --lib gpu:: --release
```

That test **passes trivially on a machine with no GPU** — `GpuConverter::new()` returning `Err` is
the case the CPU fallback exists for, so it returns rather than failing. Do not "fix" it into a
hard failure.

The screen share's echo canceller is the other exception, and needs no hardware at all — it drives
a synthetic loopback (a known delay and gain) past the canceller and measures what came out:

```bash
cargo test -p why2-chat --lib --release aec:: -- --nocapture
```

Run it with `--release`: the search correlates a few hundred milliseconds of audio at every lag and
is far too slow to sit through in a debug build.

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
  - `WHY2_CAPTURE_BACKEND` (`recorder` / `legacy`) pins a backend; `WHY2_CAPTURE_PROBE_TIMEOUT`
    overrides the probe deadline in seconds. Both exist so a machine where the heuristic picks
    wrong is one env var away from the old behaviour.
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
    version correlated block-energy envelopes first and refined the best few — it was cheaper, but
    the envelope of a loud share swamps the echo's, and it failed exactly when it was needed.
  - **The NLMS step is deliberately tiny** (`AEC_STEP`). The search hands the filter a least-squares
    gain at the right lag, so it only has to track drift, while the shared audio sits in the error
    signal as a loud disturbance that a large step turns into weight jitter. Raising it makes things
    worse, and measurably: against a share twice as loud as the echo, 0.002 removes 21 dB,
    0.0005 removes 30 dB and 0.0001 removes 34 dB. Cancellation degrades gracefully from there as
    the share gets louder relative to the voice — 40 dB at parity down to 16 dB at eighteen times
    it — while the damage done to the shared audio stays flat at about -44 dB throughout.
  - **Every failure degrades instead of breaking.** No voice session means an empty ring, which
    reads as silence and subtracts nothing; a voice output device that is not the monitored sink
    leaves our audio out of the capture entirely and the filter converges to zero on its own; and
    while the delay is unknown, or the running ERLE check finds the filter adding energy rather than
    removing it, the capture is passed through untouched rather than damaged. `WHY2_SHARE_AEC=off`
    disables it.
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
- **`config/mod.rs`** — TOML config for client (`client.toml`) and server (`server.toml`), plus
  server user store (`server_users.toml`) and server keypair storage
  (`server_keys/{private,public,private_pq,public_pq}`), all under `WHY2_CONFIG_DIR` (defaults to
  `~/.config/WHY2`, baked in by `build.rs` unless overridden at build time).
- **`bin/client/`** — the client entrypoint (`mod.rs`), the full-screen TUI (`tui/`, ratatui over
  the crossterm backend), and color handling (`colors.rs`).

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
  - C libraries that write to fd 2 (cpal/ALSA, openh264/xcap) corrupt the frame; the existing
    `gag::Gag::stderr()` wrappers in `network/voice/client` must stay.
  - `tui::install_panic_hook` is called first thing in `main` and is **not optional**: the release
    profile sets `panic = "abort"`, so `TerminalGuard::drop` never runs on a panic and the hook is
    the only path that leaves the alternate screen.
  - Fatal events (`TofuError`, a user-asked-for `Quit`) do not `process::exit` from the draw path. They call
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
  - Chat messages live in `App::messages` as `state::Entry::Message` (username/id/text/colors), not as
    rendered `Line`s — `Theme::render` turns an entry into a line on every wrap, so a `show_id` or
    `disable_colors` change repaints the messages already in the pane. Anything that rewrites
    config-driven styling must call `App::reload_theme` (which bumps the wrap-cache generation), never
    `Theme::reload` on its own.
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
