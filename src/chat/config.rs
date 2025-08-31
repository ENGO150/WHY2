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
    io,
    path::Path,
    fs::{ self, File },
};

use crate::
{
    core::misc,
    chat::options,
};

//ENUMS
enum ConfigType //TYPE OF CHAT CONFIGS
{
    Client,
    Server,
    ServerUsers,
    Authority,
}

//PRIVATE
fn init_config(filename: &str)
{
    misc::check_directory(); //CREATE USER CONFIG DIRECTORY IF MISSING

    let config_path = misc::get_why2_dir() + filename; //GET PATH

    if !Path::new(&config_path).is_file()
    {
        let mut config_file = File::create(config_path).expect("Failed to create WHY2 config"); //CREATE CONFIG

        let mut config = reqwest::blocking::get(options::CONFIG_URL.to_owned() + filename).expect("Failed to fetch config file");
        io::copy(&mut config, &mut config_file).expect("Failed writing to config file");
    }
}

fn config_path(config_type: ConfigType) -> String
{
    //GET CONFIGURATION PATH
    misc::get_why2_dir() + (match config_type
    {
        ConfigType::Client => options::CLIENT_CONFIG,
        ConfigType::Server => options::SERVER_CONFIG,
        ConfigType::ServerUsers => options::SERVER_USERS_CONFIG,
        ConfigType::Authority => options::AUTHORITY_DIR,
    })
}

fn config(key: &str, config_type: ConfigType) -> String
{
    toml_read(&config_path(config_type), key)
}

//PUBLIC
pub fn init_server_config() //INITIALIZE SERVER CONFIG FILES
{
    init_config(options::SERVER_CONFIG); //DOWNLOAD server.toml

    let users_dir_path = get_server_users_path();
    if server_config("user_pick_username") == "true" && !Path::new(&users_dir_path).is_dir()
    {
        //WRITE SOMETHING POSITIVE TO THE CONFIG :) (i love you, ignore my aggressive ass)
        fs::write(&users_dir_path, "#haha no users registered, what a loser lol").expect("Writing to config failed");
    }
}

pub fn server_config(key: &str) -> String //RETURN key FROM server.toml
{
    config(key, ConfigType::Server)
}

pub fn get_server_users_path() -> String //ik, the function names are really weird and may not be helping you, but this returns path to server_users.toml
{
    config_path(ConfigType::ServerUsers)
}

pub fn toml_read(path: &str, key: &str) -> String //READ TOML FILE
{
    let content = fs::read_to_string(path).expect("Failed to read config"); //READ CONFIG FILE
    let data: toml::Value = toml::from_str(&content).expect("Failed to parse config"); //PARSE CONFIG

    data.get(key).expect("Key not found").to_string().replace("\"", "").trim().to_string()
}
