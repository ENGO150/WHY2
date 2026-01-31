/*
This is part of WHY2
Copyright (C) 2022-2026 Václav Šmejkal

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
    str::FromStr,
    fmt::Debug,
    path::Path,
    io::{ self, Cursor },
    fs::{ self, File },
};

use toml_edit::{ DocumentMut, Value };

use crate::{ options, misc };

#[cfg(feature = "client")]
use std::fmt::Write;

#[cfg(feature = "client")]
use crate::crypto;

//ENUMS
#[cfg(feature = "client")]
pub enum TofuCode //POSSIBLE KEY VERIFICATION RESULTS
{
    Valid, //KEY MATCHES LOCAL CONFIG
    Unknown(String, String), //KEY NOT FOUND IN CONFIG
    Mismatch, //KEY DIFFERS
}

//PRIVATE
fn config_path(filename: &str) -> String //GET CONFIGURATION PATH
{
    misc::get_why2_dir() + filename
}

fn fetch_config(filename: &str) -> String //FETCH CONFIG FROM GIT
{
    misc::fetch_data(&(options::CONFIG_URL.to_owned() + filename)).expect("Fetching config failed")
}

fn get_data(path: &str) -> DocumentMut //GET DocumentMut FROM path
{
    let content = fs::read_to_string(path).expect("Failed to read config"); //READ CONFIG FILE
    content.parse::<DocumentMut>().expect("Failed to parse config") //PARSE CONFIG & RETURN
}

fn config_read<T: FromStr>(filename: &str, key: &str) -> T //READ CONFIG
where
    T::Err: Debug,
{
    let data = get_data(&config_path(filename));

    //READ
    if let Some(value) = data.get(key) //FOUND IN CONFIG
    {
        //USE APPROPRIATE DATATYPE
        let string_value = match value.as_value().expect("Invalid config")
        {
            Value::String(s) => s.value().to_string(),
            Value::Integer(i) => i.value().to_string(),
            Value::Boolean(b) => b.value().to_string(),

            _ => panic!("Unsupported config datatype")
        };

        return string_value.parse::<T>().expect("Parsing config value failed");
    }

    //key NOT FOUND IN CONFIG, FETCH CONFIG AND INSERT NEW KEY
    let mut new_config: DocumentMut = fetch_config(filename).parse().expect("Failed to parse config");

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

fn config_write(filename: &str, key: &str, value: &str) //WRITE TO CONFIG
{
    let path = config_path(filename); //PATH TO CONFIG

    //GET data
    let mut data = get_data(&path);

    //WRITE
    let table = data.as_table_mut();
    if let Some(item) = table.get_mut(key)
    {
        *item.as_value_mut().expect("Updating config failed") = value.into();
    } else
    {
        table.insert(key, value.into());
    }

    //SAVE
    fs::write(&path, data.to_string()).expect("Saving config failed");
}

//PUBLIC
pub fn init_config() //INITIALIZE CONFIG FILES
{
    misc::check_directory(); //CREATE USER CONFIG DIRECTORY IF MISSING

    {
        let filename =
        {
            #[cfg(feature = "client")]
            {
                options::CLIENT_CONFIG
            }

            #[cfg(feature = "server")]
            {
                options::SERVER_CONFIG
            }
        };

        let config_path = config_path(filename);
        if !Path::new(&config_path).is_file()
        {
            let mut config_file = File::create(config_path).expect("Failed to create WHY2 config"); //CREATE CONFIG

            let mut config = Cursor::new(fetch_config(filename));
            io::copy(&mut config, &mut config_file).expect("Failed writing to config file");
        }
    }

    let runtime_path =
    {
        #[cfg(feature = "client")]
        {
            config_path(options::SERVER_KEYS_CONFIG)
        }

        #[cfg(feature = "server")]
        {
            config_path(options::SERVER_USERS_CONFIG)
        }
    };

    //CREATE RUNTIME CONFIG
    if !Path::new(&runtime_path).is_file()
    {
        fs::write(&runtime_path, "#*#**#*###**#***###*#").expect("Writing to config failed");
    }
}

pub fn read_config<T: FromStr>(key: &str) -> T //RETURN key FROM TOML CONFIG
where
    T::Err: Debug,
{
    #[cfg(feature = "client")]
    {
        config_read(options::CLIENT_CONFIG, key)
    }

    #[cfg(feature = "server")]
    {
        config_read(options::SERVER_CONFIG, key)
    }
}

#[cfg(feature = "server")]
pub fn server_users_config(key: &str) -> String //RETURN key FROM server_users.toml
{
    config_read(options::SERVER_USERS_CONFIG, key)
}

#[cfg(feature = "client")]
pub fn client_write(key: &str, value: &str) //WRITE TO client.toml
{
    config_write(options::CLIENT_CONFIG, key, value);
}

#[cfg(feature = "server")]
pub fn server_users_write(key: &str, value: &str) //WRITE TO server_users.toml
{
    config_write(options::SERVER_USERS_CONFIG, key, value);
}

#[cfg(feature = "server")]
pub fn server_users_contains(key: &str) -> bool //CHECK IF server_users.toml contains
{
    get_data(&config_path(options::SERVER_USERS_CONFIG)).get(key).is_some()
}

#[cfg(feature = "client")]
pub fn server_keys_check(host: &str, pubkey: &str) -> TofuCode //CHECK PUBKEY VALIDITY (TOFU)
{
    //HASH PUBKEY
    let pubkey_hash = crypto::sha256(pubkey);
    let mut pubkey_string = String::with_capacity(64);

    //SERIALIZE
    for byte in pubkey_hash
    {
        write!(pubkey_string, "{:02x}", byte).unwrap();
    }

    //PEER PUBKEY STORED, CHECK VALIDITY
    if get_data(&config_path(options::SERVER_KEYS_CONFIG)).get(host).is_some()
    {
        //COMPARE
        return if config_read::<String>(options::SERVER_KEYS_CONFIG, host) == pubkey_string
        {
            TofuCode::Valid
        } else
        {
            TofuCode::Mismatch
        }
    }

    TofuCode::Unknown(pubkey_string, host.to_string())
}

#[cfg(feature = "client")]
pub fn server_keys_save(host: &str, pubkey_hash: &str) //SAVE KEY
{
    //WRITE
    config_write(options::SERVER_KEYS_CONFIG, host, pubkey_hash);
}
