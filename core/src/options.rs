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

//! REX options
//!
//! This module defines core parameters and data structures used throughout the WHY2 encryption
//! and decryption pipeline. It includes round configuration constants, mixing parameters, and
//! the input/output formats for both encryption and decryption.

use zeroize::{ Zeroize, ZeroizeOnDrop };

use crate::{ consts, Grid };

/// Container for encrypted output.
///
/// This struct holds the encrypted Grid chunks, the key Grid and the IV used during encryption.
/// It is returned by [`encrypt`](crate::encrypter::encrypt) and consumed by
/// [`decrypt`](crate::decrypter::decrypt) to reverse the transformation.
///
/// # Fields
/// - `output`: A vector of encrypted [`Grid`]`<W, H>` chunks.
/// - `key`: The key [`Grid`] used for encryption and required for decryption.
/// - `nonce`: The nonce used for CTR mode.
///
/// # Notes
/// - The key is stored in [`Grid`] form for direct use in round key generation.
/// - The nonce does not need to be kept secret but must be unique per encryption.
#[derive(Zeroize)]
pub struct EncryptedData //DATA FOR REX ENCRYPTER
<
    const W: usize = { consts::DEFAULT_GRID_WIDTH },
    const H: usize = { consts::DEFAULT_GRID_HEIGHT },
>
{
    #[zeroize(skip)]
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
