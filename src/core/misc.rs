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

use std::
{
    str,
    fs,
    path::Path,
};

use serde_json::Value;
use rand::distr::{ Alphanumeric, SampleString };

use crate::core::
{
    options,

    options::
    {
        Version,
        EncryptedData,
        DecryptedData,
    },
};

//PRIVATE
fn __get_dir(dir: &str) -> String
{
    dir.replace("{HOME}", dirs::home_dir().expect("Could not determine home directory").to_str().expect("Invalid home directory"))
}

fn get_config_dir() -> String
{
    __get_dir(options::USER_CONFIG_DIR)
}

fn get_why2_dir() -> String
{
    get_config_dir() + options::CONFIG_DIR
}

//PUBLIC
//IMPLEMENTATIONS
impl EncryptedData
{
    //CREATE EMPTY Data
    pub fn empty() -> Self
    {
        Self
        {
            output: None,
            key: None,
        }
    }

    //CREATE Data
    pub fn from(output: Vec<i64>, key: String) -> Self
    {
        Self
        {
            output: Some(output),
            key: Some(key),
        }
    }
}

impl DecryptedData
{
    //CREATE EMPTY Data
    pub fn empty() -> Self
    {
        Self
        {
            output: None,
            key: None,
        }
    }

    //CREATE Data
    pub fn from(output: String, key: String) -> Self
    {
        Self
        {
            output: Some(output),
            key: Some(key),
        }
    }
}

//FUNCTIONS
pub fn check_version()
{
    let core_options = options::get_core_options();
    if core_options.no_check { return; } //CHECK DISABLED

    check_directory(); //MAKE SURE WHY2 DIR EXISTS

    if core_options.no_output { return; } //NO OUTPUT WANTED - MEANING THIS WHOLE FUNCTION WOULD BE POINTLESS

    //DOWNLOAD versions.json
    let versions_text = reqwest::blocking::get(options::VERSIONS_URL)
        .expect("Failed to fetch versions.json")
        .text()
        .expect("Failed to read versions.json");

    let versions_json: Value = serde_json::from_str(&versions_text).expect("Parsing versions.json failed"); //PARSE versions_text INTO JSON
    let active_version = versions_json["active"].as_str().expect("Invalid versions.json scheme");

    if options::VERSION != active_version
    {
        let deprecated = versions_json["deprecated"].as_array().expect("Invalid versions.json scheme"); //GET LIST OF ALL PAST VERSIONS
        let pos = deprecated.iter().position(|x| x == options::VERSION).expect("Current version not found"); //COUNT WHERE IN TF HISTORY ARE YOU

        eprintln!("This release could be unsafe! You are {} versions behind! ({}/{})", deprecated.iter().skip(pos).count(), options::VERSION, active_version);
    }
}

pub fn check_directory()
{
    let config = get_config_dir();

    //CREATE WHY2 CONFIG DIRECTORY
    if !Path::new(&config).join(options::CONFIG_DIR).is_dir()
    {
        fs::create_dir_all(config + options::CONFIG_DIR).expect("Failed to create WHY2 config directory");
    }
}

pub fn generate_key(length: usize) -> String
{
    Alphanumeric.sample_string(&mut rand::rng(), length)
}

pub fn generate_text_key_chain(key: &str, size: usize) -> Vec<i64>
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
