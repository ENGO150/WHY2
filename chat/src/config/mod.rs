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

#[cfg(feature = "server")]
use toml_edit::RawString;

#[cfg(feature = "server")]
use crate::network::codes::{ ServerSetting, SettingValue };

use crate::{ consts, misc };

#[cfg(feature = "client_base")]
use std::fmt::Write;

#[cfg(feature = "client_base")]
use crate::crypto;

//ENUMS
#[cfg(feature = "client_base")]
pub enum TofuCode //POSSIBLE KEY VERIFICATION RESULTS
{
    Valid, //KEY MATCHES LOCAL CONFIG
    Unknown(String, String), //KEY NOT FOUND IN CONFIG
    Mismatch, //KEY DIFFERS
}

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

//THE HEADING A KEY SITS UNDER - THE LAST COMMENT BLOCK ABOVE IT. THE LICENSE BLOCK AT THE TOP OF THE FILE
//IS NOT ONE: IT IS SEPARATED FROM THE FIRST KEY BY A BLANK LINE, WHICH IS WHAT STARTS THE BLOCK OVER
#[cfg(feature = "server")]
fn heading(prefix: &str) -> Option<String>
{
    let mut heading = None;

    for line in prefix.lines()
    {
        let line = line.trim();

        if line.is_empty() { heading = None; }
        else if let Some(comment) = line.strip_prefix('#') { heading = Some(comment.trim().to_string()); }
    }

    heading
}

#[cfg(feature = "server")]
fn set_user_field(users: &mut Table, username: &str, key: &str, value: Value) //SET ONE FIELD OF username, KEEPING THE REST OF THE ENTRY
{
    //A MISSING OR LEGACY FLAT ENTRY BECOMES AN EMPTY SUBTABLE FIRST
    if users.get(username).and_then(Item::as_table_like).is_none()
    {
        users.insert(username, Item::Table(Table::new()));
    }

    users.get_mut(username).and_then(Item::as_table_like_mut)
        .expect("User entry is not a table").insert(key, Item::Value(value));
}

#[cfg(feature = "server")]
fn write_user_field(username: &str, key: &str, value: Value) //WRITE ONE FIELD OF username TO server_users.toml
{
    with_cached_mut(&config_path(consts::SERVER_USERS_CONFIG), |doc| set_user_field(doc.as_table_mut(), username, key, value));
}

#[cfg(feature = "server")]
pub fn server_users_len() -> usize //COUNT USERS
{
    get_data(&config_path(consts::SERVER_USERS_CONFIG)).len()
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

    let runtime_path =
    {
        #[cfg(feature = "client_base")]
        {
            config_path(consts::SERVER_KEYS_CONFIG)
        }

        #[cfg(feature = "server")]
        {
            config_path(consts::SERVER_USERS_CONFIG)
        }
    };

    //CREATE RUNTIME CONFIG
    if !Path::new(&runtime_path).is_file()
    {
        fs::write(&runtime_path, "#*#**#*###**#***###*#").expect("Writing to config failed");
    }

    //BRING ANY PRE-SUBTABLE USER STORE UP TO DATE
    #[cfg(feature = "server")]
    server_users_migrate();
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

#[cfg(feature = "server")]
pub fn server_users_password(username: &str) -> Option<String> //RETURN PASSWORD HASH OF username
{
    get_data(&config_path(consts::SERVER_USERS_CONFIG)).get(username)?
        .as_table_like()?.get("password")?.as_str().map(str::to_string)
}

#[cfg(feature = "server")]
pub fn server_users_role(username: &str) -> Option<usize> //RETURN PASSWORD HASH OF username
{
    get_data(&config_path(consts::SERVER_USERS_CONFIG)).get(username)?
        .as_table_like()?.get("role")?.as_integer().map(|i| i as usize)
}

#[cfg(feature = "server")]
pub fn server_users_banned(username: &str) -> Option<bool>
{
    get_data(&config_path(consts::SERVER_USERS_CONFIG)).get(username)?
        .as_table_like()?.get("banned")?.as_bool()
}

#[cfg(feature = "server")]
pub fn server_users_ban(username: &str)
{
    write_user_field(username, "banned", true.into());
}

#[cfg(feature = "server")]
pub fn server_users_add(username: &str, hash: &str) -> bool //CREATE NEW USER, RETURN TRUE ON FIRST USER
{
    let first_user = server_users_len() == 0; //SELF-EXPLANATORY, INNIT?

    write_user_field(username, "password", hash.into()); //PASSWORD
    write_user_field(username, "role", (if first_user
        { consts::SERVER_OWNER_ROLE } else { consts::SERVER_USER_ROLE } as i64).into()); //ROLE (OWNER IF THIS IS THE FIRST USER)
    write_user_field(username, "banned", false.into()); //BANNED

    first_user
}

//EVERY KEY OF server.toml AS THE CLIENT EDITS IT. THE FILE ITSELF IS THE LIST - NOTHING HERE NAMES A KEY,
//SO A KEY ADDED TO THE DEFAULT CONFIG SHOWS UP IN THE OVERLAY WITHOUT ANY FURTHER WORK
#[cfg(feature = "server")]
pub fn server_settings() -> Vec<ServerSetting>
{
    let data = get_data(&config_path(consts::SERVER_CONFIG));
    let table = data.as_table();

    let mut settings = Vec::new();
    let mut section = String::new();

    for (key, item) in table.iter()
    {
        //A KEY OF A DATATYPE THE CONFIG READER DOES NOT UNDERSTAND HAS NO ROW TO BE EDITED IN
        let Some(value) = item.as_value() else { continue };

        //THE HEADING CARRIES DOWN THE FILE UNTIL THE NEXT ONE
        if let Some(prefix) = table.key(key).and_then(|key| key.leaf_decor().prefix()).and_then(RawString::as_str)
            && let Some(found) = heading(prefix)
        {
            section = found;
        }

        let description = value.decor().suffix().and_then(RawString::as_str)
            .map(|comment| comment.trim().trim_start_matches('#').trim().to_string()).unwrap_or_default();

        settings.push(ServerSetting
        {
            key: key.to_string(),
            value: match value
            {
                Value::Boolean(on) => SettingValue::Toggle(*on.value()),
                Value::Integer(number) => SettingValue::Number(*number.value()),
                Value::String(text) => SettingValue::Text(text.value().clone()),

                _ => continue,
            },
            section: section.clone(),
            description,

            //SAVING ONE OF THESE STORES IT, AND THE RUNNING SERVER GOES ON USING WHAT IT READ AT STARTUP
            restart: consts::SERVER_RESTART_SETTINGS.contains(&key),
        });
    }

    settings
}

//STORE WHAT THE CLIENT SENT BACK, RETURNING HOW MANY ROWS WERE ACCEPTED. A KEY THE CONFIG DOES NOT ALREADY
//HAVE, OR ONE THAT COMES BACK AS A DIFFERENT DATATYPE, IS DROPPED - THE CLIENT DOES NOT GET TO INVENT KEYS
#[cfg(feature = "server")]
pub fn server_settings_write(settings: &[ServerSetting]) -> usize
{
    let data = get_data(&config_path(consts::SERVER_CONFIG));

    let accepted: Vec<(&str, Value)> = settings.iter().filter_map(|setting|
    {
        let current = data.get(&setting.key).and_then(Item::as_value)?;

        let value: Value = match (&setting.value, current)
        {
            (SettingValue::Toggle(on), Value::Boolean(_)) => (*on).into(),
            (SettingValue::Number(number), Value::Integer(_)) => (*number).into(),
            (SettingValue::Text(text), Value::String(_)) => text.as_str().into(),

            _ => return None,
        };

        Some((setting.key.as_str(), value))
    }).collect();

    //ONE PASS OVER THE DOCUMENT, SO THE FILE IS REWRITTEN ONCE NO MATTER HOW MANY ROWS CHANGED
    with_cached_mut(&config_path(consts::SERVER_CONFIG), |doc|
    {
        for (key, value) in &accepted { set_value(doc.as_table_mut(), key, value.clone()); }
    });

    accepted.len()
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

#[cfg(feature = "server")]
pub fn server_users_migrate() //CONVERT FLAT username = "<hash>" ENTRIES INTO SUBTABLES
{
    let path = config_path(consts::SERVER_USERS_CONFIG);

    //COLLECT LEGACY ENTRIES
    let legacy: Vec<(String, String)> = get_data(&path).as_table().iter()
        .filter_map(|(username, item)| match item
        {
            Item::Value(Value::String(hash)) => Some((username.to_string(), hash.value().to_string())),

            _ => None
        }).collect();

    //NOTHING TO MIGRATE
    if legacy.is_empty() { return; }

    //REWRITE
    with_cached_mut(&path, |doc|
    {
        for (username, hash) in &legacy
        {
            set_user_field(doc.as_table_mut(), username, "password", hash.into());
            set_user_field(doc.as_table_mut(), username, "role", (consts::SERVER_USER_ROLE as i64).into());
            set_user_field(doc.as_table_mut(), username, "banned", false.into());
        }
    });
}

#[cfg(feature = "server")]
pub fn server_users_contains(key: &str) -> bool //CHECK IF server_users.toml contains
{
    get_data(&config_path(consts::SERVER_USERS_CONFIG)).get(key).is_some()
}

#[cfg(feature = "client_base")]
pub fn server_keys_hash(pubkey: &str) -> String //HASH SERVER KEYS
{
    //HASH PUBKEY
    let pubkey_hash = crypto::sha256(pubkey);
    let mut pubkey_string = String::with_capacity(64);

    //SERIALIZE
    for byte in pubkey_hash
    {
        write!(pubkey_string, "{:02x}", byte).unwrap();
    }

    pubkey_string
}

#[cfg(feature = "client_base")]
pub fn server_keys_check(host: &str, pubkey: &str) -> TofuCode //CHECK PUBKEY VALIDITY (TOFU)
{
    let pubkey_string = server_keys_hash(pubkey);

    //PEER PUBKEY STORED, CHECK VALIDITY
    if get_data(&config_path(consts::SERVER_KEYS_CONFIG)).get(host).is_some()
    {
        //COMPARE
        return if config_read::<String>(consts::SERVER_KEYS_CONFIG, host) == pubkey_string
        {
            TofuCode::Valid
        } else
        {
            TofuCode::Mismatch
        }
    }

    TofuCode::Unknown(pubkey_string, host.to_string())
}

#[cfg(feature = "client_base")]
pub fn server_keys_pinned(host: &str) -> Option<String> //THE FINGERPRINT CURRENTLY PINNED FOR host, IF ANY
{
    if get_data(&config_path(consts::SERVER_KEYS_CONFIG)).get(host).is_none() { return None; }

    Some(config_read::<String>(consts::SERVER_KEYS_CONFIG, host))
}

#[cfg(feature = "client_base")]
pub fn server_keys_save(host: &str, pubkey_hash: &str) //SAVE KEY
{
    //WRITE
    config_write(consts::SERVER_KEYS_CONFIG, host, pubkey_hash);
}
