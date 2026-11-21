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

//! # Private Message History
//!
//! The stored private messages, kept as the last `max_persistent_private_messages` messages
//! when `persistent_private_messages` is on. A record belongs to the two accounts that took
//! part in it; each side is replayed its own view on login and no one else ever sees it.
//!
//! # Storage
//! `server_private_messages.bin` is a format marker followed by `wincode`-encoded
//! records and per-account id counters, sealed as a whole with [`crypto::encrypt_packet`] under
//! [`crypto::history_keys`]. Every write uses a fresh nonce, since the file is replaced rather
//! than appended to.
//!
//! The in-memory history is the working set; the file is only read once, on first touch. A
//! missing, tampered or unrecognisable file loads as an empty history (an unparsable one is
//! first copied to `server_private_messages.bin.old`), so a bad file never stops the server
//! from starting.
//!
//! # Pages
//! A page is a window of the messages one account takes part in, oldest first, and its cursor
//! is an account-local message id. Each account's ids increase in record order, so a page
//! cursor stays valid while messages are added and trimmed around it.
//!
//! Each record also keeps the next lobby id at send time. Private pages stay separate on the
//! wire, but the client uses that cursor to merge them into the lobby timeline, even when
//! timestamps are off.
//!
//! # Actions
//! Private ids use the high bit, displayed as `pN` by the client, so they cannot address a
//! public message by mistake. Only the author may delete or edit a message; either participant
//! may react or reply. Each account has its own counter, saved even when its newest message is
//! deleted. Internal record ids never leave the server; pages, replies and updates expose only
//! the receiving account's ids.

use std::
{
    fs,
    collections::{ HashMap, BTreeMap, HashSet },
    path::{ Path, PathBuf },
    sync::{ LazyLock, Mutex },
};

use wincode::{ SchemaWrite, SchemaRead };

use why2::consts as why2_consts;

use crate::
{
    crypto,
    consts::{ self, SharedKeys },
    network::codes::
    {
        MessageColors,
        MessageMedia,
        StoredPrivateMessage,
    },
};

//STRUCTS
/// The two participants and the message id each account uses.
#[derive(SchemaWrite, SchemaRead, Clone)]
pub struct Participants //THE SAME MESSAGE HAS AN ID IN EACH ACCOUNT
{
    sender: String,
    recipient: String,
    sender_id: u64,
    recipient_id: u64,
}

/// The ids and reply references for a newly sent private message.
pub struct Delivery
{
    /// The two account-local ids.
    pub participants: Participants,
    /// Whether the message was retained in history.
    pub persistent: bool,
    /// The sender's id of the message being answered.
    pub sender_reply: Option<u64>,
    /// The recipient's id of the message being answered.
    pub recipient_reply: Option<u64>,
}

/// One stored private message, as kept on disk.
#[derive(SchemaWrite, SchemaRead, Clone)]
struct Record //ONE PRIVATE MESSAGE RECORD
{
    id: u64,                   //INTERNAL ID, NEVER SENT TO A CLIENT
    participants: Participants,
    text: String,
    timestamp: Option<u64>,    //UNIX SECONDS
    before: u64,               //THE NEXT LOBBY ID WHEN THIS WAS SENT
    reply: Option<(u64, u64)>, //THE REPLIED-TO IDS FOR SENDER AND RECIPIENT
    hearts: Vec<String>,
    edited: bool,
    media: Option<MessageMedia>,
}

/// The stored records and the next message id.
#[derive(Default, SchemaWrite, SchemaRead)]
struct History //THE RECORDS AND THE NEXT ID
{
    next: u64,                    //NEXT INTERNAL MESSAGE ID
    users: BTreeMap<String, u64>, //NEXT PRIVATE ID IN EACH ACCOUNT
    records: Vec<Record>,
    read_through: BTreeMap<String, BTreeMap<String, u64>>,
}

//ONE PAGE OF THE PRIVATE HISTORY
/// One page of an account's private history, oldest message first.
pub struct Page
{
    /// The messages, from the account's point of view.
    pub messages: Vec<StoredPrivateMessage>,
    /// Id of the first message on the page; the cursor for the next older page.
    pub start: u64, //ID OF THE FIRST ONE
    /// Whether older messages remain.
    pub more: bool, //OLDER ONES LEFT
    /// Total number of the account's messages kept.
    pub kept: u64,  //MESSAGES KEPT
}

//CONSTS
/// Marker at the start of the current file format.
const MAGIC: &[u8; 8] = b"WHY2PM06"; //FORMAT MARKER

//GLOBAL VARIABLES
/// The in-memory history, loaded from disk on first touch.
static HISTORY: LazyLock<Mutex<History>> = LazyLock::new(|| Mutex::new(History::new())); //PRIVATE MESSAGE HISTORY
/// At-rest keys of the private history file.
static KEYS: LazyLock<SharedKeys> = LazyLock::new(crypto::history_keys);                 //AT-REST KEYS

//IMPLEMENTATIONS
impl Participants
{
    /// Returns the id belonging to `username`, or `None` for anyone else.
    pub fn message_id(&self, username: &str) -> Option<u64> //ONLY EXPOSE THE ACCOUNT'S OWN ID
    {
        if self.sender == username { Some(self.sender_id) }
        else if self.recipient == username { Some(self.recipient_id) }
        else { None }
    }

    /// Returns the other participant, if this account takes part.
    fn peer(&self, username: &str) -> Option<&str>
    {
        if self.sender == username { Some(&self.recipient) }
        else if self.recipient == username { Some(&self.sender) }
        else { None }
    }
}

impl Record
{
    /// Returns whether `username` is one of the two sides.
    fn involves(&self, username: &str) -> bool //IS username A SIDE OF THIS MESSAGE
    {
        self.participants.message_id(username).is_some()
    }
}

impl History
{
    /// Loads the history from disk and continues its id sequence.
    fn new() -> Self //LOAD AND CONTINUE THE IDS
    {
        let history = load();
        super::messages::resume_cursor(history.records.iter().map(|message| message.before).max().unwrap_or(0));
        history
    }

    /// Hands out an id from this account's own sequence.
    fn take_user_id(&mut self, username: &str) -> u64 //UNRELATED CONVERSATIONS DO NOT ADVANCE THIS ACCOUNT
    {
        let next = self.users.entry(username.to_string()).or_insert(consts::PRIVATE_MESSAGE_ID);
        let id = *next;
        *next += 1;
        id
    }

    /// Resolves an id only in the requesting account's history.
    fn find(&self, id: u64, username: &str) -> Option<usize> //NEVER INTERPRET ANOTHER ACCOUNT'S ID
    {
        self.records.iter().position(|message| message.participants.message_id(username) == Some(id))
    }

    fn unread(&self, username: &str) -> (u64, Vec<(String, u64, u64)>)
    {
        let next = self.users.get(username).copied().unwrap_or(consts::PRIVATE_MESSAGE_ID);
        let read = self.read_through.get(username);
        let mut counts = BTreeMap::<&str, (u64, u64)>::new();

        for record in &self.records
        {
            let Participants { sender, recipient, recipient_id, .. } = &record.participants;
            if recipient != username || sender == username ||
                read.and_then(|peers| peers.get(sender)).is_some_and(|id| recipient_id <= id) { continue; }

            let (count, newest) = counts.entry(sender).or_default();
            *count += 1;
            *newest = (*newest).max(*recipient_id);
        }

        (next, counts.into_iter().map(|(sender, (count, newest))| (sender.to_owned(), count, newest)).collect())
    }

    fn mark_read(&mut self, username: &str, peer: &str, through: u64) -> bool
    {
        let previous = self.read_through.get(username).and_then(|peers| peers.get(peer)).copied().unwrap_or(0);
        if through <= previous { return false; }

        let newest = self.records.iter().rev().find_map(|record|
        {
            let Participants { sender, recipient, recipient_id, .. } = &record.participants;
            (recipient == username && sender == peer && sender != username && *recipient_id <= through)
                .then_some(*recipient_id)
        });

        let Some(newest) = newest.filter(|id| *id > previous) else { return false; };
        self.read_through.entry(username.to_owned()).or_default().insert(peer.to_owned(), newest);
        true
    }

    /// Seals the whole history and writes it to disk.
    ///
    /// # Panics
    /// If encoding or writing fails.
    fn save(&self) //ENCRYPT-THEN-MAC THE WHOLE HISTORY
    {
        let mut bytes = MAGIC.to_vec();
        bytes.extend(wincode::config::serialize(self, consts::PACKET_CONFIG.disable_preallocation_size_limit()).expect("Encoding private message history failed"));
        let sealed = crypto::encrypt_packet::<{ why2_consts::DEFAULT_GRID_WIDTH }, { why2_consts::DEFAULT_GRID_HEIGHT }>(&bytes, &KEYS);

        //REPLACE ONLY AFTER THE WHOLE NEW FILE HAS BEEN WRITTEN
        let pending = format!("{}.tmp", path());
        fs::write(&pending, sealed).expect("Saving private message history failed");
        fs::rename(pending, path()).expect("Replacing private message history failed");
    }
}

//FUNCTIONS
//PRIVATE
/// Returns the path of the private history file.
fn path() -> String //WHERE THE PRIVATE HISTORY IS KEPT
{
    super::config_path(consts::SERVER_PRIVATE_MESSAGES_FILE)
}

/// Reads and verifies the private history file.
///
/// # Returns
/// The stored records, or an empty history if the file is missing or unreadable.
fn load() -> History //READ THE PRIVATE HISTORY OFF DISK
{
    //NO FILE IS AN EMPTY HISTORY
    let Ok(bytes) = fs::read(path()) else
    {
        log::info!("No private message history on disk, starting empty");
        return History::default();
    };

    //A HISTORY THAT WILL NOT VERIFY IS DROPPED
    let Some(plaintext) = crypto::decrypt_packet::
        <{ why2_consts::DEFAULT_GRID_WIDTH }, { why2_consts::DEFAULT_GRID_HEIGHT }>(bytes, &KEYS)
    else
    {
        log::error!("Private message history failed verification, it is being ignored");
        return History::default();
    };

    //THE VERIFIED FILE MAY HOLD MORE RECORDS THAN A NETWORK PACKET CAN PREALLOCATE
    let config = consts::PACKET_CONFIG.disable_preallocation_size_limit();

    //NO MARKER IS UNREADABLE
    let Some(records) = plaintext.strip_prefix(MAGIC) else { return unreadable() };

    match wincode::config::deserialize::<History, _>(records, config)
    {
        Ok(history) =>
        {
            log::info!("Loaded {} stored private messages", history.records.len());
            history
        },

        Err(_) => unreadable(),
    }
}

/// Keeps a copy of an unreadable private history file and starts empty.
fn unreadable() -> History //KEEP A COPY, START EMPTY
{
    let backup = format!("{}.old", path());
    let _ = fs::copy(path(), &backup);

    log::error!("Private message history could not be read, it is being ignored (copy kept as {backup})");
    History::default()
}

//PUBLIC
/// Returns the next account-local id and the unread count and newest unread id for each sender.
pub fn unread(username: &str) -> (u64, Vec<(String, u64, u64)>)
{
    HISTORY.lock().unwrap().unread(username)
}

/// Records a read position and returns whether it advanced.
pub fn mark_read(username: &str, peer: &str, through: u64) -> bool
{
    let mut history = HISTORY.lock().unwrap();
    let advanced = history.mark_read(username, peer, through);
    if advanced { history.save(); }
    advanced
}

/// Returns the other participant, only when `username` may read the message.
pub fn peer(id: u64, username: &str) -> Option<String> //RESOLVE THE ACCOUNT'S OWN MESSAGE ID
{
    let history = HISTORY.lock().unwrap();
    let record = &history.records[history.find(id, username)?];
    record.participants.peer(username).map(str::to_string)
}

/// Deletes the author's private message and returns its participants' ids.
pub fn delete(id: u64, username: &str) -> Option<Participants> //ONLY THE AUTHOR MAY DELETE IT
{
    let mut history = HISTORY.lock().unwrap();
    let index = history.find(id, username)?;
    if history.records[index].participants.sender != username { return None; }

    let record = history.records.remove(index);
    history.save();
    history.remove_media(record.media.iter().map(MessageMedia::hash));
    Some(record.participants)
}

/// Rewords the author's private message and returns its participants' ids.
pub fn edit(id: u64, username: &str, text: &str) -> Option<Participants> //ONLY THE AUTHOR MAY EDIT IT
{
    if text.trim().is_empty() { return None; }

    let mut history = HISTORY.lock().unwrap();
    let index = history.find(id, username)?;
    let record = &mut history.records[index];
    if record.participants.sender != username || record.media.is_some() { return None; }

    record.text = text.to_string();
    record.edited = true;
    let participants = record.participants.clone();
    history.save();
    Some(participants)
}

/// Toggles a participant's heart, returning the account-local ids and reactions.
pub fn heart(id: u64, username: &str) -> Option<(Participants, Vec<String>)> //ONLY THE TWO SIDES MAY REACT
{
    let mut history = HISTORY.lock().unwrap();
    let index = history.find(id, username)?;
    let record = &mut history.records[index];

    match record.hearts.iter().position(|name| name == username)
    {
        Some(index) => { record.hearts.remove(index); },
        None => record.hearts.push(username.to_string()),
    }

    let update = (record.participants.clone(), record.hearts.clone());
    history.save();
    Some(update)
}

/// Assigns each account its own id and, when enabled, stores the private message.
///
/// `reply` is resolved in the sender's account; both sides' references are saved so replies
/// keep pointing to the same message even after the original is deleted. With `persistent`
/// off, or a zero history limit, only the in-memory id counters advance.
pub fn store
(
    sender: &str,
    recipient: &str,
    text: &str,
    timestamp: Option<u64>,
    reply: Option<u64>,
    persistent: bool,
) -> Option<Delivery>
{
    store_message(sender, recipient, text, timestamp, reply, persistent, None)
}

/// Stores a private upload, committing its sealed file only if it will be retained.
pub fn store_media
(
    sender: &str,
    recipient: &str,
    text: &str,
    timestamp: Option<u64>,
    media: MessageMedia,
    pending: &Path,
    persistent: bool,
) -> Option<Delivery>
{
    store_message(sender, recipient, text, timestamp, None, persistent, Some((media, pending)))
}

fn store_message
(
    sender: &str,
    recipient: &str,
    text: &str,
    timestamp: Option<u64>,
    reply: Option<u64>,
    persistent: bool,
    upload: Option<(MessageMedia, &Path)>,
) -> Option<Delivery>
{
    let limit: usize = if persistent { super::read_config("max_persistent_private_messages") } else { 0 };
    let mut history = HISTORY.lock().unwrap();

    let reply = match reply
    {
        Some(id) =>
        {
            let target = &history.records[history.find(id, sender)?];
            if target.participants.peer(sender)? != recipient { return None; }
            Some((target.participants.message_id(sender)?, target.participants.message_id(recipient)?))
        },
        None => None,
    };

    if persistent && let Some((media, pending)) = &upload
    {
        if limit == 0 { return None; }
        fs::rename(pending, media_path(&media.hash())).ok()?;
    }

    let sender_id = history.take_user_id(sender);
    let recipient_id = if sender == recipient { sender_id } else { history.take_user_id(recipient) };
    let participants = Participants { sender: sender.to_string(), recipient: recipient.to_string(), sender_id, recipient_id };
    let delivery = Delivery
    {
        participants: participants.clone(),
        persistent: limit > 0,
        sender_reply: reply.map(|ids| ids.0),
        recipient_reply: reply.map(|ids| ids.1),
    };

    let id = history.next;
    history.next += 1;
    if limit == 0 { return Some(delivery); }

    history.records.push(Record
    {
        id,
        participants,
        text: text.to_string(),
        timestamp,
        before: super::messages::cursor(),
        reply,
        hearts: Vec::new(),
        edited: false,
        media: upload.map(|(media, _)| media),
    });

    let over = history.records.len().saturating_sub(limit);
    let removed: Vec<_> = history.records.drain(..over)
        .filter_map(|record| record.media.map(|media| media.hash())).collect();

    history.save();
    history.remove_media(removed.into_iter());
    Some(delivery)
}

/// Returns one page of `username`'s private history, oldest first.
///
/// # Parameters
/// - `username`: The account the page is built for.
/// - `before`: Return messages older than this id; `None` for the newest.
/// - `count`: The most messages on the page.
/// - `budget`: The approximate byte budget of the page; at least one message is always
///   returned so the cursor moves.
pub fn page(username: &str, before: Option<u64>, count: usize, budget: usize) -> Page
{
    let (records, start, more, kept) =
    {
        let history = HISTORY.lock().unwrap();

        let to = before.and_then(|id| history.records.iter().position(|message|
            message.participants.message_id(username).is_some_and(|local| local >= id)))
            .unwrap_or(history.records.len());

        let mut size = 0;
        let mut taken = 0;
        let mut from = to;

        //WALK BACK UNTIL THE PAGE IS FULL
        while from > 0 && taken < count
        {
            let record = &history.records[from - 1];

            //SOMEBODY ELSE'S MESSAGE IS NOT ON THE PAGE
            if !record.involves(username)
            {
                from -= 1;
                continue;
            }

            size += record.participants.sender.len() + record.participants.recipient.len() + record.text.len();

            //AT LEAST ONE MESSAGE SO THE CURSOR MOVES
            if size > budget && taken > 0 { break; }

            taken += 1;
            from -= 1;
        }

        let records: Vec<Record> = history.records[from..to].iter()
            .filter(|message| message.involves(username)).cloned().collect();

        let start = records.first().and_then(|message| message.participants.message_id(username))
            .unwrap_or_else(|| history.users.get(username).copied().unwrap_or(consts::PRIVATE_MESSAGE_ID));
        let more = history.records[..from].iter().any(|message| message.involves(username));
        let kept = history.records.iter().filter(|message| message.involves(username)).count() as u64;

        (records, start, more, kept)
    };

    Page { messages: wire(records, username), start, more, kept }
}

//RECORDS AS THE WIRE CARRIES THEM
/// Converts records into wire messages, from `username`'s point of view.
///
/// The name is always the other side, `sent` says which side we are, and the colors are looked
/// up as the accounts look now. Stored timestamps are hidden while `message_timestamps` is off.
fn wire(records: Vec<Record>, username: &str) -> Vec<StoredPrivateMessage>
{
    let mut looked_up: HashMap<String, MessageColors> = HashMap::new();
    let timestamps = super::read_config::<bool>("message_timestamps");

    records.into_iter().filter(|message| message.involves(username)).map(|message|
    {
        //WHAT server_users.toml HOLDS FOR THEM NOW
        let mut colors_of = |name: &str| looked_up.entry(name.to_string())
            .or_insert_with(|| super::users::colors(name)).clone();

        let message_id = message.participants.message_id(username).unwrap();
        let sent = message.participants.sender == username;
        let peer = match sent
        {
            true => message.participants.recipient,
            false => message.participants.sender,
        };

        let colors = match sent
        {
            //THE RECIPIENT'S NAME, OUR TEXT, AS THE LIVE ECHO PAINTS IT
            true => MessageColors
            {
                username_color: colors_of(&peer).username_color,
                message_color: colors_of(username).message_color,
            },

            false => colors_of(&peer),
        };

        StoredPrivateMessage
        {
            message_id,
            before: message.before,
            reply: message.reply.map(|ids| if sent { ids.0 } else { ids.1 }),
            hearts: message.hearts,
            edited: message.edited,
            id: super::users::id(&peer).unwrap_or_default(),
            username: peer,
            sent,
            text: message.text,
            media: message.media,
            colors,
            timestamp: message.timestamp.filter(|_| timestamps), //HIDDEN WHILE TURNED OFF
        }
    }).collect()
}

/// The separate directory used for sealed private attachments.
pub fn media_path(hash: &[u8; 32]) -> PathBuf
{
    media_dir().join(crate::misc::hex(hash))
}

/// The storage root for private uploads.
pub fn media_dir() -> PathBuf
{
    PathBuf::from(super::config_path("server_private_media"))
}

/// Whether new private uploads can be retained by the server.
pub fn media_persistence_enabled() -> bool
{
    super::read_config::<bool>("persistent_messages") &&
        super::read_config::<bool>("persistent_private_messages") &&
        super::read_config::<usize>("max_persistent_private_messages") > 0
}

/// Removes interrupted uploads and unreferenced sealed files at startup.
pub fn sweep_media()
{
    let dir = media_dir();
    let _ = fs::remove_dir_all(dir.join("pending"));
    let Ok(files) = fs::read_dir(dir) else { return };
    let history = HISTORY.lock().unwrap();
    let kept: HashSet<_> = history.records.iter().filter_map(|record|
        record.media.as_ref().map(MessageMedia::hash)).collect();

    for hash in files.flatten().filter_map(|file| file.file_name().to_str().and_then(crate::misc::unhex))
    {
        if !kept.contains(&hash) { let _ = fs::remove_file(media_path(&hash)); }
    }
}

/// Looks up a downloadable file by the requesting participant's own message id.
pub fn attachment(id: u64, username: &str) -> Option<([u8; 32], String)>
{
    HISTORY.lock().unwrap().attachment(id, username)
}

/// Checks whether the requesting account can fetch a private file or clip by hash.
pub fn can_fetch(hash: &[u8; 32], username: &str) -> bool
{
    HISTORY.lock().unwrap().can_fetch(hash, username)
}

impl History
{
    fn attachment(&self, id: u64, username: &str) -> Option<([u8; 32], String)>
    {
        let record = &self.records[self.find(id, username)?];
        let MessageMedia::File(hash) = record.media.as_ref()? else { return None };
        Some((*hash, record.text.clone()))
    }

    fn can_fetch(&self, hash: &[u8; 32], username: &str) -> bool
    {
        self.records.iter().any(|record|
            record.involves(username) && record.media.as_ref().is_some_and(|media| media.hash() == *hash))
    }

    fn remove_media(&self, hashes: impl Iterator<Item = [u8; 32]>)
    {
        for hash in hashes
        {
            if !self.records.iter().any(|record| record.media.as_ref().is_some_and(|media| media.hash() == hash))
            {
                let _ = fs::remove_file(media_path(&hash));
            }
        }
    }
}
