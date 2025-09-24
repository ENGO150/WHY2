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
    distr::{ Alphanumeric, SampleString },
};

use crate::core::options::
{
    self,
    Version,
    RexGrid
};

//PRIVATE
fn generate_rex(length: usize, rng: &mut StdRng) -> Vec<i64>
{
    //FILL
    (0..length).map(|_|
    {
        let mut bytes = [0u8; 8];
        rng.try_fill_bytes(&mut bytes).expect("Failed to generate random bytes");
        i64::from_ne_bytes(bytes)
    }).collect()
}

//PUBLIC
pub fn sha256_seed(seed_str: &str) -> [u8; 32] //GET HASH SEED; USED FOaR PADDING
{
    //SHA256
    let mut hasher = Sha256::new();
    hasher.update(seed_str.as_bytes());

    //FINALIZE
    hasher.finalize().into()
}

pub fn sha256_seed_rex_key(key: &RexGrid) -> [u8; 32] //GET HASH SEED; USED FOR SHUFFLING REX GRID
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

pub fn recommended_padding_rate(input_length: usize) -> usize //NORMAL PADDING RATE - 1 PADDING TO 3 CHARS
{
    input_length / 3
}

pub fn generate_key(length: usize) -> String //GENERATE WHY2 SYMMETRIC KEY
{
    Alphanumeric.sample_string(&mut rand::rng(), length)
}

pub fn generate_text_key_chain(key: &str, size: usize) -> Vec<i64> //GENERATE tkch, USED FOR ENCRYPTION/DECRYPTION
{
    //VARIABLES
    let mut number_buffer: usize;
    let mut number_buffer_2: usize;
    let mut number_buffer_3: usize;
    let core_options = options::get_core_options();
    let key_length = core_options.key_length;
    let mut text_key_chain: Vec<i64> = vec![0; size];
    let key_bytes = key.as_bytes();

    for i in 0..size
    {
        number_buffer = i % key_length;

        //USE CORRECT VERSION
        match core_options.version
        {
            Version::V1 =>
            {
                number_buffer_2 = i;
                number_buffer_3 = number_buffer + (i < size) as usize;
            },

            Version::V2 =>
            {
                number_buffer_2 = i;
                number_buffer_3 = key_length - (number_buffer + (i < size) as usize);
            },

            Version::V3 =>
            {
                number_buffer_2 = size - (i + 1);
                number_buffer_3 = key_length - (number_buffer + (i < size) as usize);
            },

            Version::V4 =>
            {
                number_buffer_2 = size - (i + 1);
                number_buffer_3 = ((((((i ^ number_buffer_2) + ((number_buffer << 3) ^ (number_buffer_2 & 0xF))) * (size ^ (key_length >> 2))) ^ ((!(number_buffer + size)) & 0xA7)) + (i % 7)) * (((number_buffer_2 | (i & 0xF)) + (key_length >> 3)) ^ (size * (number_buffer & 0x3F))) + (((i << 4) ^ (size >> 1)) & 0x1234) - ((i * number_buffer_2) % (key_length | size))) % key_length; //gl fucker
            },
        }

        //VALUES
        let a = key_bytes[number_buffer] as i64;
        let b = key_bytes[number_buffer_3] as i64;

        //GET MATCHING OPERATION BETWEEN VALUES
        let val = if core_options.version == Version::V4 && (number_buffer + 1) % 4 == 0
        {
            a % b.max(1)
        } else if (number_buffer + 1) % 3 == 0
        {
            a * b
        } else if (number_buffer + 1) % 2 == 0
        {
            a.wrapping_sub(b)
        } else
        {
            a.wrapping_add(b)
        };

        //SET
        text_key_chain[number_buffer_2] = val;
    }

    text_key_chain
}

pub fn generate_rex_key(length: usize) -> Vec<i64> //GENERATE WHY2 SYMMETRIC KEY
{
    //CREATE MUTABLE INSANCE OF OsRng
    let mut rng = StdRng::from_os_rng();
    generate_rex(length, &mut rng)
}

pub fn generate_rex_round_keys(master_key: &RexGrid) -> Vec<RexGrid>
{
    let mut dprng = StdRng::from_seed(sha256_seed_rex_key(master_key));
    generate_rex(128, &mut dprng);
    Vec::new()
}
