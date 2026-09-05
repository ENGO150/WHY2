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

//MODULES
#[cfg(feature = "server")]
pub mod users;

#[cfg(feature = "server")]
pub mod bans;

#[cfg(feature = "server")]
pub mod settings;

#[cfg(feature = "server")]
pub mod messages;

#[cfg(feature = "client_base")]
pub mod keys;

use std::
{
    fmt::Debug,
    path::Path,
    str::FromStr,
    collections::HashMap,
    io::{ self, Cursor },
    fs::{ self, File },
    sync::{ LazyLock, Mutex }
};

use toml_edit::
{
    DocumentMut,
    Item,
    Table,
    Value,
};

use crate::{ consts, misc };

//PRIVATE
//GLOBAL VARIABLES
static CONFIG_CACHE: LazyLock<Mutex<HashMap<String, DocumentMut>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

//FUNCTIONS
fn config_path(filename: &str) -> String //GET CONFIGURATION PATH
{
    misc::get_why2_dir() + filename
}

fn get_config() -> &'static str //GET CONFIG FROM BINARY
{
    //TODO: FIGURE OUT A BETTER WAY TO USE CONSTANTS
    #[cfg(feature = "client_base")]
    {
        include_str!("./client.toml")
    }

    #[cfg(feature = "server")]
    {
        include_str!("./server.toml")
    }
}

fn get_data(path: &str) -> DocumentMut //GET DocumentMut FROM path
{
    //GET CONFIG CACHE
    let mut cache = CONFIG_CACHE.lock().unwrap();
    if let Some(doc) = cache.get(path)
    {
        //RETURN IF CACHED
        return doc.clone();
    }

    let content = fs::read_to_string(path).expect("Failed to read config"); //READ CONFIG FILE
    let doc = content.parse::<DocumentMut>().expect("Failed to parse config"); //PARSE DOCUMENT

    cache.insert(path.to_string(), doc.clone());
    doc
}

fn with_cached_mut<F: FnOnce(&mut DocumentMut)>(path: &str, f: F)
{
    //LOAD CACHE IF MISSING
    let mut cache = CONFIG_CACHE.lock().unwrap();
    if !cache.contains_key(path)
    {
        let content = fs::read_to_string(path).expect("Failed to read config");
        cache.insert(path.to_string(), content.parse().expect("Failed to parse config"));
    }

    let doc = cache.get_mut(path).unwrap();
    f(doc);

    fs::write(path, doc.to_string()).expect("Saving config failed"); //WRITE
}

fn config_read<T: FromStr>(filename: &str, key: &str) -> T //READ CONFIG
where
    T::Err: Debug,
{
    let path = config_path(filename);
    let data = get_data(&path);

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

    //KEYS NOT FOUND IN CONFIG, FETCH CONFIG AND INSERT NEW KEY
    let mut default_config: DocumentMut = get_config().parse().expect("Failed to parse config");
    for (key, old_value) in data.as_table()
    {
        //NEW CONFIG CONTAINS SAME KEY AS THE OLD ONE, USE OLD VALUE
        if let Some(item) = default_config.get_mut(key)
        {
            //COPY OLD VALUE
            *item.as_value_mut().expect("Updating config failed") = old_value.as_value().expect("Invalid config").clone();
        }
    }

    //UPDATE
    with_cached_mut(&path, |doc| *doc = default_config);

    //REPEAT
    config_read(filename, key)
}

fn set_value(table: &mut Table, key: &str, value: Value) //ASSIGN ONE KEY, KEEPING THE COMMENTS AROUND IT
{
    if let Some(item) = table.get_mut(key)
    {
        //KEEP THE TRAILING COMMENT THE DEFAULT CONFIG SHIPPED WITH
        let decor = item.as_value().map(|old| old.decor().clone());
        let mut value = value;

        if let Some(decor) = decor { *value.decor_mut() = decor; }

        *item.as_value_mut().expect("Updating config failed") = value;
    } else
    {
        table.insert(key, Item::Value(value));
    }
}

#[cfg(feature = "client_base")]
fn config_write_value(filename: &str, key: &str, value: Value) //WRITE TYPED VALUE TO CONFIG
{
    //WRITE
    with_cached_mut(&config_path(filename), |doc| set_value(doc.as_table_mut(), key, value));
}

#[cfg(feature = "client_base")]
fn config_write(filename: &str, key: &str, value: &str) //WRITE TO CONFIG
{
    config_write_value(filename, key, value.into());
}

//PUBLIC
pub fn init_config() //INITIALIZE CONFIG FILES
{
    misc::check_directory(); //CREATE USER CONFIG DIRECTORY IF MISSING

    {
        let filename =
        {
            #[cfg(feature = "client_base")]
            {
                consts::CLIENT_CONFIG
            }

            #[cfg(feature = "server")]
            {
                consts::SERVER_CONFIG
            }
        };

        let config_path = config_path(filename);
        if !Path::new(&config_path).is_file()
        {
            let mut config_file = File::create(config_path).expect("Failed to create WHY2 config"); //CREATE CONFIG

            let mut config = Cursor::new(get_config());
            io::copy(&mut config, &mut config_file).expect("Failed writing to config file");
        }
    }

    let runtime_paths =
    {
        #[cfg(feature = "client_base")]
        {
            vec![config_path(consts::SERVER_KEYS_CONFIG)]
        }

        #[cfg(feature = "server")]
        {
            vec![config_path(consts::SERVER_USERS_CONFIG), config_path(consts::SERVER_BANS_CONFIG)]
        }
    };

    //CREATE RUNTIME CONFIGS
    for runtime_path in &runtime_paths
    {
        if !Path::new(runtime_path).is_file()
        {
            fs::write(runtime_path, "#*#**#*###**#***###*#").expect("Writing to config failed");
        }
    }
}

pub fn read_config<T: FromStr>(key: &str) -> T //RETURN key FROM TOML CONFIG
where
    T::Err: Debug,
{
    #[cfg(feature = "client_base")]
    {
        config_read(consts::CLIENT_CONFIG, key)
    }

    #[cfg(feature = "server")]
    {
        config_read(consts::SERVER_CONFIG, key)
    }
}

#[cfg(feature = "client_base")]
pub fn client_write(key: &str, value: &str) //WRITE TO client.toml
{
    config_write(consts::CLIENT_CONFIG, key, value);
}

#[cfg(feature = "client_base")]
pub fn client_write_bool(key: &str, value: bool) //WRITE BOOLEAN TO client.toml
{
    config_write_value(consts::CLIENT_CONFIG, key, value.into());
}

#[cfg(feature = "client_base")]
pub fn client_write_int(key: &str, value: i64) //WRITE INTEGER TO client.toml
{
    config_write_value(consts::CLIENT_CONFIG, key, value.into());
}
