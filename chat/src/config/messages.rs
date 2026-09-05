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
    sync::{ LazyLock, Mutex },
};

use why2::consts as why2_consts;

use crate::
{
    crypto,
    consts::{ self, SharedKeys },
    network::codes::
    {
        MessageColors,
        StoredMessage,
    },
};

//GLOBAL VARIABLES
static HISTORY: LazyLock<Mutex<Vec<StoredMessage>>> = LazyLock::new(|| Mutex::new(load())); //MESSAGE HISTORY
static KEYS: LazyLock<SharedKeys> = LazyLock::new(crypto::history_keys);                    //AT-REST KEYS

//FUNCTIONS
//PRIVATE
fn path() -> String //WHERE THE HISTORY IS KEPT
{
    super::config_path(consts::SERVER_MESSAGES_FILE)
}

fn load() -> Vec<StoredMessage> //READ THE HISTORY OFF DISK
{
    //NO FILE IS AN EMPTY HISTORY
    let Ok(bytes) = fs::read(path()) else { return Vec::new() };

    let Some(plaintext) = crypto::decrypt_packet::
        <{ why2_consts::DEFAULT_GRID_WIDTH }, { why2_consts::DEFAULT_GRID_HEIGHT }>(bytes, &KEYS)
    else { return Vec::new() };

    wincode::config::deserialize::<Vec<StoredMessage>, _>(&plaintext, consts::PACKET_CONFIG).unwrap_or_default()
}

//PUBLIC
pub fn store(username: &str, text: &str, colors: &MessageColors) //APPEND MESSAGE
{
    push(StoredMessage
    {
        username: username.to_string(),
        text: text.to_string(),
        colors: colors.clone(),
        image: None,
    });
}

pub fn store_image(username: &str, filename: &str, hash: &[u8; 32])
{
    push(StoredMessage
    {
        username: username.to_string(),
        text: filename.to_string(),
        colors: MessageColors { username_color: None, message_color: None },
        image: Some(*hash),
    });
}

fn push(message: StoredMessage) //APPEND ONE ENTRY AND REWRITE THE FILE
{
    //A HISTORY OF NOTHING IS NOT A HISTORY - DO NOT TOUCH THE FILE AT ALL
    let limit: usize = super::read_config("max_persistent_messages");
    if limit == 0 { return; }

    let mut history = HISTORY.lock().unwrap();

    history.push(message);

    //THE HISTORY IS A WINDOW OVER THE LAST limit MESSAGES, SO THE OLDEST GO AS THE NEW ONES ARRIVE
    let over = history.len().saturating_sub(limit);
    history.drain(..over);

    //ENCRYPT-THEN-MAC THE WHOLE HISTORY
    let bytes = wincode::config::serialize(&*history, consts::PACKET_CONFIG).expect("Encoding message history failed");
    let sealed = crypto::encrypt_packet::<{ why2_consts::DEFAULT_GRID_WIDTH }, { why2_consts::DEFAULT_GRID_HEIGHT }>(&bytes, &KEYS);

    fs::write(path(), sealed).expect("Saving message history failed");
}

pub fn has_image(hash: &[u8; 32]) -> bool //DOES THE HISTORY NAME THIS PICTURE?
{
    HISTORY.lock().unwrap().iter().any(|message| message.image.as_ref() == Some(hash))
}

pub fn all() -> Vec<StoredMessage> //EVERY STORED LOBBY MESSAGE, OLDEST FIRST
{
    HISTORY.lock().unwrap().clone()
}
