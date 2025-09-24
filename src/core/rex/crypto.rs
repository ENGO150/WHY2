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

use rand::
{
    SeedableRng,
    TryRngCore,
    rngs::StdRng,
};

use crate::core::rex::
{
    misc,
    options::{ self, Grid },
};

//PRIVATE
fn generate_key_handler(rng: &mut StdRng) -> Vec<i64>
{
    //FILL
    (0..(2 * options::REX_GRID_DIMENSIONS.0 * options::REX_GRID_DIMENSIONS.1)).map(|_|
    {
        let mut bytes = [0u8; 8];
        rng.try_fill_bytes(&mut bytes).expect("Failed to generate random bytes");
        i64::from_ne_bytes(bytes)
    }).collect()
}

//PUBLIC
pub fn sha256_seed_grid(key: &Grid) -> [u8; 32] //GET HASH SEED; USED FOR SHUFFLING REX GRID
{
    //SHA256
    let mut hasher = Sha256::new();

    //ADD TO HASH
    for row in key
    {
        for val in row
        {
            hasher.update(&val.to_ne_bytes());
        }
    }

    //FINALIZE
    hasher.finalize().into()
}

pub fn generate_key() -> Vec<i64> //GENERATE WHY2 SYMMETRIC KEY
{
    //CREATE MUTABLE INSANCE OF OsRng
    generate_key_handler(&mut StdRng::from_os_rng())
}

pub fn generate_round_keys(master_key: &Grid) -> Vec<Grid> //GENERATE 'RANDOM' ROUND KEYS BASED ON MASTER KEY
{
    let mut keys: Vec<Grid> = Vec::with_capacity(options::REX_ROUND_KEYS);

    //GENERATE KEYS
    for _ in 0..(options::REX_ROUND_KEYS)
    {
        //USE SEED OF LAST KEY TO GENERATE NEW KEY
        let key = generate_key_handler(&mut StdRng::from_seed(sha256_seed_grid(keys.last().unwrap_or(master_key))));

        //CONVERT KEY TO Grid & PUSH TO keys
        keys.push(misc::shape_key(key));
    }

    keys
}
