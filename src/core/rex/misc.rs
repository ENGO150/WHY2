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
    TryRngCore,
    rngs::OsRng,
};

use crate::core::rex::options::RexData;

//IMPLEMENTATIONS
impl RexData
{
    //CREATE EMPTY RexData
    pub fn empty() -> Self
    {
        Self
        {
            output: None,
            key: None,
        }
    }
}

//FUNCTIONS
pub fn generate_key(length: usize) -> Vec<i64> //GENERATE WHY2 SYMMETRIC KEY
{
    //CREATE MUTABLE INSANCE OF OsRng
    let mut rng = OsRng;

    //FILL
    (0..length).map(|_|
    {
        let mut bytes = [0u8; 8];
        rng.try_fill_bytes(&mut bytes).expect("Failed to generate random bytes");
        i64::from_ne_bytes(bytes)
    }).collect()
}
