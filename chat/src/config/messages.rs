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
    time::{ SystemTime, UNIX_EPOCH },
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
    id: u64,
    username: String,
    text: String,
    image: Option<[u8; 32]>,
    timestamp: Option<u64>, //UNIX SECONDS
    reply: Option<u64>,     //ID OF THE MESSAGE REPLIED TO
    hearts: Vec<String>,    //USERNAMES THAT HEARTED IT
    edited: bool,           //CHANGED SINCE SENT
}

struct History //THE RECORDS AND THE NEXT ID
{
    next: u64,            //NEXT MESSAGE ID
    records: Vec<Record>,
}

//ONE PAGE OF THE HISTORY
pub struct Page
{
    pub messages: Vec<StoredMessage>,
    pub start: u64, //ID OF THE FIRST ONE
    pub more: bool, //OLDER ONES LEFT
    pub kept: u64,  //MESSAGES KEPT
}

//CONSTS
const MAGIC: &[u8; 8] = b"WHY2MSG\x05"; //FORMAT MARKER

//GLOBAL VARIABLES
static HISTORY: LazyLock<Mutex<History>> = LazyLock::new(|| Mutex::new(History::new())); //MESSAGE HISTORY
static KEYS: LazyLock<SharedKeys> = LazyLock::new(crypto::history_keys);                                       //AT-REST KEYS

//IMPLEMENTATIONS
impl History
{
    fn new() -> Self //LOAD AND CONTINUE THE IDS
    {
        let records = load();
        let next = records.last().map_or(0, |message| message.id + 1);

        Self { next, records }
    }

    fn take_id(&mut self) -> u64 //HAND OUT THE NEXT ID
    {
        let id = self.next;
        self.next += 1;
        id
    }

    fn position(&self, id: u64) -> usize //INDEX OF THE FIRST RECORD AT OR AFTER id
    {
        self.records.partition_point(|message| message.id < id)
    }

    fn find(&self, id: u64) -> Option<usize> //INDEX OF RECORD id, IF KEPT
    {
        let index = self.position(id);
        self.records.get(index).is_some_and(|message| message.id == id).then_some(index)
    }

    fn save(&self) //ENCRYPT-THEN-MAC THE WHOLE HISTORY
    {
        let mut bytes = MAGIC.to_vec();
        bytes.extend(wincode::config::serialize(&self.records, consts::PACKET_CONFIG).expect("Encoding message history failed"));
        let sealed = crypto::encrypt_packet::<{ why2_consts::DEFAULT_GRID_WIDTH }, { why2_consts::DEFAULT_GRID_HEIGHT }>(&bytes, &KEYS);

        fs::write(path(), sealed).expect("Saving message history failed");
    }

    fn orphans(&self, dropped: Vec<[u8; 32]>) -> Vec<[u8; 32]> //DROPPED PICTURES NOTHING NAMES
    {
        dropped.into_iter()
            .filter(|hash| !self.records.iter().any(|message| message.image.as_ref() == Some(hash)))
            .filter(|hash| !super::users::names_avatar(hash))
            .collect()
    }
}

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

    //NO MARKER IS UNREADABLE
    let Some(records) = plaintext.strip_prefix(MAGIC) else { return unreadable() };

    match wincode::config::deserialize::<Vec<Record>, _>(records, consts::PACKET_CONFIG)
    {
        Ok(history) =>
        {
            log::info!("Loaded {} stored messages", history.len());
            history
        },

        Err(_) => unreadable(),
    }
}

fn unreadable() -> Vec<Record> //KEEP A COPY, START EMPTY
{
    let backup = format!("{}.old", path());
    let _ = fs::copy(path(), &backup);

    log::error!("Message history could not be read, it is being ignored (copy kept as {backup})");
    Vec::new()
}

//PUBLIC
pub fn timestamp() -> Option<u64> //NOW, IF TIMESTAMPS ARE ON
{
    super::read_config::<bool>("message_timestamps")
        .then(|| SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_secs()))
}

pub fn next_id() -> u64 //ID FOR A MESSAGE THAT IS NOT KEPT
{
    HISTORY.lock().unwrap().take_id()
}

pub fn store(username: &str, text: &str, timestamp: Option<u64>, reply: Option<u64>) -> u64 //APPEND MESSAGE
{
    push(username, text, None, timestamp, reply)
}

pub fn store_image(username: &str, filename: &str, hash: &[u8; 32], timestamp: Option<u64>) -> u64
{
    push(username, filename, Some(*hash), timestamp, None)
}

fn push(username: &str, text: &str, image: Option<[u8; 32]>, timestamp: Option<u64>, reply: Option<u64>) -> u64 //APPEND ONE ENTRY AND REWRITE THE FILE
{
    let limit: usize = super::read_config("max_persistent_messages");

    let mut guard = HISTORY.lock().unwrap();
    let id = guard.take_id();

    //A HISTORY OF NOTHING DOES NOT TOUCH THE FILE
    if limit == 0 { return id; }

    guard.records.push(Record
    {
        id,
        username: username.to_string(),
        text: text.to_string(),
        image,
        timestamp,
        reply,
        hearts: Vec::new(),
        edited: false,
    });

    //KEEP THE LAST limit MESSAGES
    let over = guard.records.len().saturating_sub(limit);
    let dropped: Vec<[u8; 32]> = guard.records.drain(..over).filter_map(|message| message.image).collect();

    //A PICTURE ANOTHER ENTRY - OR A PROFILE - STILL NAMES STAYS
    let orphans = guard.orphans(dropped);

    guard.save();
    drop(guard); //THE FILES ARE NOT THE HISTORY'S BUSINESS

    remove_images(orphans);

    id
}

fn remove_images(orphans: Vec<[u8; 32]>) //DELETE PICTURES NOTHING NAMES
{
    if !orphans.is_empty() { log::info!("Dropping {} stored images with no history entry left", orphans.len()); }

    for hash in orphans { let _ = fs::remove_file(misc::get_image_dir().join(misc::hex(&hash))); }
}

pub fn exists(id: u64) -> bool //IS MESSAGE id IN THE HISTORY?
{
    HISTORY.lock().unwrap().find(id).is_some()
}

pub fn author(id: u64) -> Option<String> //WHO SAID MESSAGE id
{
    let history = HISTORY.lock().unwrap();

    history.find(id).map(|index| history.records[index].username.clone())
}

pub fn delete(id: u64) -> bool //REMOVE MESSAGE id AND REWRITE THE FILE
{
    let mut guard = HISTORY.lock().unwrap();

    let Some(index) = guard.find(id) else { return false };

    let dropped: Vec<[u8; 32]> = guard.records.remove(index).image.into_iter().collect();
    let orphans = guard.orphans(dropped);

    guard.save();
    drop(guard);

    remove_images(orphans);

    true
}

pub fn heart(id: u64, username: &str) -> Option<Vec<String>> //TOGGLE A HEART, THE NEW SET
{
    let mut guard = HISTORY.lock().unwrap();

    let index = guard.find(id)?;
    let hearts = &mut guard.records[index].hearts;

    match hearts.iter().position(|name| name == username)
    {
        Some(heart) => { hearts.remove(heart); },
        None => hearts.push(username.to_string()),
    }

    let hearts = hearts.clone();
    guard.save();

    Some(hearts)
}

pub fn edit(id: u64, username: &str, text: &str) -> bool //REWORD OWN TEXT MESSAGE id
{
    let mut guard = HISTORY.lock().unwrap();

    let Some(index) = guard.find(id) else { return false };
    let record = &mut guard.records[index];

    if record.username != username || record.image.is_some() { return false; }

    record.text = text.to_string();
    record.edited = true;

    guard.save();

    true
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

        let to = before.map_or(history.records.len(), |id| history.position(id));

        let mut size = 0;
        let mut from = to;

        //WALK BACK UNTIL THE PAGE IS FULL
        while from > 0 && to - from < count
        {
            let record = &history.records[from - 1];

            size += record.username.len() + record.text.len() + record.hearts.iter().map(String::len).sum::<usize>();
            if size > budget && from != to { break; }

            from -= 1;
        }

        let start = history.records.get(from).map_or(history.next, |message| message.id);

        (history.records[from..to].to_vec(), start, from > 0, history.records.len() as u64)
    };

    let mut looked_up: HashMap<String, MessageColors> = HashMap::new();
    let timestamps = super::read_config::<bool>("message_timestamps");

    let messages = records.into_iter().map(|message|
    {
        //WHAT server_users.toml HOLDS FOR THEM NOW
        let stored = looked_up.entry(message.username.clone())
            .or_insert_with(|| super::users::colors(&message.username));

        StoredMessage
        {
            message_id: message.id,
            username: message.username,
            text: message.text,
            colors: match message.image.is_some()
            {
                true => MessageColors { username_color: stored.username_color, message_color: None },
                false => stored.clone(),
            },
            image: message.image,
            timestamp: message.timestamp.filter(|_| timestamps), //HIDDEN WHILE TURNED OFF
            reply: message.reply,
            hearts: message.hearts,
            edited: message.edited,
        }
    }).collect();

    Page { messages, start, more, kept }
}
