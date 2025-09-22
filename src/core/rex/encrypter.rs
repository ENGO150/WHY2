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

use std::iter;

use rand::
{
    SeedableRng,
    rngs::StdRng,
    prelude::SliceRandom,
};

use crate::core::
{
    crypto,
    misc,
    options::RexData,
};

//TYPES
type Grid = [[i64; 8]; 8];

//FUNCTIONS
//PRIVATE
fn empty_grid() -> Grid
{
    [[0i64; 8]; 8]
}

//PUBLIC
pub fn encrypt(input: Vec<i64>, key: Option<Vec<i64>>) -> RexData //ENCRYPT
{
    //CHECK FOR ACTIVE WHY2 VERSION
    misc::check_version();

    //GET KEY THAT WILL BE USED FOR ENCRYPTION
    let key_used = match key
    {
        //KEY PASSED AS PARAMETER
        Some(k) =>
        {
            //CHECK FOR INVALID KEY
            if k.len() != 128 { return RexData::empty(); }

            //USE KEY IF MATCHING LENGTH
            k
        },

        //NO KEY, GENERATE ONE
        None =>
        {
            misc::generate_rex_key(128)
        }
    };

    //GET MUTABLE input
    let mut input_used = input;

    //PAD input_used TO MULTIPLE OF 64 (ADD EXTRA GRID IF FULL) [PKCS#7]
    let remainder = input_used.len() % 64; //PADDING CHARS REMAINING TO FULL GRID
    let padding_len = if remainder == 0 { 64 } else { 64 - remainder }; //HOW MUCH PADDING TO INSERT
    input_used.extend(iter::repeat(padding_len as i64).take(padding_len));

    //SPLIT INTO CHUNKS OF 64 AND SHAPE TO 8x8 GRID
    let mut chunks: Vec<Grid> = input_used.chunks(64).map(|chunk|
    {
        let mut grid = empty_grid(); //CREATE GRID
        for (i, &val) in chunk.iter().enumerate()
        {
            grid[i / 8][i % 8] = val;
        }

        grid
    }).collect();

    //SHUFFLE INPUT GRID USING DETERMINISTIC PRNG SEEDED BY KEY HASH
    let mut dprng = StdRng::from_seed(crypto::sha256_seed_rex_key(&key_used));
    for chunk in &mut chunks
    {
        //FLATTEN CHUNK
        let mut flattened: Vec<i64> = chunk.iter().flatten().copied().collect();

        //SHUFFLE
        flattened.shuffle(&mut dprng);

        //REBUILD
        for (i, val) in flattened.into_iter().enumerate()
        {
            chunk[i / 8][i % 8] = val;
        }
    }

    //SHAPE KEY TO 8x8 GRID
    let mut key_grid = empty_grid();
    for i in 0..64
    {
        key_grid[i / 8][i % 8] = key_used[i] ^ key_used[i + 64]; //COMBINE EVERY PART OF KEY
    }

    //RETURN EMPTY DATA (ONLY TEST)
    RexData::empty()
}

pub fn encrypt_string(input: String, key: Option<Vec<i64>>) -> RexData //ENCRYPT STRING USING THE encrypt FN
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
