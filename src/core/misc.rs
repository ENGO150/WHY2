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

use reqwest::blocking::Client;

use serde_json::Value;

use semver::Version;

use crate::core::options;

//PRIVATE
fn __get_dir(dir: &str) -> String
{
    dir.replace("{HOME}", dirs::home_dir().expect("Could not determine home directory").to_str().expect("Invalid home directory"))
}

fn get_config_dir() -> String
{
    __get_dir(options::USER_CONFIG_DIR)
}

//PUBLIC
pub fn check_version() //CHECK FOR LATEST WHY2 VERSION
{
    let core_options = options::get_core_options();
    if core_options.no_check { return; } //CHECK DISABLED

    check_directory(); //MAKE SURE WHY2 DIR EXISTS

    if core_options.no_output { return; } //NO OUTPUT WANTED - MEANING THIS WHOLE FUNCTION WOULD BE POINTLESS

    //FETCH METADATA (USE CUSTOM User-Agent, FOR CRATES.IO TO WORK)
    let client = Client::new();
    let metadata_raw = client.get(options::METADATA_URL)
        .header("User-Agent", "why2-version-check")
        .send().expect("Sending metadata request failed")
        .text().expect("Fetching metadata failed");

    //PARSE METADATA TO JSON
    let metadata: Value = serde_json::from_str(&metadata_raw).expect("Parsing versions.json failed"); //PARSE
    let newest_version = metadata.get("crate") //GET LATEST VERSION
        .and_then(|c| c.get("newest_version"))
        .and_then(|v| v.as_str())
        .unwrap();

    //OUTDATED VERSION, CALCULATE HOW MANY NEWER VERSIONS EXIST
    let current_version = env!("CARGO_PKG_VERSION");
    if current_version != newest_version
    {
        //GET ARRAY OF VERSIONS
        let versions = metadata.get("versions").and_then(|v| v.as_array()).unwrap();
        let mut newer_versions = 0;
        let current_version = Version::parse(current_version).expect("Invalid version");

        //CALCULATE
        for version in versions
        {
            //FOUND NEWER VERSION
            if Version::parse(version.get("num").and_then(|n| n.as_str()).expect("")).expect("") > current_version
            {
                //INCREMENT COUNTER
                newer_versions += 1;
            }
        }

        println!("This release could be unsafe! You are {newer_versions} versions behind! ({current_version}/{newest_version})");
    }
}

pub fn check_directory() //CREATE WHY2 CONFIG DIRECTORY
{
    let config = get_config_dir() + options::CONFIG_DIR;

    //CREATE WHY2 CONFIG DIRECTORY
    if !Path::new(&config).is_dir()
    {
        fs::create_dir_all(config).expect("Failed to create WHY2 config directory");
    }
}

pub fn get_why2_dir() -> String //RETURN PATH TO WHY2 CONFIG DIRECTORY
{
    get_config_dir() + options::CONFIG_DIR
}
