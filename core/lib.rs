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
//! ## Design Overview
//! The WHY2 encryption algorithm is loosely inspired by AES, but with a twist. Instead of relying on S-boxes,
//! WHY2 uses a nonlinear ARX-style transformation (Addition, Rotation, XOR) for symmetric diffusion.
//!
//! Key mechanics include:
//! - **Grid-based State**: Input and key data are formatted into 2D grids of 64-bit cells.
//! - **Key Expansion**: The key grid is shuffled and seeded to generate round keys.
//! - **Nonlinear Mixing**: Each round applies a transformation to the input grids using round tweaks to ensure variability.
//!
//! WHY2 also powers a minimalist chat application built for maximal privacy, designed for self-hosting
//! by individuals or small groups.
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
//! ## License
//! WHY2 is licensed under the GNU GPLv3. You are free to use, modify, and redistribute it
//! under the terms of the license. See <https://www.gnu.org/licenses/> for details.
//!
//! ### Deprecation Notice
//! Some parts of this module are deprecated and retained only for refence and legacy compatibility.
//! These parts represent early versions of the WHY2 encryption routines that are now considered insecure.
//!
//! Due to identified security concerns, these legacy modules should **not be used in production**.
//!
//! For secure applications, always use [`rex`]—the current and trusted implementation of the WHY2 encryption engine.

macro_rules! deprecated_mods
{
    (
        version = $version:literal,
        message = $msg:literal,
        mods = [$($name:ident),* $(,)?]
    ) =>
    {
        $(
            #[deprecated(since = $version, note = $msg)]
            pub mod $name;
        )*
    };
}

/// Modern, AES-inspired, implementation of WHY2
pub mod rex;

#[cfg(feature = "chat")]
#[path = "../chat/mod.rs"]
#[doc(hidden)]
pub mod chat;

//DEPRECATED
deprecated_mods!
{
    version = "0.2.0-rex",
    message = "Legacy encryption is unsecure. Use REX module instead.",
    mods = [ crypto, decrypter, encrypter, options ]
}
