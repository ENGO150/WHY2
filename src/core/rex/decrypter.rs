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
//! Grid unmixing, deterministic unshuffling, and PKCS-style padding removal. It reconstructs
//! the original data from encrypted Grid chunks using a symmetric key.
//!
//! # Overview
//! WHY2 encrypts data by transforming it into fixed-size grids ([`Grid`](crate::core::rex::Grid)) and applying
//! nonlinear and linear mixing across multiple rounds. Decryption reverses these steps:
//!
//! 1. **Round Key Generation**: Reconstructs round keys from the master key using chained SHA-256 seeds.
//! 2. **Grid Unmixing**: Applies inverse subcell, row shift, and column mixing in reverse round order.
//! 3. **Unshuffling**: Reverses the Grid permutation using a deterministic PRNG seeded from the key hash.
//! 4. **PKCS Padding Removal**: Truncates the final output using the last cell value as a padding marker.

use rand::
{
    SeedableRng,
    prelude::SliceRandom,
};

use rand_chacha::ChaCha20Rng;

use crate::core::rex::
{
    crypto,
    Grid,
    options::{ EncryptedData, DecryptedData },
};

/// Decrypts a WHY2-encrypted data into raw `i64` values.
///
/// This function reverses the full WHY2 encryption pipeline:
/// - Applies inverse round transformations (subcell, shift rows, mix columns)
/// - XORs each grid with round keys in reverse order
/// - Unshuffles each grid using a deterministic PRNG seeded from the key hash
/// - Removes PKCS-style padding from the final output
///
/// # Parameters
/// - `input`: An [`EncryptedData`] struct containing the encrypted grids and key grid.
///
/// # Returns
/// - Ok([`DecryptedData`]) struct containing:
///   - `output`: A vector of decrypted `i64` values
///   - `key`: The original key Grid flattened into a vector
/// - Err(String) if Grid area is 1
///
/// # Notes
/// - Padding is removed using PKCS-style logic: the last cell value indicates how many trailing values to discard.
/// - The PRNG used for unshuffling is seeded from the SHA-256 hash of the key grid.
/// - All transformations are deterministic and reversible.
pub fn decrypt<const W: usize, const H: usize>(input: EncryptedData<W, H>) -> Result<DecryptedData, String>
{
    //GET MUTABLE input
    let mut grids = input.output;
    let key_grid = input.key;

    //GENERATE ROUND KEYS
    let round_keys = crypto::generate_round_keys(&key_grid)?;

    //PREVIOUS GRID STATE (FOR CBC)
    let mut previous_grid = Grid::<W, H>::new().unwrap();

    //DECRYPT EACH ENCRYPTED GRID
    for mut grid in &mut grids
    {
        //SAVE CURRENT GRID STATE
        let current_grid = grid.clone();

        //XOR WITH EACH ROUND KEY AND SHIFT ROWS & COLUMNS
        for (i, round_key) in round_keys[1..].iter().enumerate().rev()
        {
            grid.inv_mix_matrix(round_key); //UNMIX MATRIX
            grid.inv_mix_columns();                   //UNMIX COLUMNS
            grid.inv_shift_rows(round_key); //UNSHIFT ROWS
            grid.inv_subcell(i);               //INVERT SUBCELL
            grid ^= round_key;                        //XOR
        }

        //INITIAL XOR
        grid ^= &round_keys[0];
        grid ^= &previous_grid; //CIPHER BLOCK CHAINING (CBC)

        //SAVE CURRENT (TECHNICALLY PREVIOUS) GRID STATE
        previous_grid = current_grid;
    }

    //DE-SHUFFLING VARIABLES
    let grid_area = W * H; //AREA OF A GRID
    let mut dprng = ChaCha20Rng::from_seed(crypto::sha256_seed_grid(&key_grid)); //DETERMINISTIC PSEUDO RANDOM NUMBER GENERATOR

    //DE-SHUFFLE INPUT GRIDS USING DPRNG SEEDED BY KEY HASH
    for grid in &mut grids
    {
        //SHUFFLE-MAP
        let mut shuffle_map: Vec<usize> = (0..grid_area).collect(); //UNSUFFLED MAP (0, 1, 2 ... 64)
        shuffle_map.shuffle(&mut dprng); //SHUFFLE GRID WITH DPRNG

        //FLATTEN CHUNK
        let flattened: Vec<i64> = grid.iter().flatten().copied().collect();

        //APPLY INVERSE PERMUTATION
        let mut unshuffled = vec![0i64; grid_area];
        for (i, &shuffled_i) in shuffle_map.iter().enumerate()
        {
            unshuffled[shuffled_i] = flattened[i];
        }

        //REBUILD
        for (i, val) in unshuffled.into_iter().enumerate()
        {
            grid[i / W][i % W] = val;
        }
    }

    //FLATTEN Vec<Grid> TO Vec<i64>
    let mut flattened: Vec<i64> = grids.iter().flat_map(|grid| grid.iter().flat_map(|row| row.iter())).copied().collect();

    //REMOVE PADDING
    flattened.truncate(flattened.len() - (*flattened.last().unwrap() as usize));

    //RETURN OUTPUT
    Ok(DecryptedData
    {
        output: flattened,
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
/// - Uses native-endian decoding for each `i64` value.
/// - Each decrypted value contributes up to two Unicode scalar values.
/// - PKCS-style padding is removed before decoding.
pub fn decrypt_string<const W: usize, const H: usize>(input: EncryptedData<W, H>) -> Result<String, String>
{
    //DECRYPT
    let decrypted = decrypt(input)?.output;

    let mut output = String::with_capacity(decrypted.len() * 2);

    for n in decrypted
    {
        let buf = n.to_ne_bytes();

        //FIRST 4 BYTES = HIGH CHAR, FOLLOWING 4 BYTES = LOW CHAR
        let hi = u32::from_ne_bytes(buf[0..4].try_into().unwrap()); //HIGH
        let lo = u32::from_ne_bytes(buf[4..8].try_into().unwrap()); //LOW

        //PUSH CHARS TO STRING
        output.push(char::from_u32(hi).unwrap());
        if lo != 0 { output.push(char::from_u32(lo).unwrap()); }
    }

    Ok(output)
}
