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
//! WHY2 encrypts data by transforming it into fixed-size grids ([`Grid`]) using
//! CTR mode with a block cipher. Decryption reverses this process:
//!
//! 1. **Round Key Generation**: Reconstructs round keys from the master key using chained SHA-256 seeds.
//! 2. **CTR Mode Decryption**: Each ciphertext [`Grid`] is XORed with the keystream block (the nonce plus block counter encrypted with WHY2).
//!
//! Since CTR mode is symmetric, the same encryption function is used for both encryption and decryption.

use zeroize::Zeroizing;

use crate::
{
    crypto,
    grid::{ Grid, GridError },
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
/// # Returns
/// [`DecryptedData`] containing:
///   - `output`: A vector of decrypted `i64` values
///   - `key`: The original key [`Grid`] flattened into a vector
///
/// # Errors
/// Returns [`GridError`] if round-key generation fails.
///
/// # Security Notes
/// - Verify an authenticator before calling this. The returned plaintext is unauthenticated,
///   and decryption errors must not be exposed to untrusted callers.
#[must_use]
pub fn decrypt<const W: usize, const H: usize>(input: EncryptedData<W, H>) -> Result<DecryptedData, GridError>
{
    //GET MUTABLE input
    let mut grids = input.output;
    let key_grid = input.key;

    //GENERATE ROUND KEYS
    let round_keys = crypto::generate_round_keys(&key_grid)?;

    //APPLY CTR MODE (PARALLEL)
    crypto::apply_ctr(&mut grids, &input.nonce, &round_keys, None);

    //FLATTEN Vec<Grid> TO Vec<i64>
    let flattened = Zeroizing::new(grids.iter()
        .flat_map(Grid::to_flat).collect::<Vec<i64>>());

    //RETURN OUTPUT
    Ok(DecryptedData
    {
        output: flattened.to_vec(),
        key: key_grid.into_iter().collect(),
    })
}

/// Decrypts a WHY2-encrypted string.
///
/// Reads two 32-bit units per decrypted `i64` (big-endian, high unit first) until it reaches
/// the `u32::MAX` terminator written by [`encrypt_string`](crate::encrypter::encrypt_string).
/// Everything after the terminator is grid padding and is ignored.
///
/// # Errors
/// [`GridError::InvalidUnicode`] if a unit is not a valid `char` or the terminator is missing.
/// The error deliberately carries no decrypted data. Verify an authenticator **before**
/// calling this function and do not expose the error to untrusted parties.
#[must_use]
pub fn decrypt_string<const W: usize, const H: usize>(input: EncryptedData<W, H>) -> Result<Zeroizing<String>, GridError>
{
    //DECRYPT
    let decrypted = decrypt(input)?;

    let mut output = Zeroizing::new(String::with_capacity(decrypted.output.len() * 2));
    let mut terminated = false;

    'cells: for n in decrypted.output.iter()
    {
        let n = *n as u64;

        for unit in [(n >> 32) as u32, n as u32]
        {
            //TERMINATOR, THE REST IS PADDING
            if unit == u32::MAX
            {
                terminated = true;
                break 'cells;
            }

            output.push(char::from_u32(unit).ok_or(GridError::InvalidUnicode)?);
        }
    }

    //NO TERMINATOR = WRONG KEY OR CORRUPTED DATA
    if !terminated
    {
        return Err(GridError::InvalidUnicode);
    }

    Ok(output)
}
