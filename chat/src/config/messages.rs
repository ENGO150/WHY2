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

//! # Lobby Message History
//!
//! The persistent history of the lobby, kept as the last `max_persistent_messages` messages
//! when `persistent_messages` is on. Channels have no history: a channel only exists while
//! somebody is in it.
//!
//! # Storage
//! `server_messages.bin` is the one file under the configuration directory that is not TOML:
//! it is a format marker followed by a `wincode`-encoded `Vec<Record>`, sealed as a whole with
//! [`crypto::encrypt_packet`] under [`crypto::history_keys`]. Every write uses a fresh nonce,
//! since the file is replaced rather than appended to.
//!
//! The in-memory history is the working set; the file is only read once, on first touch. A
//! missing, tampered or unrecognisable file loads as an empty history (an unparsable one is
//! first copied to `server_messages.bin.old`), so a bad file never stops the server from
//! starting.
//!
//! # Records and Colors
//! A record stores who said what, not how it looked: colors are looked up per sender when
//! a page is built, so a replay is painted the way its sender looks **now**.
//!
//! # Message Ids
//! Every message gets an id from one counter shared by the lobby and every channel, taken under
//! the history lock so the stored ids rise in record order. That ordering is what lets
//! [`page`] binary-search by id, and why a page cursor stays valid while messages are added,
//! trimmed or deleted around it.

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
        VoiceNote,
    },
};

//STRUCTS
/// One stored message, as kept on disk.
#[derive(SchemaWrite, SchemaRead, Clone)]
struct Record //ONE MESSAGE RECORD
{
    /// Server-assigned message id.
    id: u64,
    /// Author's username.
    username: String,
    /// Message text, or the filename of a picture.
    text: String,
    /// Hash of the stored picture, if this is one.
    image: Option<[u8; 32]>,
    timestamp: Option<u64>,   //UNIX SECONDS
    reply: Option<u64>,       //ID OF THE MESSAGE REPLIED TO
    hearts: Vec<String>,      //USERNAMES THAT HEARTED IT
    edited: bool,             //CHANGED SINCE SENT
    voice: Option<VoiceNote>, //THE CLIP OF A VOICE MESSAGE
    pinned: bool,             //PINNED BY A MODERATOR
}

/// A [`Record`] in the format before pins, kept to migrate older files.
#[derive(SchemaRead)]
struct VoiceRecord //A RECORD BEFORE PINS (remove with next version bump)
{
    id: u64,
    username: String,
    text: String,
    image: Option<[u8; 32]>,
    timestamp: Option<u64>,
    reply: Option<u64>,
    hearts: Vec<String>,
    edited: bool,
    voice: Option<VoiceNote>,
}

/// The stored records and the next message id.
struct History //THE RECORDS AND THE NEXT ID
{
    next: u64,            //NEXT MESSAGE ID
    records: Vec<Record>,
}

//ONE PAGE OF THE HISTORY
/// One page of the history, oldest message first.
pub struct Page
{
    /// The messages, with their senders' current colors.
    pub messages: Vec<StoredMessage>,
    /// Id of the first message on the page; the cursor for the next older page.
    pub start: u64, //ID OF THE FIRST ONE
    /// Whether older messages remain.
    pub more: bool, //OLDER ONES LEFT
    /// Total number of messages kept.
    pub kept: u64,  //MESSAGES KEPT
}

//CONSTS
/// Marker at the start of the current file format.
const MAGIC: &[u8; 8] = b"WHY2MSG\x07"; //FORMAT MARKER
/// Marker of the format before pins.
const MAGIC_V6: &[u8; 8] = b"WHY2MSG\x06"; //MARKER BEFORE PINS (remove with next version bump)

//GLOBAL VARIABLES
/// The in-memory history, loaded from disk on first touch.
static HISTORY: LazyLock<Mutex<History>> = LazyLock::new(|| Mutex::new(History::new())); //MESSAGE HISTORY
/// At-rest keys of the history file.
static KEYS: LazyLock<SharedKeys> = LazyLock::new(crypto::history_keys);                                       //AT-REST KEYS

//IMPLEMENTATIONS
impl From<VoiceRecord> for Record //NOT PINNED (remove with next version bump)
{
    fn from(message: VoiceRecord) -> Self
    {
        Self
        {
            id: message.id,
            username: message.username,
            text: message.text,
            image: message.image,
            timestamp: message.timestamp,
            reply: message.reply,
            hearts: message.hearts,
            edited: message.edited,
            voice: message.voice,
            pinned: false,
        }
    }
}

impl Record
{
    /// Returns the hash of the file in `server_images/` this record names, picture or voice clip.
    fn stored(&self) -> Option<[u8; 32]> //THE FILE IN server_images/ IT NAMES
    {
        self.image.or(self.voice.as_ref().map(|voice| voice.hash))
    }
}

impl History
{
    /// Loads the history from disk and continues its id sequence.
    fn new() -> Self //LOAD AND CONTINUE THE IDS
    {
        let records = load();
        let next = records.last().map_or(0, |message| message.id + 1);

        Self { next, records }
    }

    /// Hands out the next message id.
    fn take_id(&mut self) -> u64 //HAND OUT THE NEXT ID
    {
        let id = self.next;
        self.next += 1;
        id
    }

    /// Returns the index of the first record whose id is at or after `id`.
    fn position(&self, id: u64) -> usize //INDEX OF THE FIRST RECORD AT OR AFTER id
    {
        self.records.partition_point(|message| message.id < id)
    }

    /// Returns the index of record `id`, if kept.
    fn find(&self, id: u64) -> Option<usize> //INDEX OF RECORD id, IF KEPT
    {
        let index = self.position(id);
        self.records.get(index).is_some_and(|message| message.id == id).then_some(index)
    }

    /// Seals the whole history and writes it to disk.
    ///
    /// # Panics
    /// If encoding or writing fails.
    fn save(&self) //ENCRYPT-THEN-MAC THE WHOLE HISTORY
    {
        let mut bytes = MAGIC.to_vec();
        bytes.extend(wincode::config::serialize(&self.records, consts::PACKET_CONFIG).expect("Encoding message history failed"));
        let sealed = crypto::encrypt_packet::<{ why2_consts::DEFAULT_GRID_WIDTH }, { why2_consts::DEFAULT_GRID_HEIGHT }>(&bytes, &KEYS);

        fs::write(path(), sealed).expect("Saving message history failed");
    }

    /// Filters dropped files down to those nothing else names — no record, no avatar and not the
    /// server icon.
    fn orphans(&self, dropped: Vec<[u8; 32]>) -> Vec<[u8; 32]> //DROPPED PICTURES NOTHING NAMES
    {
        dropped.into_iter()
            .filter(|hash| !self.records.iter().any(|message| message.stored().as_ref() == Some(hash)))
            .filter(|hash| !super::users::names_avatar(hash))
            .filter(|hash| super::server_icon().as_ref() != Some(hash))
            .collect()
    }
}

//FUNCTIONS
//PRIVATE
/// Returns the path of the history file.
fn path() -> String //WHERE THE HISTORY IS KEPT
{
    super::config_path(consts::SERVER_MESSAGES_FILE)
}

/// Reads and verifies the history file, migrating an older format.
///
/// # Returns
/// The stored records, or an empty history if the file is missing or unreadable.
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

    //AN OLDER MARKER IS AN OLDER FORMAT
    if let Some(records) = plaintext.strip_prefix(MAGIC_V6)
    {
        return migrate(wincode::config::deserialize::<Vec<VoiceRecord>, _>(records, consts::PACKET_CONFIG)
            .map(|history| history.into_iter().map(Record::from).collect()));
    }

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

/// Keeps a copy of an unreadable history file and starts empty.
fn unreadable() -> Vec<Record> //KEEP A COPY, START EMPTY
{
    let backup = format!("{}.old", path());
    let _ = fs::copy(path(), &backup);

    log::error!("Message history could not be read, it is being ignored (copy kept as {backup})");
    Vec::new()
}

/// Accepts records decoded from an older format.
fn migrate<E>(history: Result<Vec<Record>, E>) -> Vec<Record> //LOAD AN OLDER FORMAT (remove with next version bump)
{
    let Ok(history) = history else { return unreadable() };

    log::info!("Migrated {} stored messages from an older format", history.len());
    history
}

//PUBLIC
/// Returns the current Unix time if `message_timestamps` is on, else `None`.
pub fn timestamp() -> Option<u64> //NOW, IF TIMESTAMPS ARE ON
{
    super::read_config::<bool>("message_timestamps")
        .then(|| SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_secs()))
}

/// Takes a message id for a message that is not stored (a channel message, or with the history
/// off).
pub fn next_id() -> u64 //ID FOR A MESSAGE THAT IS NOT KEPT
{
    HISTORY.lock().unwrap().take_id()
}

/// Stores a text message.
///
/// # Parameters
/// - `username`: The author.
/// - `text`: The message text.
/// - `timestamp`: When it was sent, if timestamps are on.
/// - `reply`: The id of the message it replies to.
///
/// # Returns
/// The message id, which is assigned even if nothing is kept.
pub fn store(username: &str, text: &str, timestamp: Option<u64>, reply: Option<u64>) -> u64 //APPEND MESSAGE
{
    push(username, text, None, None, timestamp, reply)
}

/// Stores a picture message; returns its id.
pub fn store_image(username: &str, filename: &str, hash: &[u8; 32], timestamp: Option<u64>) -> u64
{
    push(username, filename, Some(*hash), None, timestamp, None)
}

/// Stores a voice message; returns its id.
pub fn store_voice(username: &str, voice: &VoiceNote, timestamp: Option<u64>) -> u64
{
    push(username, "", None, Some(voice.clone()), timestamp, None)
}

//APPEND ONE ENTRY AND REWRITE THE FILE
/// Appends one record, trims the history to its limit and rewrites the file.
///
/// Files named only by trimmed records are deleted once the lock is released.
///
/// # Returns
/// The message id.
fn push(username: &str, text: &str, image: Option<[u8; 32]>, voice: Option<VoiceNote>, timestamp: Option<u64>, reply: Option<u64>) -> u64
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
        voice,
        pinned: false,
    });

    //KEEP THE LAST limit MESSAGES
    let over = guard.records.len().saturating_sub(limit);
    let dropped: Vec<[u8; 32]> = guard.records.drain(..over).filter_map(|message| message.stored()).collect();

    //A PICTURE ANOTHER ENTRY - OR A PROFILE - STILL NAMES STAYS
    let orphans = guard.orphans(dropped);

    guard.save();
    drop(guard); //THE FILES ARE NOT THE HISTORY'S BUSINESS

    remove_images(orphans);

    id
}

/// Deletes stored files by hash.
fn remove_images(orphans: Vec<[u8; 32]>) //DELETE PICTURES NOTHING NAMES
{
    if !orphans.is_empty() { log::info!("Dropping {} stored files with no history entry left", orphans.len()); }

    for hash in orphans { let _ = fs::remove_file(misc::get_image_dir().join(misc::hex(&hash))); }
}

/// Returns whether message `id` is in the history.
pub fn exists(id: u64) -> bool //IS MESSAGE id IN THE HISTORY?
{
    HISTORY.lock().unwrap().find(id).is_some()
}

/// Returns the content hash and filename of a stored attachment.
pub fn attachment(id: u64) -> Option<([u8; 32], String)>
{
    let history = HISTORY.lock().unwrap();
    let record = &history.records[history.find(id)?];
    Some((record.image?, record.text.clone()))
}

/// Returns the author of message `id`, if kept.
pub fn author(id: u64) -> Option<String> //WHO SAID MESSAGE id
{
    let history = HISTORY.lock().unwrap();

    history.find(id).map(|index| history.records[index].username.clone())
}

/// Removes message `id`, deleting its file if nothing else names it.
///
/// Authorization is the caller's job.
///
/// # Returns
/// `true` if the message existed.
pub fn delete(id: u64) -> bool //REMOVE MESSAGE id AND REWRITE THE FILE
{
    let mut guard = HISTORY.lock().unwrap();

    let Some(index) = guard.find(id) else { return false };

    let dropped: Vec<[u8; 32]> = guard.records.remove(index).stored().into_iter().collect();
    let orphans = guard.orphans(dropped);

    guard.save();
    drop(guard);

    remove_images(orphans);

    true
}

/// Toggles `username`'s heart on message `id`.
///
/// # Returns
/// The new set of usernames that hearted it, or `None` if the message is not kept.
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

/// Rewords message `id`, if it is a text message written by `username`.
///
/// # Returns
/// `true` if the message was edited.
pub fn edit(id: u64, username: &str, text: &str) -> bool //REWORD OWN TEXT MESSAGE id
{
    let mut guard = HISTORY.lock().unwrap();

    let Some(index) = guard.find(id) else { return false };
    let record = &mut guard.records[index];

    if record.username != username || record.stored().is_some() { return false; }

    record.text = text.to_string();
    record.edited = true;

    guard.save();

    true
}

/// Toggles the pin on message `id`.
///
/// # Returns
/// - `Some(pinned)` with the new state.
/// - `None` if the message is not kept, or pinning it would exceed `max_pinned_messages`.
pub fn pin(id: u64) -> Option<bool> //TOGGLE A PIN, THE NEW STATE
{
    let limit: usize = super::read_config("max_pinned_messages");

    let mut guard = HISTORY.lock().unwrap();

    let index = guard.find(id)?;

    //NO PIN PAST THE LIMIT
    if !guard.records[index].pinned && guard.records.iter().filter(|message| message.pinned).count() >= limit { return None; }

    let record = &mut guard.records[index];
    record.pinned = !record.pinned;

    let pinned = record.pinned;
    guard.save();

    Some(pinned)
}

/// Returns every pinned message, oldest first.
pub fn pinned() -> Vec<StoredMessage> //EVERY PINNED MESSAGE, OLDEST FIRST
{
    let records: Vec<Record> = HISTORY.lock().unwrap().records.iter().filter(|message| message.pinned).cloned().collect();

    wire(records)
}

/// Returns whether any record names this picture.
pub fn has_image(hash: &[u8; 32]) -> bool //DOES THE HISTORY NAME THIS PICTURE?
{
    HISTORY.lock().unwrap().records.iter().any(|message| message.image.as_ref() == Some(hash))
}

/// Returns the stored voice message whose clip has this hash.
pub fn voice(hash: &[u8; 32]) -> Option<VoiceNote> //THE STORED VOICE MESSAGE WITH THIS CLIP
{
    HISTORY.lock().unwrap().records.iter().find_map(|message| message.voice.as_ref().filter(|voice| voice.hash == *hash)).cloned()
}

/// Returns whether the server keeps this file at all: as a history picture, an avatar or the
/// server icon.
pub fn stored(hash: &[u8; 32]) -> bool //IS THIS PICTURE ONE THE SERVER KEEPS AT ALL?
{
    has_image(hash) || super::users::names_avatar(hash) || super::server_icon().as_ref() == Some(hash)
}

//DELETE EVERY PICTURE THE HISTORY DOES NOT NAME
/// Deletes every file in `server_images/` that nothing names any more.
pub fn sweep_images()
{
    let Ok(directory) = fs::read_dir(misc::get_image_dir()) else { return }; //NO DIRECTORY, NOTHING TO SWEEP

    let files: Vec<_> = directory.flatten().map(|entry| entry.path()).collect();

    //AN EMPTY DIRECTORY IS NOT WORTH A HISTORY READ
    if files.is_empty() { return; }

    let mut kept: HashSet<String> = HISTORY.lock().unwrap().records.iter()
        .filter_map(|message| message.stored().map(|hash| misc::hex(&hash)))
        .collect();

    //A PROFILE OWNS ITS PICTURE THE WAY AN ENTRY OWNS ITS OWN
    kept.extend(super::users::avatars().iter().map(|hash| misc::hex(hash)));
    kept.extend(super::server_icon().map(|hash| misc::hex(&hash)));

    let mut swept = 0;

    for file in files
    {
        let named = file.file_name().and_then(|name| name.to_str())
            .map(|name| kept.contains(name)).unwrap_or(false);

        if !named && fs::remove_file(&file).is_ok() { swept += 1; }
    }

    if swept > 0 { log::info!("Swept {swept} stored files nothing names any more"); }
}

//THE NEWEST MESSAGES BEFORE before, OLDEST FIRST
/// Returns one page of the history, oldest first.
///
/// # Parameters
/// - `before`: Return messages older than this id; `None` for the newest.
/// - `count`: The most messages on the page.
/// - `budget`: The approximate byte budget of the page; at least one message is always
///   returned so the cursor moves.
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

    Page { messages: wire(records), start, more, kept }
}

//RECORDS AS THE WIRE CARRIES THEM
/// Converts records into wire messages, attaching each sender's current colors.
///
/// A picture or voice line keeps only the username color, and stored timestamps are hidden
/// while `message_timestamps` is off.
fn wire(records: Vec<Record>) -> Vec<StoredMessage>
{
    let mut looked_up: HashMap<String, MessageColors> = HashMap::new();
    let timestamps = super::read_config::<bool>("message_timestamps");

    records.into_iter().map(|message|
    {
        //WHAT server_users.toml HOLDS FOR THEM NOW
        let stored = looked_up.entry(message.username.clone())
            .or_insert_with(|| super::users::colors(&message.username));

        let media = message.stored().is_some();

        StoredMessage
        {
            message_id: message.id,
            username: message.username,
            text: message.text,
            colors: match media
            {
                true => MessageColors { username_color: stored.username_color, message_color: None },
                false => stored.clone(),
            },
            image: message.image,
            timestamp: message.timestamp.filter(|_| timestamps), //HIDDEN WHILE TURNED OFF
            reply: message.reply,
            hearts: message.hearts,
            edited: message.edited,
            voice: message.voice,
            pinned: message.pinned,
        }
    }).collect()
}
