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

//! REX options
//!
//! This module defines core parameters and data structures used throughout the WHY2 encryption
//! and decryption pipeline. It includes round configuration constants, mixing parameters, and
//! the input/output formats for both encryption and decryption.

use zeroize::{ Zeroize, ZeroizeOnDrop };

use crate::Grid;

/// Number of round keys used in the WHY2 cipher.
///
/// Each round key introduces nonlinear and linear mixing. This value controls the depth
/// of encryption and decryption. Increasing it strengthens diffusion but adds computational cost.
///
/// Do not modify unless you're fully aware of the cryptographic implications.
pub const ROUND_KEYS: usize = 14;

/// Number of ARX mixing iterations per cell in the [`subcell`](crate::Grid::subcell) transformation.
///
/// This controls how many rounds of Add-Rotate-XOR are applied to each cell. More rounds
/// increase diffusion and resistance to pattern leakage.
///
/// Changing this affects the cipher’s nonlinear behavior.
pub const SUBCELL_ROUNDS: u32 = 32;

/// Constant used to break symmetry in ARX mixing.
///
/// This is derived from `(2^32) / φ`, where φ is the golden ratio. It ensures that each
/// round introduces asymmetry and avoids cyclic patterns in the
/// [`subcell`](crate::Grid::subcell) transformation.
///
/// This value is cryptographically sensitive and should not be changed casually.
pub const SUBCELL_DELTA: u32 = 0x9E3779B9;

//STRUCTS
/// Container for encrypted output.
///
/// This struct holds the encrypted Grid chunks, the key Grid and the IV used during encryption.
/// It is returned by [`encrypt`](crate::encrypter::encrypt) and consumed by
/// [`decrypt`](crate::decrypter::decrypt) to reverse the transformation.
///
/// # Fields
/// - `output`: A vector of encrypted `Grid<W, H>` chunks.
/// - `key`: The key Grid used for encryption and required for decryption.
/// - `iv`: The initialization vector used for CBC.
///
/// # Notes
/// - The key is stored in Grid form for direct use in round key generation.
/// - The IV does not need to be kept secret but must be unique per encryption.
pub struct EncryptedData<const W: usize, const H: usize> //DATA FOR REX ENCRYPTER
{
    pub output: Vec<Grid<W, H>>, //OUTPUT VALUE
    pub key: Grid<W, H>,         //KEY USED FOR ENCRYPTION
    pub nonce: Grid<W, H>,       //NONCE
}

/// Container for decrypted output.
///
/// This struct holds the final data and the original key used during decryption.
/// It is returned by [`decrypt`](crate::decrypter::decrypt) and may be used to reconstruct
/// the original string or binary payload.
///
/// # Fields
/// - `output`: A flat vector of decrypted `i64` values.
/// - `key`: The original key used for decryption, stored as a flat vector.
///
/// # Notes
/// - Padding is removed before populating `output`.
/// - The key is flattened for portability and auditability.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct DecryptedData //DATA FOR REX DECRYPTER
{
    pub output: Vec<i64>, //OUTPUT VALUE
    pub key: Vec<i64>,    //KEY USED FOR DECRYPTION
}
