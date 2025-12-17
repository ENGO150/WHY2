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
    fs,
    path::Path,
    time::Duration,
};

use reqwest::
{
    Error,
    blocking::Client,
    header::
    {
        HeaderMap,
        HeaderValue,
        USER_AGENT,
    },
};

use serde_json::Value;

use semver::Version;

use crate::chat::options;

//PRIVATE
fn get_dir(dir: &str) -> String
{
    dir.replace("{HOME}", dirs::home_dir().expect("Could not determine home directory").to_str().expect("Invalid home directory"))
}

fn get_config_dir() -> String
{
    get_dir(options::USER_CONFIG_DIR)
}

//PUBLIC
pub fn get_version<'a>() -> &'a str //GET COMPILED PACKAGE VERSION
{
    env!("CARGO_PKG_VERSION")
}

pub fn get_identifier() -> String //GET IDENTIFIER OF PACKAGE VERSION [WHY2/VERSION]
{
    format!("WHY2/{}", get_version())
}

pub fn fetch_data(url: &str) -> Result<String, Error> //FETCH DATA USING REQWEST
{
    //CUSTOM CLIENT HEADERS
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_str(&get_identifier()).expect("Invalid fetch request headers"));

    //BUILD CUSTOM CLIENT
    let client = Client::builder()
        .timeout(Duration::from_millis(options::FETCH_TIMEOUT))
        .default_headers(headers)
        .build()?;

    client.get(url).send()?.text()
}

pub fn check_version() //CHECK FOR LATEST WHY2 VERSION
{
    //FETCH METADATA (USE CUSTOM User-Agent, FOR CRATES.IO TO WORK)
    let metadata_raw = fetch_data(options::METADATA_URL).expect("Fetching versions failed");

    //PARSE METADATA TO JSON
    let metadata: Value = serde_json::from_str(&metadata_raw).expect("Parsing versions failed"); //PARSE
    let newest_version = metadata.get("crate") //GET LATEST VERSION
        .and_then(|c| c.get("newest_version"))
        .and_then(|v| v.as_str())
        .unwrap();

    //OUTDATED VERSION, CALCULATE HOW MANY NEWER VERSIONS EXIST
    let current_version = get_version();
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
            if Version::parse(version.get("num").and_then(|n| n.as_str()).unwrap()).unwrap() > current_version
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

pub fn clear_lines(n: usize) //CLEARS n LINES (ALSO MOVES THE CURSOR n LINES UP)
{
    for i in 0..n
    {
        //CLEAR CURRENT LINE
        print!("\x1B[2K\r");

        //MOVE UP
        if i < n - 1
        {
            print!("\x1B[1A");
        }
    }
}
