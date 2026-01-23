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

//! REX Decrypter
//!
//! This module defines the core decryption for WHY2, including round-key reversal
//! and Grid unmixing. It reconstructs the original data from encrypted Grid
//! chunks using a symmetric key.
//!
//! # Overview
//! WHY2 encrypts data by transforming it into fixed-size grids ([`Grid`](crate::grid::Grid)) using
//! CTR mode with a block cipher. Decryption reverses this process:
//!
//! 1. **Round Key Generation**: Reconstructs round keys from the master key using chained SHA-256 seeds.
//! 2. **CTR Mode Decryption**: Each ciphertext [`Grid`](crate::grid::Grid) is XORed with the keystream block (the nonce plus block counter encrypted with WHY2).
//!
//! Since CTR mode is symmetric, the same encryption function is used for both encryption and decryption.

use zeroize::Zeroizing;

use crate::
{
    crypto,
    grid::GridError,
    types::{ EncryptedData, DecryptedData },
};

/// Decrypts a WHY2-encrypted data into raw `i64` values.
///
/// This function reverses the full WHY2 encryption pipeline using CTR mode:
///
/// $$ P_i = C_i \oplus E_K(\text{Nonce} + i) $$
///
/// where $E_K$ is the WHY2 block cipher and $i$ is the block counter.
///
/// - Generates round keys from the master key
/// - Applies CTR mode decryption (XOR with keystream blocks)
///
/// # Parameters
/// - `input`: An [`EncryptedData`] struct containing the encrypted grids and key grid.
///
/// - Ok([`DecryptedData`]) struct containing:
///   - `output`: A vector of decrypted `i64` values
///   - `key`: The original key [`Grid`](crate::grid::Grid) flattened into a vector
/// - Err(String) if [`Grid`](crate::grid::Grid) area is 1
pub fn decrypt<const W: usize, const H: usize>(input: EncryptedData<W, H>) -> Result<DecryptedData, GridError>
{
    //GET MUTABLE input
    let mut grids = input.output;
    let key_grid = input.key;

    //GENERATE ROUND KEYS
    let round_keys = crypto::generate_round_keys(&key_grid)?;

    //APPLY CTR MODE (PARALLEL)
    crypto::apply_ctr(&mut grids, &input.nonce, &round_keys);

    //FLATTEN Vec<Grid> TO Vec<i64>
    let flattened = Zeroizing::new(grids.iter()
        .flat_map(|grid| grid.iter().flat_map(|row| row.iter())).copied().collect::<Vec<i64>>());

    //RETURN OUTPUT
    Ok(DecryptedData
    {
        output: flattened.to_vec(),
        key: key_grid.into_iter().collect(),
    })
}

/// Decrypts a WHY2-encrypted data and reconstructs the original string.
///
/// This function performs full decryption using [`decrypt`],
/// then interprets each `i64` value as two concatenated `u32` characters. The first 4 bytes
/// represent the high character, and the next 4 bytes represent the low character. If the low
/// character is zero, it is omitted.
///
/// # Parameters
/// - `input`: An [`EncryptedData`] struct containing encrypted grids and key.
///
/// # Returns
/// - Ok(`String`) reconstructed from the decrypted values.
/// - Err(`String`) if Grid area is 1
///
/// # Notes
/// - Uses CTR mode for decryption (same as encryption).
/// - Uses big-endian decoding for each `i64` value.
/// - Each decrypted value contributes up to two Unicode scalar values.
pub fn decrypt_string<const W: usize, const H: usize>(input: EncryptedData<W, H>) -> Result<Zeroizing<String>, GridError>
{
    //DECRYPT
    let decrypted = decrypt(input)?;

    let mut output = Zeroizing::new(String::with_capacity(decrypted.output.len() * 2));

    for n in decrypted.output.iter()
    {
        let buf = n.to_be_bytes();

        //FIRST 4 BYTES = HIGH CHAR, FOLLOWING 4 BYTES = LOW CHAR
        let hi = u32::from_be_bytes(buf[0..4].try_into().unwrap()); //HIGH
        let lo = u32::from_be_bytes(buf[4..8].try_into().unwrap()); //LOW

        //PUSH CHARS TO STRING
        if hi != 0 { output.push(char::from_u32(hi).ok_or(GridError::InvalidUnicode { value: hi })?); }
        if lo != 0 { output.push(char::from_u32(lo).ok_or(GridError::InvalidUnicode { value: lo })?); }
    }

    Ok(output)
}
