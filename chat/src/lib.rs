/*
This is part of WHY2
Copyright (C) 2022-2026 Václav Šmejkal

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

//! # WHY2 Chat
//!
//! `why2-chat` is the reference chat application built on top of the [`why2`] REX cipher.
//! It carries text, pictures, files, voice and screen sharing over a custom protocol, and
//! demonstrates the cipher in a real, adversarial setting rather than in isolation.
//!
//! ## Crate Layout
//!
//! The library holds both halves of the protocol:
//!
//! - **The client side** — dialing and the handshake, the session loop that turns packets into
//!   [`ClientEvent`](network::client::ClientEvent)s for a front end, the image cache, slash
//!   commands, translations, and the voice and screen-sharing clients.
//! - **The server side** — accepting and authenticating connections, broadcasting, rate
//!   limiting, the account, ban and history stores, and the voice and screen-sharing relays.
//!
//! Everything they share — the packet codes, the framing, the key exchange and the
//! cryptography — sits beside them, so a third-party client (such as WHY2-Desktop) can speak
//! the protocol without reimplementing it.
//!
//! ## Feature Flags
//!
//! Almost every module is gated:
//!
//! | Feature             | Enables                                                             |
//! |---------------------|---------------------------------------------------------------------|
//! | `client_base`       | Core client networking, the image cache, commands and translations. |
//! | `client_voice`      | Voice chat, voice messages and attachment media playback (on top of `client_base`).            |
//! | `client_screen`     | Screen sharing (implies `client_voice`).                            |
//! | `client`            | All client features (the default).                                  |
//! | `server`            | The server half of every protocol.                                  |
//!
//! The client features and `server` are mutually exclusive; `build.rs` refuses the
//! combination. These docs are the one exception: they are built with both halves enabled, so
//! every module appears here, while a real build has one or the other. The internal `chat`
//! feature is enabled by any of them and must never be enabled directly.
//!
//! ## Security Model
//!
//! Every packet on the main connection is authenticated encryption: either one-shot
//! [`why2::auth::AuthenticatedData`] under the session keys, or a counter-bound
//! encrypt-then-MAC stream ([`crypto::RexPacketStream`]) on the auxiliary sockets. The
//! session keys come out of a hybrid P-521 + ML-KEM-768 key exchange (see [`crypto::kex`])
//! and are rotated every [`consts::REKEY_INTERVAL`] seconds. Server identities are pinned
//! trust-on-first-use.

pub mod colors;
pub mod config;
pub mod consts;
pub mod crypto;
pub mod misc;
pub mod network;
pub mod options;
pub mod role;

#[cfg(feature = "client_base")]
pub mod cache;

#[cfg(feature = "client_base")]
pub mod command;

#[cfg(feature = "client_base")]
pub mod i18n;
