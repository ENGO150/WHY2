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
//! WHY2 is a modern, high-performance symmetric encryption system designed for
//! privacy-first applications where transparency and freedom are non-negotiable.
//!
//! ## What is WHY2?
//!
//! WHY2 is a **grid-based block cipher** that organizes data into configurable
//! 2D matrices of 64-bit cells. Unlike traditional table-based designs, it achieves
//! nonlinearity through **ARX operations** (Add-Rotate-XOR), eliminating cache-timing
//! vulnerabilities inherent to lookup table approaches.
//!
//! The cipher uses a **Substitution-Permutation Network** structure with multiple
//! rounds of mixing operations, combining nonlinear transformations with linear
//! diffusion layers. Encryption is performed in **Counter (CTR) mode**, enabling
//! parallel processing of multiple blocks.
//!
//! ## Key Characteristics
//!
//! - **Configurable Block Sizes**: From 4×4 to 16×16 grids (1024-16384 bits per block)
//! - **Cache-Timing Resistant**: ARX-based design eliminates table lookups
//! - **SIMD-Optimized**: Vectorized operations for modern CPUs (AVX2, NEON)
//! - **Constant-Time**: All cryptographic operations avoid timing side-channels
//! - **Memory-Safe**: Pure Rust implementation prevents buffer overflows
//! - **Parallel Encryption**: CTR mode enables multi-core processing
//!
//! ## Design Philosophy
//!
//! WHY2 draws from established cryptographic principles while introducing
//! innovations suited for modern hardware:
//!
//! - **SPN Architecture**: Proven approach used by standardized ciphers
//! - **ARX Operations**: Memory-hard-free construction (inspired by TEA/XTEA/Salsa20)
//! - **True MDS Diffusion**: Cauchy MDS matrix over $\mathbb{F}_{2^{64}}$ provides
//!   provably optimal branch number, enabling formal security bounds.
//! - **Native 64-bit Operations**: Optimized for contemporary processor architectures
//!
//! For detailed security architecture and implementation specifics, see the
//! [SECURITY](https://git.satan.red/ENGO150/WHY2/-/blob/stable/SECURITY) documentation.
//!
//! ## Features
//!
//! - Grid-based encryption with customizable dimensions
//! - ARX-style nonlinear mixing (cache-timing resistant)
//! - SIMD-accelerated operations (4× i64 vector processing)
//! - Round-key expansion via ChaCha20 CSPRNG
//! - Optional authenticated encryption (HMAC-SHA256)
//! - Constant-time implementation (via `subtle` crate)
//! - Memory safety through Rust ownership system
//! - Automatic key material zeroization on drop
//!
//! ## Cargo Features
//!
//! This crate allows selective enabling of components to keep the build lightweight.
//!
//! - **`auth`** (default):
//!   Enables the [`auth`] module for verifying data integrity and authenticity using an
//!   Encrypt-then-MAC scheme (HMAC-SHA256).
//!
//! - **`constant-time`** (default):
//!   Enables constant-time comparison for cryptographic operations using the [`subtle`] crate.
//!   Disabling this may improve performance on non-sensitive data but opens the system to
//!   timing attacks.
//!
//! - ~~**`legacy`**:
//!   Enables the deprecated \[`legacy`\] module containing older, insecure versions of the
//!   encryption routines. This feature should only be used for migration or compatibility
//!   testing.~~
//!
//! ## Philosophy
//!
//! - **Privacy is a right**, not a subscription feature.
//! - **No government oversight**: no telemetry, no backdoors, no metadata leakage.
//! - **No payment required**: encryption should be free as in freedom.
//! - **Full transparency**: all design decisions documented, all code auditable.
//!
//! ## Terminology
//!
//! The codebase is organized to distinguish between the current implementation and
//! deprecated versions:
//!
//! * **REX**: Refers to the modern, secure implementation of the WHY2 algorithm.
//! These are the modules exposed directly at the crate root (e.g., [`encrypter`], [`decrypter`]).
//! * ~~**Legacy**: Refers to older, deprecated encryption routines found in the \[`legacy`\] module.
//! These are retained for compatibility but are considered insecure.~~
//!
//! ## Examples
//!
//! For comprehensive usage examples, including global algorithm usage and specific edge cases,
//! please refer to the [examples directory](https://git.satan.red/ENGO150/WHY2/-/tree/stable/examples)
//! in the official repository.
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

//DOCS
#![doc(html_logo_url = "https://why2.satan.red/apple-icon.png")]
#![doc(html_favicon_url = "https://why2.satan.red/icon-dark-32x32.png")]

//PRIVATE MODULES
mod gf;

//PUBLIC MODULES
#[cfg(feature = "auth")]
pub mod auth;
pub mod consts;
pub mod crypto;
pub mod decrypter;
pub mod encrypter;
pub mod grid;
pub mod types;
