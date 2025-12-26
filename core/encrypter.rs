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
//! deterministic shuffling, round-based mixing, and ISO 10126 padding. It transforms
//! raw data into encrypted Grid chunks using a symmetric key.
//!
//! # Overview
//! WHY2 encrypts data by converting it into fixed-size grids ([`Grid`]) and applying
//! nonlinear and linear transformations across multiple rounds. The process includes:
//!
//! 1. **Grid Shaping**: Input is padded and split into [`Grid`] chunks.
//! 2. **Key Handling**: A symmetric key is either provided or securely generated.
//! 3. **Deterministic Shuffling**: Each [`Grid`] is shuffled using a PRNG seeded from the key hash.
//! 4. **Round-Based Mixing**: Each [`Grid`] undergoes XOR, subcell diffusion, row shifting, and column mixing.

use rand_chacha::ChaCha20Rng;
use rand::
{
    Rng,
    SeedableRng,
    prelude::SliceRandom,
};

use zeroize::Zeroizing;

use crate::
{
    crypto,
    Grid,
    GridError,
    options::EncryptedData,
};

/// Encrypts a vector of `i64` values.
///
/// This function transforms the input into fixed-size grids ([`Grid`]), applies ISO 10126
/// padding, and performs round-based encryption using nonlinear and linear mixing.
///
/// # Parameters
/// - `input`: A vector of `i64` values representing the data.
/// - `key`: An optional symmetric key. If `None`, a secure key is generated automatically.
///          If provided, it must be exactly $2 \times W \times H$ elements long.
///
/// # Returns
/// An [`EncryptedData`] struct containing:
/// - `output`: A vector of encrypted [`Grid`]s.
/// - `key`: The key [`Grid`] used for encryption.
///
/// # Behavior
/// - Pads the input to a multiple of the grid area using ISO 10126 padding (random bytes).
/// - Splits the input into grid chunks and shuffles each using a deterministic PRNG seeded from the key hash.
/// - Applies round-based transformations: initial XOR, subcell mixing, row shifting, and column mixing.
///
/// Each plaintext block $P_i$ is encrypted using CTR mode:
/// $$ C_i = P_i \oplus E_K(\text{Nonce} + i) $$
///
/// where $E_K$ denotes the WHY2 block cipher keyed with $K$, and $i$ is the block counter.
pub fn encrypt<const W: usize, const H: usize>(input: Vec<i64>, key: Option<Vec<i64>>) -> Result<EncryptedData<W, H>, GridError>
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
                return Err(GridError::InvalidKeyLength { expected_len: grid_area, actual_len: k.len() });
            }

            //USE KEY IF MATCHING LENGTH
            Zeroizing::new(k)
        },

        //NO KEY, GENERATE ONE
        None => crypto::generate_key::<W, H>()
    };

    //GET MUTABLE input
    let mut input_used = Zeroizing::new(input);

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
    let mut grids: Vec<Grid<W, H>> = input_used.chunks(grid_area).map(|chunk| -> Result<Grid<W, H>, GridError>
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
        let mut flattened = Zeroizing::new(grid.iter().flatten().copied().collect::<Vec<i64>>());

        //SHUFFLE
        flattened.shuffle(&mut dprng);

        //REBUILD
        for (i, val) in flattened.iter().enumerate()
        {
            grid[i / W][i % W] = *val;
        }
    }

    //GENERATE ROUND KEYS
    let round_keys = crypto::generate_round_keys(&key_grid)?;

    //CTR VARIABLES
    let nonce = crypto::generate_nonce()?; //RANDOM GRID
    let mut counter_grid = nonce.clone();

    //APPLY ENCRYPTION TO EACH GRID
    for grid in &mut grids
    {
        //CREATE KEYSTREAM BLOCK
        let mut keystream_block = counter_grid.clone();

        //INITIAL XOR
        keystream_block ^= &round_keys[0];

        //ROUND OPERATIONS
        for (i, round_key) in round_keys[1..].iter().enumerate()
        {
            keystream_block ^= round_key;                    //XOR
            keystream_block.subcell(i);               //SUBCELL
            keystream_block.shift_rows(round_key); //SHIFT ROWS
            keystream_block.mix_columns();                   //MIX COLUMNS
            keystream_block.mix_diagonals();                 //MIX DIAGONALS
            keystream_block.mix_matrix(round_key); //MIX MATRIX
        }

        //XOR KEYSTREAM AND DATA
        *grid ^= &keystream_block;

        //INCREMENT COUNTER FOR NEXT BLOCK
        counter_grid.increment();
    }

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
/// - `input`: A reference to the string to encrypt.
/// - `key`: An optional symmetric key. If `None`, a secure key is generated automatically.
///          If provided, it must be exactly $2 \times W \times H$ elements long.
pub fn encrypt_string<const W: usize, const H: usize>(input: &String, key: Option<Vec<i64>>) -> Result<EncryptedData<W, H>, GridError>
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
    encrypt(vec_input.to_vec(), key)
}
