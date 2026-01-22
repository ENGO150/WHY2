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

//! # WHY2
//!
//! WHY2 is a modern, fast, and secure encryption crate designed for privacy-first applications.
//!
//! ## Design Overview
//! The WHY2 encryption algorithm is loosely inspired by AES, but with a twist. Instead of relying on S-boxes,
//! WHY2 uses a nonlinear ARX-style transformation (Addition, Rotation, XOR) for symmetric diffusion.
//!
//! Key mechanics include:
//! - **Grid-based State**: Input and key data are formatted into 2D grids of 64-bit cells.
//! - **Key Expansion**: The key grid is shuffled and seeded to generate round keys.
//! - **Nonlinear Mixing**: Each round applies a transformation to the input grids using round tweaks to ensure variability.
//!
//! WHY2 also powers a minimalist text and voice chat application built for maximal privacy, designed for self-hosting
//! by individuals or small groups.
//!
//! ## Features
//! - Grid-based encryption with customizable layout
//! - ARX-style nonlinear mixing instead of S-boxes
//! - Round-key generation from seeded, shuffled keys
//! - Lightweight encrypted text and voice chat backend for private deployments
//! - Maximal customization
//!
//! ## Cargo Features
//!
//! This crate allows selective enabling of components to keep the build lightweight.
//!
//! - **`constant-time`** (default):
//!   Enables constant-time comparison for cryptographic operations using the [`subtle`] crate.
//!   Disabling this may improve performance on non-sensitive data but opens the system to timing attacks.
//!
//! - **`client`**:
//!   Enables the terminal-based client application with interactive interface and real-time voice chat support.
//!
//! - **`server`**:
//!   Enables the relay server logic for routing encrypted messages between clients.
//!   *Use this if you are building a custom node or hosting a relay.*
//!
//! - **`legacy`**:
//!   Enables the deprecated `legacy` module containing older, insecure versions of the encryption routines.
//!   This feature should only be used for migration or compatibility testing.
//!
//! ## Philosophy
//! - **Privacy is a right**, not a subscription feature.
//! - **No government insight**: no telemetry, no backdoors, no metadata leakage.
//! - **No payment required**: encryption should be free as in freedom.
//!
//! ## Terminology
//!
//! The codebase is organized to distinguish between the current implementation and deprecated versions:
//!
//! * **REX**: Refers to the modern, secure implementation of the WHY2 algorithm.
//! These are the modules exposed directly at the crate root (e.g., [`encrypter`], [`decrypter`]).
//! * **Legacy**: Refers to older, deprecated encryption routines found in the [`legacy`] module.
//! These are retained for compatibility but are considered insecure.
//!
//! ## Security Disclaimer
//!
//! WHY2 is an experimental encryption algorithm. While it draws inspiration from established designs like AES,
//! **it has not undergone formal cryptographic review or extensive academic analysis**.
//!
//! As such, it should **not be considered suitable for high-assurance or production-grade cryptographic applications** where
//! proven security guarantees are required. Use at your own discretion, and always evaluate your threat model carefully.
//!
//! ## License
//! WHY2 is licensed under the GNU GPLv3. You are free to use, modify, and redistribute it
//! under the terms of the license. See <https://www.gnu.org/licenses/> for details.

//MODULES
#[cfg(feature = "auth")]
pub mod auth;
pub mod consts;
pub mod crypto;
pub mod decrypter;
pub mod encrypter;
pub mod grid;
pub mod types;

#[cfg(feature = "legacy")]
#[deprecated(since = "0.2.0-rex", note = "Legacy encryption is unsecure. Use REX module (why2::core) instead.")]
pub mod legacy;
