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

//! # WHY2 Core
//!
//! This module implements the core encryption logic behind WHY2 algorithm.
//!
//! ## Design Overview
//! - Input and key are formatted into 2D grids of 64-bit cells.
//! - The key grid is shuffled and seeded to generate round keys.
//! - Each round applies a nonlinear transformation to the input grids.
//! - The transofrmation avoid traditional S-boxes, relying instead on symmetric diffusion.
//! - Round tweaks ensure variability across rounds without requiring per-round constants.
//!
//! ### Deprecation Notice
//!
//! Some parts of this module are deprecated and retained only for reference and legacy compatibility.
//! Those parts are early versions of the WHY2 encryption routines that are considered insecure.
//!
//! Due to identified security concerns and lack of cryptographic robustness, this module should **not be used in production**.
//!
//! These deprecated components are **not actively documented or maintained**. They remain visible for historical context,
//! but all future documentation efforts are focused on secure and supported modules.
//!
//! For secure applications, use [`core::rex`]—the current and trusted implementation of the WHY2 encryption engine.

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
