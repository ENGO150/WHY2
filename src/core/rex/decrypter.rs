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

use rand::
{
    SeedableRng,
    prelude::SliceRandom,
};

use rand_chacha::ChaCha20Rng;

use crate::core::
{
    misc,
    rex::
    {
        crypto,
        options::{ EncryptedData, DecryptedData },
    },
};

pub fn decrypt<const W: usize, const H: usize>(input: EncryptedData<W, H>) -> DecryptedData //ENCRYPT
{
    //CHECK FOR ACTIVE WHY2 VERSION
    misc::check_version();

    //GET MUTABLE input
    let mut grids = input.output;
    let key_grid = input.key;

    //GENERATE ROUND KEYS
    let round_keys = crypto::generate_round_keys(&key_grid);

    //DECRYPT EACH ENCRYPTED GRID
    for grid in &mut grids
    {
        //XOR WITH EACH ROUND KEY AND SHIFT ROWS & COLUMNS
        for (i, round_key) in round_keys[1..].iter().enumerate().rev()
        {
            grid.inv_mix_columns();         //UNMIX COLUMNS
            grid.inv_shift_rows(round_key); //UNSHIFT ROWS
            grid.inv_subcell(i);            //INVERT SUBCELL
            grid.xor_grids(round_key);      //XOR
        }

        //INITIAL XOR
        grid.xor_grids(&round_keys[0]);
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
    DecryptedData
    {
        output: flattened,
        key: key_grid.into_iter().collect(),
    }
}

pub fn decrypt_string<const W: usize, const H: usize>(input: EncryptedData<W, H>) -> String //DECRYPT, CONVERT output INTO STRING
{
    //DECRYPT
    let decrypted = decrypt(input).output;

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

    output
}
