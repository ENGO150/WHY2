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

//PRIVATE
fn config_path(filename: &str) -> String //GET CONFIGURATION PATH
{
    misc::get_why2_dir() + filename
}

fn init_config(filename: &str) //CREATE CONFIG IF MISSING
{
    misc::check_directory(); //CREATE USER CONFIG DIRECTORY IF MISSING

    let config_path = config_path(filename);
    if !Path::new(&config_path).is_file()
    {
        let mut config_file = File::create(config_path).expect("Failed to create WHY2 config"); //CREATE CONFIG

        let mut config = reqwest::blocking::get(options::CONFIG_URL.to_owned() + filename).expect("Failed to fetch config file");
        io::copy(&mut config, &mut config_file).expect("Failed writing to config file");
    }
}

fn get_data(path: &str) -> toml::Value //GET Value FROM path
{
    let content = fs::read_to_string(path).expect("Failed to read config"); //READ CONFIG FILE
    toml::from_str(&content).expect("Failed to parse config") //PARSE CONFIG & RETURN
}

fn config_read(filename: &str, key: &str) -> String //READ CONFIG
{
    get_data(&config_path(filename)).get(key).expect("Key not found").to_string().replace("\"", "").trim().to_string()
}

//PUBLIC
pub fn init_server_config() //INITIALIZE SERVER CONFIG FILES
{
    init_config(options::SERVER_CONFIG); //DOWNLOAD server.toml

    let users_dir_path = config_path(options::SERVER_USERS_CONFIG);
    if !Path::new(&users_dir_path).is_file()
    {
        //WRITE SOMETHING POSITIVE TO THE CONFIG :) (i love you, ignore my aggressive ass)
        fs::write(&users_dir_path, "#haha no users registered, what a loser lol").expect("Writing to config failed");
    }
}

pub fn init_client_config()
{
    init_config(options::CLIENT_CONFIG); //DOWNLOAD client.toml
}

pub fn server_config(key: &str) -> String //RETURN key FROM server.toml
{
    config_read(options::SERVER_CONFIG, key)
}

pub fn client_config(key: &str) -> String //RETURN key FROM client.toml
{
    config_read(options::CLIENT_CONFIG, key)
}

pub fn server_users_config(key: &str) -> String //RETURN key FROM server_users.toml
{
    config_read(options::SERVER_USERS_CONFIG, key)
}

pub fn server_users_write(key: &str, value: &str) //WRITE TO server_users.toml
{
    let path = config_path(options::SERVER_USERS_CONFIG); //PATH TO server_users.toml

    //GET data
    let mut data = get_data(&path);

    //WRITE
    data.as_table_mut().expect("Writing to config failed").insert(key.to_string(), value.into());

    //SAVE
    fs::write(&path, toml::to_string(&data).expect("Parsing config failed")).expect("Saving config failed");
}

pub fn server_users_contains(key: &str) -> bool //CHECK IF server_users.toml contains
{
    get_data(&config_path(options::SERVER_USERS_CONFIG)).get(key).is_some()
}
