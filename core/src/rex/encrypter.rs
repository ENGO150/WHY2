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

//! REX Encrypter
//!
//! This module defines the full encryption pipeline for WHY2, including Grid shaping,
//! deterministic shuffling, round-based mixing, and PKCS-style padding. It transforms
//! raw data into encrypted Grid chunks using a symmetric key.
//!
//! # Overview
//! WHY2 encrypts data by converting it into fixed-size grids ([`Grid`]) and applying
//! nonlinear and linear transformations across multiple rounds. The process includes:
//!
//! 1. **Grid Shaping**: Input is padded and split into `Grid` chunks.
//! 2. **Key Handling**: A symmetric key is either provided or securely generated.
//! 3. **Deterministic Shuffling**: Each Grid is shuffled using a PRNG seeded from the key hash.
//! 4. **Round-Based Mixing**: Each Grid undergoes XOR, subcell diffusion, row shifting, and column mixing.

use rand::
{
    SeedableRng,
    prelude::SliceRandom,
};

use rand::Rng;
use rand_chacha::ChaCha20Rng;

use crate::rex::
{
    crypto,
    Grid,
    options::EncryptedData,
};

/// Encrypts a vector of `i64` values.
///
/// This function transforms the input into fixed-size grids ([`Grid`]), applies PKCS#7-style
/// padding, and performs round-based encryption using nonlinear and linear mixing. If no key is
/// provided, a secure symmetric key is generated internally.
///
/// # Parameters
/// - `input`: A vector of `i64` values representing the data.
/// - `key`: An optional symmetric key. If `None`, a secure key is generated automatically.
///          If provided, it must be exactly `2 × W × H` elements long.
///
/// # Returns
/// An [`EncryptedData`] struct containing:
/// - `output`: A vector of encrypted Grids.
/// - `key`: The key grid used for encryption.
///
/// # Behavior
/// - Pads the input to a multiple of the grid area using PKCS#7-style padding.
/// - Splits the input into grid chunks and shuffles each using a deterministic PRNG seeded from the key hash.
/// - Applies round-based transformations: initial XOR, subcell mixing, row shifting, and column mixing.
/// - Returns `None` if the provided key is invalid (wrong length).
pub fn encrypt<const W: usize, const H: usize>(input: Vec<i64>, key: Option<Vec<i64>>) -> Result<EncryptedData<W, H>, String>
{
    //REX OPTIONS
    let grid_area = W * H; //AREA OF REX GRID

    //GET KEY THAT WILL BE USED FOR ENCRYPTION
    let key_used = match key
    {
        //KEY PASSED AS PARAMETER
        Some(k) =>
        {
            //CHECK FOR INVALID KEY
            if k.len() != grid_area * 2
            {
                return Err(format!
                (
                    "Invalid key length: expected length {}, got {}",
                    grid_area * 2, k.len()
                ));
            }

            //USE KEY IF MATCHING LENGTH
            k
        },

        //NO KEY, GENERATE ONE
        None => crypto::generate_key::<W, H>()
    };

    //GET MUTABLE input
    let mut input_used = input;

    //PAD input_used TO MULTIPLE OF 64 (ADD EXTRA GRID IF FULL) [ISO 10126]
    let remainder = input_used.len() % grid_area; //PADDING CHARS REMAINING TO FULL GRID
    let padding_len = if remainder == 0 { grid_area } else { grid_area - remainder }; //HOW MUCH PADDING TO INSERT

    //FILL PADDING
    let mut rng = rand::rng();
    for _ in 0..(padding_len - 1)
    {
        input_used.push(rng.random::<i64>());
    }
    input_used.push(padding_len as i64);

    //SPLIT INTO CHUNKS OF 64 AND SHAPE TO 8x8 GRID
    let mut grids: Vec<Grid<W, H>> = input_used.chunks(grid_area).map(|chunk| -> Result<Grid<W, H>, String>
    {
        let mut grid = Grid::new()?; //CREATE GRID
        for (i, &val) in chunk.iter().enumerate()
        {
            grid[i / W][i % W] = val;
        }

        Ok(grid)
    }).collect::<Result<Vec<_>, _>>()?;

    //SHAPE KEY TO 8x8 GRID
    let key_grid = Grid::<W, H>::from_key(key_used)?;

    //SHUFFLE INPUT GRID USING DETERMINISTIC PRNG SEEDED BY KEY HASH
    let mut dprng = ChaCha20Rng::from_seed(crypto::sha256_seed_grid(&key_grid)); //DETERMINISTIC PSEUDO RANDOM NUMBER GENERATOR
    for grid in &mut grids
    {
        //FLATTEN CHUNK
        let mut flattened: Vec<i64> = grid.iter().flatten().copied().collect();

        //SHUFFLE
        flattened.shuffle(&mut dprng);

        //REBUILD
        for (i, val) in flattened.into_iter().enumerate()
        {
            grid[i / W][i % W] = val;
        }
    }

    //GENERATE ROUND KEYS
    let round_keys = crypto::generate_round_keys(&key_grid)?;

    //PREVIOUS GRID STATE (FOR CBC)
    let mut previous_grid = Grid::<W, H>::new().unwrap();

    //APPLY ENCRYPTION TO EACH GRID
    for mut grid in &mut grids
    {
        //INITIAL XOR
        grid ^= &previous_grid; //CIPHER BLOCK CHAINING (CBC)
        grid ^= &round_keys[0];

        //XOR WITH EACH ROUND KEY AND SHIFT ROWS & COLUMNS
        for (i, round_key) in round_keys[1..].iter().enumerate()
        {
            grid ^= round_key;                    //XOR
            grid.subcell(i);               //SUBCELL
            grid.shift_rows(round_key); //SHIFT ROWS
            grid.mix_columns();                   //MIX COLUMNS
            grid.mix_matrix(round_key); //MIX MATRIX
        }

        //SAVE CURRENT GRID STATE
        previous_grid = grid.clone();
    }

    //RETURN OUTPUT
    Ok(EncryptedData
    {
        output: grids,
        key: key_grid,
    })
}

/// Encrypts a string.
///
/// This function encodes the input string into `i64` values by packing two `char`s
/// into each 64-bit integer. It then delegates to [`encrypt`], which applies Grid shaping,
/// deterministic shuffling, round-based mixing, and PKCS#7-style padding.
///
/// # Parameters
/// - `input`: A reference to the string to encrypt.
/// - `key`: An optional symmetric key. If `None`, a secure key is generated automatically.
///          If provided, it must be exactly `2 × W × H` elements long.
///
/// # Returns
/// An [`EncryptedData`] struct containing:
/// - `output`: A vector of encrypted `Grid` chunks.
/// - `key`: The key grid used for encryption.
///
/// # Encoding Notes
/// - Each `i64` packs two `char`s: the first 4 bytes are the high character, the next 4 bytes the low.
/// - If the string has an odd number of characters, a null character (`'\0'`) is appended for alignment.
pub fn encrypt_string<const W: usize, const H: usize>(input: &String, key: Option<Vec<i64>>) -> Result<EncryptedData<W, H>, String>
{
    //CONVERT input TO Vec<i64>
    let mut chars: Vec<char> = input.chars().collect();

    //INSERT PADDING
    if chars.len() % 2 != 0
    {
        chars.push('\0');
    }

    //CONVERT
    let vec_input = chars.chunks(2).map(|pair|
    {
        //FILL BUFFER
        let mut buf = [0u8; 8];
        buf[..4].copy_from_slice(&(pair[0] as u32).to_ne_bytes());
        buf[4..].copy_from_slice(&(pair[1] as u32).to_ne_bytes());

        //APPEND
        i64::from_ne_bytes(buf)
    }).collect();

    //ENCRYPT Vec<i64> AND RETURN
    encrypt(vec_input, key)
}
