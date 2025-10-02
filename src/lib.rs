/*
This is part of WHY2
Copyright (C) 2022-2025 Václav Šmejkal

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

//! # WHY2
//!
//! WHY2 is a modern, fast, and secure encryption crate designed for privacy-first applications.
//!
//! The WHY2 encryption algorithm is loosely inspired by AES, but with a twist. Instead of relying on S-boxes,
//! WHY2 uses a nonlinear ARX-style transformation (Addition, Rotation, XOR) for diffusion. Input and key data
//! are formatted into grids with customizable dimensions. The key is shuffled, seeded, and expanded into round keys,
//! which are then applied to the input grid across multiple rounds.
//!
//! WHY2 also powers a minimalist chat application built for maximal privacy. It is designed for self-hosting
//! by individuals or small groups — not for large public chat rooms.
//!
//! ## Features
//! - Grid-based encryption with customizable layout
//! - ARX-style nonlinear mixing instead of S-boxes
//! - Round-key generation from seeded, shuffled keys
//! - Lightweight encrypted chat backend for private deployments
//! - Maximal customization
//!
//! ## Philosophy
//! - **Privacy is a right**, not a subscription feature.
//! - **No government insight**: no telemetry, no backdoors, no metadata leakage.
//! - **No payment required**: encryption should be free as in freedom.
//!
//! ## Security Disclaimer
//!
//! WHY2 is an experimental encryption algorithm. While it draws inspiration from established designs like AES,
//! **it has not undergone formal cryptographic review or extensive academic analysis**.
//!
//! As such, it should **not be considered suitable for high-assurance or production-grade cryptographic applications** where
//! proven security guarantees are required. Use at your own discretion, and always evaluate your threat model carefully.
//!
//! ## Versioning Note
//! All versions with the `-rex` suffix use the redesigned encryption system introduced in WHY2 v0.2.0-rex.
//! These builds are not compatible with pre-`rex` versions and should be treated as a separate lineage.
//!
//! ## License
//! WHY2 is licensed under the GNU GPLv3. You are free to use, modify, and redistribute it
//! under the terms of the license. See <https://www.gnu.org/licenses/> for details.

/// Core of WHY2 - encryption functions
pub mod core;

/// Chat - Minimalist chat app built on core module
#[cfg(feature = "chat")]
pub mod chat;
