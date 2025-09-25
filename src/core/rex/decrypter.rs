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
    misc,
    rex::
    {
        crypto,
        misc as rex_misc,
        options::
        {
            self,
            Grid,
            Data,
        },
    },
};

pub fn decrypt(input: Data) -> Data //ENCRYPT
{
    //CHECK FOR ACTIVE WHY2 VERSION
    misc::check_version();

    //GET MUTABLE input
    let mut chunks = input.output;
    let key_grid = input.key;

    //GENERATE ROUND KEYS
    let round_keys = crypto::generate_round_keys(&key_grid);

    //DECRYPT EACH ENCRYPTED GRID
    for chunk in &mut chunks
    {
        //XOR WITH EACH ROUND KEY AND SHIFT ROWS & COLUMNS
        for (i, round_key) in round_keys[1..].iter().enumerate().rev()
        {
            rex_misc::inv_mix_columns(chunk);           //UNMIX COLUMNS
            rex_misc::inv_shift_rows(chunk, round_key); //UNSHIFT ROWS
            rex_misc::inv_subcell(chunk, i);            //INVERT SUBCELL
            rex_misc::xor_grids(chunk, round_key);      //XOR
        }

        //INITIAL XOR
        rex_misc::xor_grids(chunk, &round_keys[0]);
    }

    //RETURN OUTPUT
    Data
    {
        output: Vec::new(),
        key: rex_misc::empty_grid(),
    }
}
