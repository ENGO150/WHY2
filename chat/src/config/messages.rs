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
    fs,
    collections::{ HashMap, HashSet },
    sync::{ LazyLock, Mutex },
};

use wincode::{ SchemaWrite, SchemaRead };

use why2::consts as why2_consts;

use crate::
{
    misc,
    crypto,
    consts::{ self, SharedKeys },
    network::codes::
    {
        MessageColors,
        StoredMessage,
    },
};

//STRUCTS
#[derive(SchemaWrite, SchemaRead, Clone)]
struct Record //ONE MESSAGE RECORD
{
    username: String,
    text: String,
    image: Option<[u8; 32]>,
}

struct History //THE RECORDS AND WHERE THEY START
{
    base: u64,            //ABSOLUTE INDEX OF records[0]
    records: Vec<Record>,
}

//ONE PAGE OF THE HISTORY
pub struct Page
{
    pub messages: Vec<StoredMessage>,
    pub start: u64, //ABSOLUTE INDEX OF THE FIRST ONE
    pub more: bool, //OLDER ONES LEFT
    pub kept: u64,  //MESSAGES KEPT
}

//GLOBAL VARIABLES
static HISTORY: LazyLock<Mutex<History>> = LazyLock::new(|| Mutex::new(History { base: 0, records: load() })); //MESSAGE HISTORY
static KEYS: LazyLock<SharedKeys> = LazyLock::new(crypto::history_keys);                                       //AT-REST KEYS

//FUNCTIONS
//PRIVATE
fn path() -> String //WHERE THE HISTORY IS KEPT
{
    super::config_path(consts::SERVER_MESSAGES_FILE)
}

fn load() -> Vec<Record> //READ THE HISTORY OFF DISK
{
    //NO FILE IS AN EMPTY HISTORY
    let Ok(bytes) = fs::read(path()) else
    {
        log::info!("No message history on disk, starting empty");
        return Vec::new();
    };

    //A HISTORY THAT WILL NOT VERIFY IS DROPPED
    let Some(plaintext) = crypto::decrypt_packet::
        <{ why2_consts::DEFAULT_GRID_WIDTH }, { why2_consts::DEFAULT_GRID_HEIGHT }>(bytes, &KEYS)
    else
    {
        log::error!("Message history failed verification, it is being ignored");
        return Vec::new();
    };

    match wincode::config::deserialize::<Vec<Record>, _>(&plaintext, consts::PACKET_CONFIG)
    {
        Ok(history) =>
        {
            log::info!("Loaded {} stored messages", history.len());
            history
        },

        Err(_) => migrate(&plaintext), //MAYBE IT IS ONE THE COLORS ARE STILL IN
    }
}

fn migrate(plaintext: &[u8]) -> Vec<Record>
{
    let Ok(history) = wincode::config::deserialize::<Vec<StoredMessage>, _>(plaintext, consts::PACKET_CONFIG) else
    {
        log::error!("Message history is of an older format, it is being ignored");
        return Vec::new();
    };

    log::info!("Migrated {} stored messages, their colors dropped", history.len());

    history.into_iter().map(|message| Record
    {
        username: message.username,
        text: message.text,
        image: message.image,
    }).collect()
}

//PUBLIC
pub fn store(username: &str, text: &str) //APPEND MESSAGE
{
    push(Record
    {
        username: username.to_string(),
        text: text.to_string(),
        image: None,
    });
}

pub fn store_image(username: &str, filename: &str, hash: &[u8; 32])
{
    push(Record
    {
        username: username.to_string(),
        text: filename.to_string(),
        image: Some(*hash),
    });
}

fn push(message: Record) //APPEND ONE ENTRY AND REWRITE THE FILE
{
    //A HISTORY OF NOTHING DOES NOT TOUCH THE FILE
    let limit: usize = super::read_config("max_persistent_messages");
    if limit == 0 { return; }

    let mut guard = HISTORY.lock().unwrap();

    guard.records.push(message);

    //KEEP THE LAST limit MESSAGES
    let over = guard.records.len().saturating_sub(limit);
    let dropped: Vec<[u8; 32]> = guard.records.drain(..over).filter_map(|message| message.image).collect();

    guard.base += over as u64;
    let history = &guard.records;

    //A PICTURE ANOTHER ENTRY - OR A PROFILE - STILL NAMES STAYS
    let orphans: Vec<[u8; 32]> = dropped.into_iter()
        .filter(|hash| !history.iter().any(|message| message.image.as_ref() == Some(hash)))
        .filter(|hash| !super::users::names_avatar(hash))
        .collect();

    //ENCRYPT-THEN-MAC THE WHOLE HISTORY
    let bytes = wincode::config::serialize(&*history, consts::PACKET_CONFIG).expect("Encoding message history failed");
    let sealed = crypto::encrypt_packet::<{ why2_consts::DEFAULT_GRID_WIDTH }, { why2_consts::DEFAULT_GRID_HEIGHT }>(&bytes, &KEYS);

    fs::write(path(), sealed).expect("Saving message history failed");

    drop(guard); //THE FILES ARE NOT THE HISTORY'S BUSINESS

    if !orphans.is_empty() { log::info!("Dropping {} stored images with no history entry left", orphans.len()); }

    for hash in orphans { let _ = fs::remove_file(misc::get_image_dir().join(misc::hex(&hash))); }
}

pub fn has_image(hash: &[u8; 32]) -> bool //DOES THE HISTORY NAME THIS PICTURE?
{
    HISTORY.lock().unwrap().records.iter().any(|message| message.image.as_ref() == Some(hash))
}

pub fn stored(hash: &[u8; 32]) -> bool //IS THIS PICTURE ONE THE SERVER KEEPS AT ALL?
{
    has_image(hash) || super::users::names_avatar(hash)
}

//DELETE EVERY PICTURE THE HISTORY DOES NOT NAME
pub fn sweep_images()
{
    let Ok(directory) = fs::read_dir(misc::get_image_dir()) else { return }; //NO DIRECTORY, NOTHING TO SWEEP

    let files: Vec<_> = directory.flatten().map(|entry| entry.path()).collect();

    //AN EMPTY DIRECTORY IS NOT WORTH A HISTORY READ
    if files.is_empty() { return; }

    let mut kept: HashSet<String> = HISTORY.lock().unwrap().records.iter()
        .filter_map(|message| message.image.as_ref().map(|hash| misc::hex(hash)))
        .collect();

    //A PROFILE OWNS ITS PICTURE THE WAY AN ENTRY OWNS ITS OWN
    kept.extend(super::users::avatars().iter().map(|hash| misc::hex(hash)));

    let mut swept = 0;

    for file in files
    {
        let named = file.file_name().and_then(|name| name.to_str())
            .map(|name| kept.contains(name)).unwrap_or(false);

        if !named && fs::remove_file(&file).is_ok() { swept += 1; }
    }

    if swept > 0 { log::info!("Swept {swept} stored images nothing names any more"); }
}

//THE NEWEST MESSAGES BEFORE before, OLDEST FIRST
pub fn page(before: Option<u64>, count: usize, budget: usize) -> Page
{
    let (records, start, more, kept) =
    {
        let history = HISTORY.lock().unwrap();

        let end = history.base + history.records.len() as u64;
        let before = before.unwrap_or(end).clamp(history.base, end);

        let mut size = 0;
        let mut start = before;

        //WALK BACK UNTIL THE PAGE IS FULL
        while start > history.base && ((before - start) as usize) < count
        {
            let record = &history.records[(start - 1 - history.base) as usize];

            size += record.username.len() + record.text.len();
            if size > budget && start != before { break; }

            start -= 1;
        }

        let from = (start - history.base) as usize;
        let to = (before - history.base) as usize;

        (history.records[from..to].to_vec(), start, start > history.base, history.records.len() as u64)
    };

    let mut looked_up: HashMap<String, MessageColors> = HashMap::new();

    let messages = records.into_iter().map(|message|
    {
        //WHAT server_users.toml HOLDS FOR THEM NOW
        let stored = looked_up.entry(message.username.clone())
            .or_insert_with(|| super::users::colors(&message.username));

        StoredMessage
        {
            username: message.username,
            text: message.text,
            colors: match message.image.is_some()
            {
                true => MessageColors { username_color: stored.username_color, message_color: None },
                false => stored.clone(),
            },
            image: message.image,
        }
    }).collect();

    Page { messages, start, more, kept }
}
