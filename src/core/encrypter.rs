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
    crypto,
    misc,
    options,
    options::EncryptedData,
};

use rand::
{
    Rng,
    SeedableRng,
    rngs::StdRng,
};

pub fn encrypt_text(text: &str, key: &str) -> EncryptedData
{
    //CHECK FOR ACTIVE WHY2 VERSION
    misc::check_version();

    let core_options = options::get_core_options(); //CORE OPTIONS

    //GET key_used
    let key_used = if !key.is_empty() //key WAS PASSED TO FUNCTION
    {
        //CHECK FOR INVALID [SHORT] key
        if key.len() < core_options.key_length { return EncryptedData::empty(); }

        key.to_owned()
    } else //NO key, GENERATE ONE
    {
        misc::generate_key(core_options.key_length)
    };

    let mut text_used = text.to_owned();

    //PADDING
    if core_options.padding > 0
    {
        //CONVERT text_used TO VECTOR OR CHARS
        let mut split_text: Vec<char> = text_used.chars().collect();

        //CREATE DETERMINISTIC RANDOM GENERATOR
        let mut drng = StdRng::from_seed(crypto::sha256_seed(&key_used));
        let mut rng = rand::rng(); //this one probably shouldn't be deterministic lmao

        //INSERT PADDING
        for _ in 0..(core_options.padding)
        {
            //GENERATE "RANDOM" POSITION AND RANDOM CHARACTER
            let random_position = drng.random_range(0..(split_text.len()));
            let random_char = loop
            {
                let c: char = rng.random::<char>(); //GENERATE
                if c.is_control() { continue; } //DO NOT USE CONTROL CHARS

                break c;
            };

            //INSERT TO VECTOR
            split_text.insert(random_position, random_char);
        }

        //REBUILD AND OVERWRITE ORIGINAL text_used
        text_used = split_text.iter().collect();
    }

    let text_used_chars: Vec<i64> = text_used.chars().map(|c| c as i64).collect();
    let text_used_length = text_used_chars.len(); //LENGTH OF TEXT (+ PADDING)

    //LOAD text_key_chain
    let mut text_key_chain = misc::generate_text_key_chain(&key_used, text_used_length);

    //ACTUALLY ENCRYPT TEXT
    for i in 0..text_used_length
    {
        text_key_chain[i] = (core_options.encryption_operation)(text_key_chain[i], text_used_chars[i]);
    }

    //RETURN DATA
    EncryptedData::from(text_key_chain, key_used)
}
