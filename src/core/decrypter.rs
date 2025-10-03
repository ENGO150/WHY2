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

#![allow(deprecated)]

use crate::core::
{
    crypto,
    options::
    {
        self,
        EncryptedData,
        DecryptedData,
    },
};

use rand::
{
    Rng,
    SeedableRng,
    rngs::StdRng,
};

pub fn decrypt_text(encrypted_data: EncryptedData) -> DecryptedData //DECRYPT
{
    //VARIABLES
    let core_options = options::get_core_options(); //CORE OPTIONS
    let text_encrypted = encrypted_data.output; //ENCRYPTED VECTOR
    let text_encrypted_length = text_encrypted.len();
    let key = encrypted_data.key; //KEY USED FOR ENCRYPTION
    let mut text_decrypted: Vec<u32> = vec![0; text_encrypted_length];

    //LOAD text_key_chain
    let text_key_chain = crypto::generate_text_key_chain(&key, text_encrypted_length);

    //ACTUALLY ENCRYPT TEXT
    for i in 0..text_encrypted_length
    {
        text_decrypted[i] = (core_options.encryption_operation)(text_key_chain[i], text_encrypted[i]) as u32;
    }

    //PADDING
    if core_options.padding > 0
    {
        //CREATE DETERMINISTIC RANDOM GENERATOR
        let mut drng = StdRng::from_seed(crypto::sha256_seed(&key));

        //GET RANDOM SEQUENCE USED IN ENCRYPTION
        let mut sequence: Vec<usize> = vec![0; core_options.padding];
        for i in 0..core_options.padding
        {
            //GENERATE "RANDOM" POSITION
            let random_position = drng.random_range(0..text_encrypted_length - core_options.padding + i);

            //ADD TO VECTOR
            sequence[i] = random_position;
        }
        sequence.reverse(); //REVERSE VECTOR

        //REMOVE PADDING
        for s in sequence
        {
            text_decrypted.remove(s);
        }
    }

    //RETURN DATA
    DecryptedData
    {
        output: text_decrypted.iter().filter_map(|&c| char::from_u32(c)).collect(),
        key: key,
    }
}
