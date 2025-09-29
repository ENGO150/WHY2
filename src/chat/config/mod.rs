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

use reqwest::blocking::Response;

use toml_edit::{ DocumentMut, Value };

use crate::chat::
{
    options,
    misc,
};

//PRIVATE
fn config_path(filename: &str) -> String //GET CONFIGURATION PATH
{
    misc::get_why2_dir() + filename
}

fn fetch_config(filename: &str) -> Response //FETCH CONFIG FROM GIT
{
    reqwest::blocking::get(options::CONFIG_URL.to_owned() + filename).expect("Failed to fetch config file")
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

fn get_data(path: &str) -> DocumentMut //GET DocumentMut FROM path
{
    let content = fs::read_to_string(path).expect("Failed to read config"); //READ CONFIG FILE
    content.parse::<DocumentMut>().expect("Failed to parse config") //PARSE CONFIG & RETURN
}

fn config_read(filename: &str, key: &str) -> String //READ CONFIG
{
    let data = get_data(&config_path(filename));

    //READ
    if let Some(value) = data.get(key) //FOUND IN CONFIG
    {
        //USE APPROPRIATE DATATYPE
        return match value.as_value().expect("Invalid config")
        {
            Value::String(s) => s.value().to_string(),
            Value::Integer(i) => i.value().to_string(),
            Value::Boolean(b) => b.value().to_string(),

            _ => panic!("Unsupported config datatype")
        }
    }

    //key NOT FOUND IN CONFIG, FETCH CONFIG AND INSERT NEW KEY
    let mut new_config: DocumentMut = fetch_config(filename).text().expect("Failed to fetch config file").parse().expect("Failed to parse config");

    //LOAD OLD CONFIG
    for (key, old_value) in data.as_table()
    {
        //NEW CONFIG CONTAINS SAME KEY AS THE OLD ONE, USE OLD VALUE
        if let Some(item) = new_config.get_mut(key)
        {
            //COPY OLD VALUE
            *item.as_value_mut().expect("Updating config failed") = old_value.as_value().expect("Invalid config").clone();
        }
    }

    //UPDATE
    fs::write(&config_path(&filename), new_config.to_string()).expect("Updating config file failed");

    //REPEAT
    config_read(filename, key)
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
    data.as_table_mut().insert(key, value.into());

    //SAVE
    fs::write(&path, data.to_string()).expect("Saving config failed");
}

pub fn server_users_contains(key: &str) -> bool //CHECK IF server_users.toml contains
{
    get_data(&config_path(options::SERVER_USERS_CONFIG)).get(key).is_some()
}
