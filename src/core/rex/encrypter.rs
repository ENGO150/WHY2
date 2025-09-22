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

use crate::core::
{
    misc,
    rex::
    {
        misc as rex_misc,
        options::RexData,
    },
};

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
            if k.len() != 256 { return RexData::empty(); }

            //USE KEY IF MATCHING LENGTH
            k
        },

        //NO KEY, GENERATE ONE
        None =>
        {
            rex_misc::generate_key(256)
        }
    };

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
