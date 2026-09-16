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
    time::SystemTime,
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
//STRUCTS
struct Cached //A PARSED CONFIG AND THE FILE IT CAME FROM
{
    doc: DocumentMut,
    stamp: Option<(SystemTime, u64)>,
}

//GLOBAL VARIABLES
static CONFIG_CACHE: LazyLock<Mutex<HashMap<String, Cached>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

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

fn stamp(path: &str) -> Option<(SystemTime, u64)> //FILE FINGERPRINT
{
    let metadata = fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

fn load<'a>(cache: &'a mut HashMap<String, Cached>, path: &str) -> &'a mut Cached //CACHE path, REPARSING A FILE THAT CHANGED
{
    let current = stamp(path); //TAKEN BEFORE THE READ, SO A RACE COSTS A REPARSE AND NOT A MISS

    //AN UNREADABLE FILE KEEPS WHAT WE HOLD
    let stale = match cache.get(path)
    {
        Some(cached) => current.is_some() && cached.stamp != current,
        None => true,
    };

    if stale
    {
        let content = fs::read_to_string(path).expect("Failed to read config");
        cache.insert(path.to_string(), Cached { doc: content.parse().expect("Failed to parse config"), stamp: current });
    }

    cache.get_mut(path).expect("Config cache missing")
}

fn with_cached<F: FnOnce(&DocumentMut) -> R, R>(path: &str, f: F) -> R //READ THE CACHED DOCUMENT IN PLACE
{
    let mut cache = CONFIG_CACHE.lock().unwrap();
    f(&load(&mut cache, path).doc)
}

fn with_cached_mut<F: FnOnce(&mut DocumentMut)>(path: &str, f: F)
{
    let mut cache = CONFIG_CACHE.lock().unwrap();
    let cached = load(&mut cache, path);

    f(&mut cached.doc);

    fs::write(path, cached.doc.to_string()).expect("Saving config failed"); //WRITE
    cached.stamp = stamp(path); //OUR OWN WRITE IS NOT A CHANGE
}

fn config_read<T: FromStr>(filename: &str, key: &str) -> T //READ CONFIG
where
    T::Err: Debug,
{
    let path = config_path(filename);

    //READ
    let found = with_cached(&path, |data| data.get(key).map(|value|
    {
        //USE APPROPRIATE DATATYPE
        match value.as_value().expect("Invalid config")
        {
            Value::String(s) => s.value().to_string(),
            Value::Integer(i) => i.value().to_string(),
            Value::Boolean(b) => b.value().to_string(),

            _ => panic!("Unsupported config datatype")
        }
    }));

    if let Some(string_value) = found //FOUND IN CONFIG
    {
        return string_value.parse::<T>().expect("Parsing config value failed");
    }

    //KEY NOT IN CONFIG, INSERT IT
    let mut default_config: DocumentMut = get_config().parse().expect("Failed to parse config");
    with_cached(&path, |data| for (key, old_value) in data.as_table()
    {
        //KEY IS IN BOTH, USE THE OLD VALUE
        if let Some(item) = default_config.get_mut(key)
        {
            //COPY OLD VALUE
            *item.as_value_mut().expect("Updating config failed") = old_value.as_value().expect("Invalid config").clone();
        }
    });

    //UPDATE
    with_cached_mut(&path, |doc| *doc = default_config);

    //REPEAT
    config_read(filename, key)
}

fn set_value(table: &mut Table, key: &str, value: Value) //ASSIGN ONE KEY, KEEPING THE COMMENTS AROUND IT
{
    if let Some(item) = table.get_mut(key)
    {
        //KEEP THE DEFAULT CONFIG'S TRAILING COMMENT
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
