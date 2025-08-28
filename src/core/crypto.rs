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

use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

pub fn seed(seed_str: &str) -> [u8; 32]
{
    //HASH INTO u64
    let mut hasher = DefaultHasher::new();
    seed_str.hash(&mut hasher);
    let hash64 = hasher.finish();

    //FILL 32 BYTE ARRAY
    let mut seed = [0u8; 32];
    seed[..8].copy_from_slice(&hash64.to_le_bytes());
    seed
}
