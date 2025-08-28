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
    env,
    fs,
    path::Path,
};

use curl::easy::Easy;
use serde_json::Value;
use rand::distr::{ Alphanumeric, SampleString };

use crate::core::options;

//PRIVATE
fn __get_dir(dir: &str) -> String
{
    dir.replace("{HOME}", env::home_dir().expect("Could not determine home directory").to_str().expect("Invalid home directory"))
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
pub fn check_version()
{
    if options::get_core_options().no_check { return; } //CHECK DISABLED

    check_directory(); //MAKE SURE WHY2 DIR EXISTS

    if options::get_core_options().no_output { return; } //NO OUTPUT WANTED - MEANING THIS WHOLE FUNCTION WOULD BE POINTLESS

    //DOWNLOAD versions.json
    let versions_text =
    {
        let mut buffer = String::new();
        let mut easy = Easy::new();
        easy.url(options::VERSIONS_URL).expect("Invalid URL"); //SET URL

        {
            let mut transfer = easy.transfer();
            transfer.write_function(|data|
            {
                buffer.push_str(str::from_utf8(data).expect("Invalid versions.json")); //LOAD INTO STRING
                Ok(data.len())
            }).expect("Reading versions.json failed");
            transfer.perform().expect("Downloading versions.json failed");
        }

        buffer
    };

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

    if !Path::new(&(config.clone() + options::CONFIG_DIR)).is_dir() { fs::create_dir_all(config + options::CONFIG_DIR).expect("Failed to create WHY2 config directory"); } //CREATE WHY2 CONFIG DIRECTORY
}

pub fn generate_key(length: usize) -> String
{
    Alphanumeric.sample_string(&mut rand::rng(), length)
}
