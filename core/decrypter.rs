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

//! REX Decrypter
//!
//! This module defines the core decryption for WHY2, including round-key reversal,
//! Grid unmixing, deterministic unshuffling, and ISO 10126 padding removal. It reconstructs
//! the original data from encrypted Grid chunks using a symmetric key.
//!
//! # Overview
//! WHY2 encrypts data by transforming it into fixed-size grids ([`Grid`](crate::Grid)) using
//! CTR mode with a block cipher. Decryption reverses this process:
//!
//! 1. **Round Key Generation**: Reconstructs round keys from the master key using chained SHA-256 seeds.
//! 2. **CTR Mode Decryption**: Each ciphertext [`Grid`](crate::Grid) is XORed with the keystream block (the nonce plus block counter encrypted with WHY2).
//! 3. **ISO 10126 Padding Removal**: Truncates the final output using the last cell value as a padding marker.
//!
//! Since CTR mode is symmetric, the same encryption function is used for both encryption and decryption.

use rand::{ Rng, SeedableRng };
use rand_chacha::ChaCha20Rng;

use zeroize::Zeroizing;

use crate::
{
    crypto,
    GridError,
    options::{ EncryptedData, DecryptedData },
};

#[cfg(feature = "constant-time")]
use subtle::
{
    ConstantTimeEq,
    ConstantTimeLess,
    ConstantTimeGreater,
    ConditionallySelectable,
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
/// - Removes ISO 10126 padding from the final output
///
/// # Parameters
/// - `input`: An [`EncryptedData`] struct containing the encrypted grids and key grid.
///
/// - Ok([`DecryptedData`](crate::options::DecryptedData)) struct containing:
///   - `output`: A vector of decrypted `i64` values
///   - `key`: The original key [`Grid`](crate::Grid) flattened into a vector
/// - Err(String) if [`Grid`](crate::Grid) area is 1
pub fn decrypt<const W: usize, const H: usize>(input: EncryptedData<W, H>) -> Result<DecryptedData, GridError>
{
    //GET MUTABLE input
    let mut grids = input.output;
    let key_grid = input.key;

    //GENERATE ROUND KEYS
    let round_keys = crypto::generate_round_keys(&key_grid)?;

    //APPLY CTR MODE (PARALLEL)
    crypto::apply_ctr(&mut grids, &input.nonce, &round_keys);

    //DE-SHUFFLING VARIABLES
    let grid_area = W * H; //AREA OF A GRID
    let mut dprng = ChaCha20Rng::from_seed(crypto::sha256_seed_grid(&key_grid)); //DETERMINISTIC PSEUDO RANDOM NUMBER GENERATOR

    //DE-SHUFFLE INPUT GRIDS USING DPRNG SEEDED BY KEY HASH
    for grid in &mut grids
    {
        //SHUFFLE-MAP
        let mut shuffle_map = Zeroizing::new((0..grid_area).collect::<Vec<usize>>()); //UNSUFFLED MAP (0, 1, 2 ... 64)

        //SHUFFLE
        for i in (1..shuffle_map.len()).rev()
        {
            let j = dprng.random_range(0..=i);

            #[cfg(feature = "constant-time")]
            {
                for k in 0..=i
                {
                    let is_match = k.ct_eq(&j);

                    //CAST SHUFFLE MAP TO u64s
                    let mut val_i = shuffle_map[i] as u64;
                    let mut val_k = shuffle_map[k] as u64;

                    //SWAP
                    u64::conditional_swap(&mut val_i, &mut val_k, is_match);

                    //WRITE TO shuffle_map
                    shuffle_map[i] = val_i as usize;
                    shuffle_map[k] = val_k as usize;
                }
            }

            #[cfg(not(feature = "constant-time"))]
            {
                shuffle_map.swap(i, j);
            }
        }

        //FLATTEN CHUNK
        let flattened = Zeroizing::new(grid.iter().flatten().copied().collect::<Vec<i64>>());

        //APPLY INVERSE PERMUTATION
        let mut unshuffled = Zeroizing::new(vec![0i64; grid_area]);
        for (i, &shuffled_i) in shuffle_map.iter().enumerate()
        {
            #[cfg(feature = "constant-time")]
            {
                //O(N) SCAN FOR EACH CELL -> O(N^2)
                for j in 0..grid_area
                {
                    let is_match = j.ct_eq(&shuffled_i);
                    unshuffled[j].conditional_assign(&flattened[i], is_match);
                }
            }

            #[cfg(not(feature = "constant-time"))]
            {
                unshuffled[shuffled_i] = flattened[i];
            }
        }

        //REBUILD
        for (i, val) in unshuffled.iter().enumerate()
        {
            grid[i / W][i % W] = *val;
        }
    }

    //FLATTEN Vec<Grid> TO Vec<i64>
    let mut flattened = Zeroizing::new(grids.iter()
        .flat_map(|grid| grid.iter().flat_map(|row| row.iter())).copied().collect::<Vec<i64>>());

    let padding_len = *flattened.last().unwrap_or(&0) as usize;

    //CHECK PADDING VALIDITY
    #[cfg(feature = "constant-time")]
    {
        let padding_len_u64 = padding_len as u64;
        let total_len_u64 = flattened.len() as u64;

        //padding_len > 0
        let padding_gt_zero = padding_len_u64.ct_gt(&0);

        //padding_len <= total_len
        let len_valid = !total_len_u64.ct_lt(&padding_len_u64);

        if !bool::from(padding_gt_zero & len_valid)
        {
             return Err(GridError::InvalidPadding);
        }
    }

    #[cfg(not(feature = "constant-time"))]
    if padding_len == 0 || padding_len > flattened.len() //INVALID (POSSIBLY MALICIOUS) PADDING
    {
        return Err(GridError::InvalidPadding);
    }

    //REMOVE PADDING
    let new_len = flattened.len() - padding_len;
    flattened.truncate(new_len);

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
/// - ISO 10126 padding is removed before decoding.
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
        output.push(char::from_u32(hi).unwrap());
        if lo != 0 { output.push(char::from_u32(lo).unwrap()); }
    }

    Ok(output)
}
