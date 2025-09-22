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

use sha2::{ Sha256, Digest };

pub fn sha256_seed(seed_str: &str) -> [u8; 32] //GET HASH SEED; USED FOR PADDING
{
    //SHA256
    let mut hasher = Sha256::new();
    hasher.update(seed_str.as_bytes());

    //FINALIZE
    hasher.finalize().into()
}

pub fn sha256_seed_rex_key(key: &Vec<i64>) -> [u8; 32] //GET HASH SEED; USED FOR SHUFFLING REX GRID
{
    //SHA256
    let mut hasher = Sha256::new();

    //ADD TO HASH
    for &val in key
    {
        hasher.update(&val.to_ne_bytes());
    }

    //FINALIZE
    hasher.finalize().into()
}

pub fn recommended_padding_rate(input_length: usize) -> usize //NORMAL PADDING RATE - 1 PADDING TO 3 CHARS
{
    input_length / 3
}
