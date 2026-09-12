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

//GLOBAL VARIABLES
static HISTORY: LazyLock<Mutex<Vec<Record>>> = LazyLock::new(|| Mutex::new(load())); //MESSAGE HISTORY
static KEYS: LazyLock<SharedKeys> = LazyLock::new(crypto::history_keys);             //AT-REST KEYS

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

    //A HISTORY THAT WILL NOT VERIFY IS DROPPED RATHER THAN REFUSED, WHICH IS WORTH SAYING OUT LOUD: IT IS
    //ALSO WHAT A FILE WRITTEN UNDER ANOTHER SERVER'S KEY, OR A TAMPERED ONE, LOOKS LIKE FROM HERE
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
    //A HISTORY OF NOTHING IS NOT A HISTORY - DO NOT TOUCH THE FILE AT ALL
    let limit: usize = super::read_config("max_persistent_messages");
    if limit == 0 { return; }

    let mut history = HISTORY.lock().unwrap();

    history.push(message);

    //THE HISTORY IS A WINDOW OVER THE LAST limit MESSAGES, SO THE OLDEST GO AS THE NEW ONES ARRIVE
    let over = history.len().saturating_sub(limit);
    let dropped: Vec<[u8; 32]> = history.drain(..over).filter_map(|message| message.image).collect();

    //AND A PICTURE IS KEPT BY THE HISTORY AND BY NOTHING ELSE, SO AN ENTRY LEAVING THE WINDOW IS THE END
    //OF IT. THE SAME PICTURE POSTED TWICE IS ONE FILE (IT IS NAMED AFTER ITS CONTENT), SO WHAT IS LEFT
    //HAS TO BE ASKED FIRST - THE OLDER LINE GOING DOES NOT TAKE THE NEWER ONE'S PICTURE WITH IT
    let orphans: Vec<[u8; 32]> = dropped.into_iter()
        .filter(|hash| !history.iter().any(|message| message.image.as_ref() == Some(hash)))
        .collect();

    //ENCRYPT-THEN-MAC THE WHOLE HISTORY
    let bytes = wincode::config::serialize(&*history, consts::PACKET_CONFIG).expect("Encoding message history failed");
    let sealed = crypto::encrypt_packet::<{ why2_consts::DEFAULT_GRID_WIDTH }, { why2_consts::DEFAULT_GRID_HEIGHT }>(&bytes, &KEYS);

    fs::write(path(), sealed).expect("Saving message history failed");

    drop(history); //THE FILES ARE NOT THE HISTORY'S BUSINESS - THE LOCK IS DONE WITH

    if !orphans.is_empty() { log::info!("Dropping {} stored images with no history entry left", orphans.len()); }

    for hash in orphans { let _ = fs::remove_file(misc::get_image_dir().join(misc::hex(&hash))); }
}

pub fn has_image(hash: &[u8; 32]) -> bool //DOES THE HISTORY NAME THIS PICTURE?
{
    HISTORY.lock().unwrap().iter().any(|message| message.image.as_ref() == Some(hash))
}

//EVERY PICTURE THE HISTORY DOES NOT NAME IS SCRAP: NOTHING ELSE EVER POINTS AT server_images/, SO A FILE THAT
//OUTLIVED ITS ENTRY CAN ONLY SIT THERE - AN UPLOAD ABANDONED HALFWAY, A CRASH BETWEEN THE RENAME AND THE
//ENTRY, A HISTORY DISCARDED WHOLE BY DROPPING ITS KEY. THIS IS A STARTUP JOB AND HAS TO STAY ONE: AN
//UPLOAD IN FLIGHT IS BUILT IN THAT SAME DIRECTORY UNDER ITS UID, AND A SWEEP WOULD TAKE IT
pub fn sweep_images()
{
    let Ok(directory) = fs::read_dir(misc::get_image_dir()) else { return }; //NO DIRECTORY, NOTHING TO SWEEP

    let files: Vec<_> = directory.flatten().map(|entry| entry.path()).collect();

    //AN EMPTY DIRECTORY IS NOT WORTH TOUCHING THE HISTORY OVER - THE FIRST READ OF IT MINTS ITS AT-REST KEY
    if files.is_empty() { return; }

    let kept: HashSet<String> = HISTORY.lock().unwrap().iter()
        .filter_map(|message| message.image.as_ref().map(|hash| misc::hex(hash)))
        .collect();

    let mut swept = 0;

    for file in files
    {
        let named = file.file_name().and_then(|name| name.to_str())
            .map(|name| kept.contains(name)).unwrap_or(false);

        if !named && fs::remove_file(&file).is_ok() { swept += 1; }
    }

    if swept > 0 { log::info!("Swept {swept} stored images nothing names any more"); }
}

//EVERY STORED LOBBY MESSAGE, OLDEST FIRST
pub fn all() -> Vec<StoredMessage>
{
    let history = HISTORY.lock().unwrap().clone();
    let mut looked_up: HashMap<String, MessageColors> = HashMap::new();

    history.into_iter().map(|message|
    {
        //WHAT server_users.toml HOLDS FOR THEM RIGHT NOW - THE RECORD ITSELF CARRIES NO COLOR TO PREFER
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
    }).collect()
}
