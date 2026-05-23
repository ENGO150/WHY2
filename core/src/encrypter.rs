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

//! REX Encrypter
//!
//! This module defines the full encryption pipeline for WHY2, including Grid shaping
//! and round-based mixing. It transforms raw data into encrypted Grid chunks using
//! a symmetric key.
//!
//! # Overview
//! WHY2 encrypts data by converting it into fixed-size grids ([`Grid`]) and applying
//! nonlinear and linear transformations across multiple rounds in CTR mode. The process includes:
//!
//! 1. **[`Grid`] Shaping**: Input is padded and split into [`Grid`] chunks.
//! 2. **Key Handling**: A symmetric key is either provided or securely generated.
//! 3. **Nonce Generation**: A random nonce is generated for CTR mode.
//! 4. **CTR Mode Encryption**: Each plaintext [`Grid`] is XORed with a keystream
//! block derived from encrypting the nonce plus block counter.

use zeroize::Zeroizing;

use crate::
{
    crypto,
    grid::{ Grid, GridError },
    types::EncryptedData,
};

/// Encrypts a vector of `i64` values.
///
/// This function transforms the input into fixed-size grids ([`Grid`]) and performs
/// round-based encryption using nonlinear and linear mixing.
///
/// # Parameters
/// - `input`: A slice of `i64` values representing the data.
/// - `key`: An optional symmetric key. If `None`, a secure key is generated automatically.
///          If provided, it must be exactly $2 \times W \times H$ elements long.
///
/// # Returns
/// An [`EncryptedData`] struct containing:
/// - `output`: A vector of encrypted [`Grid`]s.
/// - `key`: The key [`Grid`] used for encryption.
///
/// # Behavior
/// - Splits the input into [`Grid`] chunks.
/// - Generates a random nonce for CTR mode.
/// - Applies CTR mode encryption using the WHY2 block cipher.
///
/// Each plaintext block $P_i$ is encrypted using CTR mode:
/// $$ C_i = P_i \oplus E_K(\text{Nonce} + i) $$
///
/// where $E_K$ denotes the WHY2 block cipher keyed with $K$, and $i$ is the block counter.
pub fn encrypt<const W: usize, const H: usize>(input: &[i64], key: Option<&[i64]>) -> Result<EncryptedData<W, H>, GridError>
{
    //REX OPTIONS
    let grid_area = W * H; //AREA OF REX GRID

    //GET KEY THAT WILL BE USED FOR ENCRYPTION
    let key_used = match key
    {
        //KEY PASSED AS PARAMETER
        Some(k) => Zeroizing::new(k.to_vec()),

        //NO KEY, GENERATE ONE
        None => crypto::generate_key::<W, H>()
    };

    //SPLIT INTO CHUNKS OF 64 AND SHAPE TO 8x8 GRID
    let mut grids: Vec<Grid<W, H>> = input.chunks(grid_area).map(|chunk| -> Result<Grid<W, H>, GridError>
    {
        let mut grid = Grid::new()?; //CREATE GRID
        for (i, &val) in chunk.iter().enumerate()
        {
            grid[i / W][i % W] = val;
        }

        Ok(grid)
    }).collect::<Result<Vec<_>, _>>()?;

    //SHAPE KEY TO 8x8 GRID
    let key_grid = Grid::<W, H>::from_key(&key_used)?;

    //GENERATE ROUND KEYS
    let round_keys = crypto::generate_round_keys(&key_grid)?;

    //CTR VARIABLES
    let nonce = crypto::generate_nonce()?; //RANDOM GRID

    //APPLY CTR MODE (PARALLEL)
    crypto::apply_ctr(&mut grids, &nonce, &round_keys);

    //RETURN OUTPUT
    Ok(EncryptedData
    {
        output: grids,
        key: key_grid,
        nonce: nonce,
    })
}

/// Encrypts a string.
///
/// This function encodes the input string into `i64` values by packing two `char`s
/// into each 64-bit integer.
///
/// # Parameters
/// - `input`: A string slice to encrypt.
/// - `key`: An optional symmetric key. If `None`, a secure key is generated automatically.
///          If provided, it must be exactly $2 \times W \times H$ elements long.
pub fn encrypt_string<const W: usize, const H: usize>(input: &str, key: Option<&[i64]>) -> Result<EncryptedData<W, H>, GridError>
{
    //CONVERT input TO Vec<i64>
    let mut chars = Zeroizing::new(input.chars().collect::<Vec<char>>());

    //INSERT PADDING
    if chars.len() % 2 != 0
    {
        chars.push('\0');
    }

    //CONVERT
    let vec_input = Zeroizing::new(chars.chunks(2).map(|pair|
    {
        //FILL BUFFER
        let mut buf = [0u8; 8];
        buf[..4].copy_from_slice(&(pair[0] as u32).to_be_bytes());
        buf[4..].copy_from_slice(&(pair[1] as u32).to_be_bytes());

        //APPEND
        i64::from_be_bytes(buf)
    }).collect::<Vec<i64>>());

    //ENCRYPT Vec<i64> AND RETURN
    encrypt(&vec_input, key)
}
