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
    mem,
    iter,
    time::Instant,
    collections::
    {
        BTreeMap,
        BTreeSet,
        HashMap,
        VecDeque,
    },
};

use ratatui::
{
    layout::Rect,
    style::{ Color, Style },
    text::{ Line, Span },
};

use unicode_width::UnicodeWidthChar;

use image::{ DynamicImage, Rgba };

use ratatui_image::
{
    FontSize,
    FilterType,
    picker::{ Capability, Picker, cap_parser::QueryStdioOptions },
    protocol::{ StatefulProtocol, StatefulProtocolType },
};

use crate::
{
    config,
    misc,
    role::Role,
    options::{ self, LoginState },
    network::
    {
        codes::
        {
            MessageColors,
            OnlineUser,
            Device,
        },
        client::
        {
            self,
            VoiceUser,
            image::{ Animation, ImageFrame },
        },
    },
};

#[cfg(feature = "client_voice")]
use crate::network::voice::client::options as voice_options;

#[cfg(feature = "client_screen")]
use crate::network::screen::client::options as screen_options;

use super::
{
    consts,
    account::Account,
    input::InputBuffer,
    login::{ Login, Reconnect, Stage },
    palette::{ self, Palette },
    settings::Settings,
    tofu::Prompt,
    theme::{ self, Theme },
};

//ENUMS
pub enum Entry //ONE ROW OF HISTORY
{
    Line(Line<'static>), //ALREADY STYLED - CLIENT OUTPUT, NOTICES, BLOCK COMMANDS

    //A CHAT MESSAGE, STORED UNRENDERED
    Message
    {
        username: String,
        id: usize,
        message_id: u64,
        timestamp: Option<u64>, //UNIX SECONDS
        text: String,
        colors: MessageColors,
        reply: Option<u64>,     //THE MESSAGE IT ANSWERS
        hearts: Vec<String>,    //WHO HEARTED IT
        edited: bool,           //CHANGED SINCE SENT
    },

    //A REPLAYED MESSAGE, WITHOUT A CLIENT ID
    History
    {
        username: String,
        message_id: u64,
        timestamp: Option<u64>,
        text: String,
        colors: MessageColors,
        reply: Option<u64>,
        hearts: Vec<String>,
        edited: bool,
    },

    //A PRIVATE MESSAGE, STORED UNRENDERED
    Private
    {
        sent: bool, //TO username, ELSE FROM
        username: String,
        id: usize,
        text: String,
        colors: MessageColors,
    },

    //A TRANSFER AND HOW FAR IT HAS GOT
    Transfer(Transfer),

    //A PICTURE AND ITS CAPTION
    Image
    {
        username: String,
        filename: String,
        message_id: u64,
        timestamp: Option<u64>,
        username_color: Option<u8>, //THE SENDER'S, LIKE A MESSAGE'S
        hash: Option<[u8; 32]>,     //WHAT TO ASK THE SERVER FOR
        picture: Picture,
        hearts: Vec<String>,
    },
}

impl Entry
{
    pub fn message_id(&self) -> Option<u64> //THE SERVER'S ID, IF IT IS A MESSAGE
    {
        match self
        {
            Entry::Message { message_id, .. } | Entry::History { message_id, .. } | Entry::Image { message_id, .. } =>
                Some(*message_id),
            _ => None,
        }
    }

    pub fn username(&self) -> Option<&str> //WHO SAID IT, IF IT IS A MESSAGE
    {
        match self
        {
            Entry::Message { username, .. } | Entry::History { username, .. } | Entry::Image { username, .. } => Some(username),
            _ => None,
        }
    }

    pub fn hearts(&self) -> &[String] //WHO HEARTED IT
    {
        match self
        {
            Entry::Message { hearts, .. } | Entry::History { hearts, .. } | Entry::Image { hearts, .. } => hearts,
            _ => &[],
        }
    }

    fn set_hearts(&mut self, new: Vec<String>) //REPLACE WHO HEARTED IT
    {
        if let Entry::Message { hearts, .. } | Entry::History { hearts, .. } | Entry::Image { hearts, .. } = self { *hearts = new; }
    }

    pub fn edited(&self) -> bool //CHANGED SINCE SENT
    {
        matches!(self, Entry::Message { edited: true, .. } | Entry::History { edited: true, .. })
    }

    fn set_text(&mut self, new: String) //REWORD IT
    {
        if let Entry::Message { text, edited, .. } | Entry::History { text, edited, .. } = self
        {
            *text = new;
            *edited = true;
        }
    }

    pub fn reply(&self) -> Option<u64> //THE MESSAGE IT ANSWERS
    {
        match self
        {
            Entry::Message { reply, .. } | Entry::History { reply, .. } => *reply,
            _ => None,
        }
    }

    pub fn striped(&self) -> bool //WHETHER IT TAKES PART IN THE STRIPES
    {
        matches!(self, Entry::Message { .. } | Entry::History { .. } | Entry::Private { .. } | Entry::Image { .. })
    }
}

//WHAT THERE IS TO DRAW UNDER A CAPTION
pub enum Picture
{
    Absent,            //NOT ASKED FOR YET
    Deferred,          //LOADED ONCE IT IS ON SCREEN
    Waiting,           //ASKED FOR, NOT HERE YET
    Gone,              //THE SERVER DOES NOT HAVE IT ANY MORE
    Ready(Box<Fitted>),
}

//STRUCTS
pub struct Transfer //A FILE ON ITS WAY, IN OR OUT
{
    pub uid: u64,              //WHAT A PROGRESS TICK NAMES
    pub upload: bool,          //WHICH WAY IT GOES
    pub image: bool,           //WHAT TO CALL IT
    pub filename: String,
    pub done: u64,
    pub total: u64,
    pub outcome: Option<bool>, //None WHILE IT RUNS
}

pub struct Fitted //A PICTURE AT THE SIZE THE PANE DRAWS IT AT
{
    pub frames: Animation,                  //THE PICTURE ITSELF, KEPT TO FIT AGAIN AT A NEW PANE WIDTH
    pub current: usize,                     //WHICH FRAME `protocol` HOLDS - A STILL HAS ONLY THE ONE
    pub next: Instant,                      //WHEN THE FRAME AFTER IT IS DUE
    pub rows: u16,                          //ROWS IT RESERVES AT THAT WIDTH
    pub fitted: u16,                        //THE WIDTH `protocol` WAS FITTED TO
    pub protocol: Option<StatefulProtocol>, //None WHILE THE PICTURE IS OFF SCREEN
    pub unloaded: Option<Unloaded>,         //THE TERMINAL'S ID FOR IT, KEPT WHILE IT IS
}

//WHAT AN UNLOADED PICTURE KEEPS OF ITS PROTOCOL
pub struct Unloaded
{
    pub kind: StatefulProtocolType,
    pub background: Option<Rgba<u8>>,
}

#[derive(Clone, Copy)]
pub struct Placement //WHERE ONE IMAGE SITS IN THE WRAPPED VIEW
{
    pub entry: usize,  //WHICH App::messages ENTRY IT BELONGS TO
    pub caption: u16,  //FIRST ROW OF THE CAPTION
    pub row: u16,      //FIRST RESERVED ROW (WHERE THE CAPTION ENDS)
    pub height: u16,   //RESERVED ROWS - 0 WHILE THERE IS NO PICTURE
}

//A DRAG, IN WRAPPED-VIEW ROWS
#[derive(Clone, Copy)]
pub struct Selection
{
    pub anchor: (u16, u16), //(ROW IN THE WRAPPED VIEW, COLUMN INSIDE THE PANE)
    pub cursor: (u16, u16),
    pub dragged: bool,      //A DRAG EVER ARRIVED
}

//STRUCTS
pub struct App
{
    //MESSAGE PANE
    pub messages: VecDeque<Entry>, //THE PANE BEING LOOKED AT - THE CHANNEL WE ARE STANDING IN
    pub channel: String,           //WHICH CHANNEL THAT IS ("" = THE LOBBY)
    pub panes: HashMap<String, VecDeque<Entry>>, //THE OTHER CHANNELS' SCROLLBACK, PARKED WHILE WE ARE AWAY
    pub scroll: Option<u16>, //None = STUCK TO THE BOTTOM
    pub unread: usize,       //MESSAGES ARRIVED WHILE SCROLLED AWAY

    //SIDEBAR
    pub username: String, //OUR OWN USERNAME (options::get_server_username IS THE SERVER'S NAME)
    pub role: Role,       //OUR OWN ROLE
    pub online: Vec<OnlineUser>,
    pub offline: BTreeMap<String, Option<u8>>, //REGISTERED USERS NOBODY IS CONNECTED AS, AND THEIR COLORS
    pub offline_listed: bool, //WHETHER THE SERVER SENDS THEM AT ALL
    pub devices: HashMap<String, Device>, //WHAT EACH USER JOINED ON, KEYED BY USERNAME
    pub channels: BTreeSet<String>, //NAMED CHANNELS THE SERVER CURRENTLY HOLDS
    pub voice: Vec<VoiceUser>, //WHAT THE VOICE PANEL DRAWS
    pub voice_roster: BTreeMap<usize, String>, //WHO THE SERVER SAYS IS IN VOICE IN OUR CHANNEL (US EXCLUDED)
    pub voice_activity: Vec<VoiceUser>, //WHO WE ARE ACTUALLY HEARING
    pub voice_enabled: bool,

    //CONNECTION (SHOWN IN THE MESSAGE PANE TITLE)
    pub address: String,     //AS THE USER TYPED IT - NO IMPLICIT PORT
    pub server_name: String, //THE SERVER'S OWN NAME, ONCE IT HAS INTRODUCED ITSELF

    //INPUT
    pub input: InputBuffer,
    pub palette: Palette,
    pub settings: Settings, //SETTINGS OVERLAY (CLOSED UNLESS THE USER OPENED IT)
    pub login: Option<Login>, //CONNECT BOX
    pub tofu: Option<Prompt>, //SERVER IDENTITY PROMPT
    pub account: Option<Account>, //ACCOUNT FORM
    pub theme: Theme,
    pub picker: Picker, //WHAT THE TERMINAL CAN DRAW, AND HOW BIG ITS CELLS ARE

    //WHERE THE MESSAGE PANE WAS LAST DRAWN
    pub pane: Rect,
    pub pane_offset: u16,
    pub selection: Option<Selection>, //A DRAG-SELECTED RUN OF THE PANE, KEPT UNTIL THE NEXT PRESS

    //A TOAST IN THE CHROME, WHICH EXPIRES
    pub notice: Option<(String, Instant)>,

    //WHO IS WRITING IN OUR CHANNEL, AND WHEN THEY LAST SAID SO
    pub typing_users: BTreeMap<String, Instant>,
    typing: bool,                   //THE LINE CHANGED SINCE THE LAST TICK
    typing_sent: Option<Instant>,   //WHEN WE LAST TOLD THE SERVER

    //REQUEST BOOKKEEPING
    pub list_requested: bool,
    #[cfg(feature = "client_screen")]
    pub screens_requested: bool,

    //PICTURES TO ASK THE SERVER FOR
    pub image_requests: Vec<[u8; 32]>,

    //PICTURES THAT SCROLLED INTO VIEW, TO LOAD OUT OF THE CACHE
    pub image_loads: Vec<[u8; 32]>,

    //PICTURES ASKED OF THE SERVER, NOT ANSWERED YET
    image_fetching: Vec<[u8; 32]>,

    //LOBBY HISTORY PAGING
    history_anchor: Option<usize>,    //WHERE THE FIRST REPLAYED ENTRY SITS IN THE LOBBY PANE
    history_cursor: Option<u64>,      //ITS SERVER INDEX, WHILE OLDER ONES ARE LEFT
    history_pending: bool,            //A PAGE WAS ASKED FOR
    pub history_request: Option<u64>, //THE PAGE THE TICK ASKS FOR

    //LIFECYCLE
    pub leaving: bool,      //THE USER ASKED TO LEAVE
    pub logging_out: bool,  //THE USER ASKED TO LOG OUT
    pub disconnect_reason: Option<String>, //WHY THE SERVER IS ABOUT TO DROP US
    pub reconnect: Reconnect, //DIALS ITSELF BACK AFTER A DROP THE USER DID NOT ASK FOR
    pub drop_stream: bool,  //THE LOOP OWNS THE WRITE HALF
    pub should_quit: bool,
    pub exit_code: i32,
    pub quit_message: Option<String>, //PRINTED ON THE NORMAL SCREEN AFTER TEARDOWN
    pub dirty: bool,

    //WHAT THE LAST FRAME DREW OVER THE PANE
    overlays: Vec<Rect>,
    picture_rows: Vec<(u16, String)>, //AND EACH PICTURE ROW'S FIRST CELL
    pub avatar_marks: Vec<(u16, usize)>, //HOW MANY TIMES EACH AVATAR ROW WAS MARKED

    //WRAP CACHE
    generation: u64,
    wrapped: Option<(u16, u64, Vec<Line<'static>>, Vec<Placement>, Vec<u16>, Vec<Option<Style>>)>,
    stripe: bool, //PARITY OF THE MESSAGES TRIMMED OFF THE TOP
    pub stripe_bg: Color,
}

//IMPLEMENTATIONS
impl Selection
{
    pub fn ordered(&self) -> ((u16, u16), (u16, u16)) //THE TWO ENDS IN READING ORDER
    {
        if self.cursor < self.anchor { (self.cursor, self.anchor) } else { (self.anchor, self.cursor) }
    }
}

impl Default for App
{
    fn default() -> Self { Self::new() }
}

impl App
{
    pub fn new() -> Self
    {
        Self
        {
            messages: VecDeque::new(),
            channel: String::new(),
            panes: HashMap::new(),
            scroll: None,
            unread: 0,
            username: String::new(),
            role: Role::default(),
            online: Vec::new(),
            offline: BTreeMap::new(),
            offline_listed: false,
            devices: HashMap::new(),
            channels: BTreeSet::new(),
            voice: Vec::new(),
            voice_roster: BTreeMap::new(),
            voice_activity: Vec::new(),
            voice_enabled: false,
            address: String::new(),
            server_name: String::new(),
            input: InputBuffer::new(),
            palette: Palette::new(),
            settings: Settings::new(),
            login: Some(Login::new()),
            tofu: None,
            account: None,
            theme: Theme::load(),
            overlays: Vec::new(),
            picture_rows: Vec::new(),
            avatar_marks: Vec::new(),
            picker: Picker::halfblocks(), //UNTIL init_picker HAS ASKED THE TERMINAL
            pane: Rect::ZERO,
            pane_offset: 0,
            selection: None,
            notice: None,
            typing_users: BTreeMap::new(),
            typing: false,
            typing_sent: None,
            list_requested: false,
            #[cfg(feature = "client_screen")]
            screens_requested: false,
            image_requests: Vec::new(),
            image_loads: Vec::new(),
            image_fetching: Vec::new(),
            history_anchor: None,
            history_cursor: None,
            history_pending: false,
            history_request: None,
            leaving: false,
            logging_out: false,
            disconnect_reason: None,
            reconnect: Reconnect::default(),
            drop_stream: false,
            should_quit: false,
            exit_code: 0,
            quit_message: None,
            dirty: true,
            generation: 0,
            stripe: false,
            stripe_bg: theme::STRIPE_FALLBACK,
            wrapped: None,
        }
    }

    //BUILD THE PANEL FROM ROSTER + LOCAL SESSION
    pub fn rebuild_voice(&mut self)
    {
        let mut users: Vec<VoiceUser> = Vec::with_capacity(self.voice_roster.len() + 1);

        //US, FROM THE LOCAL SESSION
        if self.voice_enabled
        {
            users.push(match self.voice_activity.iter().find(|user| user.is_local)
            {
                Some(local) => VoiceUser { username: self.username.clone(), ..*local },

                //THE FIRST TICK IS UP TO 100 ms AWAY
                None => VoiceUser
                {
                    id: 0,
                    username: self.username.clone(),
                    is_speaking: false,
                    latency: None,
                    is_local: true,
                },
            });
        }

        //EVERYBODY ELSE, IN ID ORDER
        for (id, username) in self.voice_roster.iter()
        {
            let heard = self.voice_activity.iter().find(|user| !user.is_local && user.id == *id);

            users.push(VoiceUser
            {
                id: *id,
                username: username.clone(),
                is_speaking: heard.is_some_and(|user| user.is_speaking),
                latency: heard.and_then(|user| user.latency),
                is_local: false,
            });
        }

        self.voice = users;
        self.dirty = true;
    }

    //OUTPUT
    pub fn push(&mut self, line: Line<'static>)
    {
        self.push_entry(Entry::Line(line));
    }

    //STORE A CHAT MESSAGE UNRENDERED
    pub fn push_message(&mut self, username: String, id: usize, message_id: u64, timestamp: Option<u64>, text: String,
        colors: MessageColors, reply: Option<u64>)
    {
        self.push_entry(Entry::Message { username, id, message_id, timestamp, text, colors, reply, hearts: Vec::new(), edited: false });
    }

    //STORE AN ENTRY IN ANOTHER CHANNEL'S PARKED PANE
    pub fn park_entry(&mut self, channel: String, entry: Entry)
    {
        let lobby = channel.is_empty();
        let pane = self.panes.entry(channel).or_default();

        pane.push_back(entry);

        while pane.len() > consts::HISTORY_LIMIT
        {
            pane.pop_front();

            //THE REPLAYED ENTRIES MOVE UP, OR GO
            if lobby
            {
                self.history_anchor = self.history_anchor.and_then(|anchor| anchor.checked_sub(1));
                if self.history_anchor.is_none() { self.history_cursor = None; }
            }
        }
    }

    //STORE A PRIVATE MESSAGE UNRENDERED
    pub fn push_private(&mut self, sent: bool, username: String, id: usize, text: String, colors: MessageColors)
    {
        self.push_entry(Entry::Private { sent, username, id, text, colors });
    }

    //THE NEWEST PAGE OF THE LOBBY'S HISTORY
    pub fn start_history(&mut self, entries: Vec<Entry>, start: u64, more: bool)
    {
        self.history_anchor = Some(self.messages.len());
        self.history_cursor = more.then_some(start);

        for entry in entries { self.push_entry(entry); }
    }

    //AN OLDER PAGE, ABOVE WHAT IS ALREADY THERE
    pub fn prepend_history(&mut self, entries: Vec<Entry>, start: u64, more: bool)
    {
        self.history_pending = false;

        //ONLY THE LOBBY'S PANE HAS A HISTORY
        if !self.channel.is_empty() { return; }

        let Some(anchor) = self.history_anchor else { return };

        //THE PANE'S CAP ENDS THE PAGING
        let room = consts::HISTORY_LIMIT.saturating_sub(self.messages.len());
        let skip = entries.len().saturating_sub(room);

        self.history_cursor = (more && skip == 0).then_some(start);

        let before = self.wrapped_rows();

        let tail = self.messages.split_off(anchor);
        self.messages.extend(entries.into_iter().skip(skip));
        self.messages.extend(tail);

        self.generation += 1;
        self.dirty = true;

        //KEEP THE VIEW ON WHAT IT WAS SHOWING
        let grown = self.wrapped_rows().saturating_sub(before);

        if let Some(scroll) = self.scroll.as_mut() { *scroll = scroll.saturating_add(grown); }

        if let Some(selection) = self.selection.as_mut()
        {
            selection.anchor.0 = selection.anchor.0.saturating_add(grown);
            selection.cursor.0 = selection.cursor.0.saturating_add(grown);
        }
    }

    //DROP A DELETED MESSAGE FROM WHICHEVER PANE HOLDS IT
    pub fn delete_message(&mut self, message_id: u64)
    {
        let lobby = self.channel.is_empty();

        if let Some(index) = self.messages.iter().position(|entry| entry.message_id() == Some(message_id))
        {
            self.remove_entry(index, lobby);
        }
        //A PARKED PANE ONLY LOSES THE ENTRY
        else if let Some(pane) = self.panes.get_mut("")
            && let Some(index) = pane.iter().position(|entry| entry.message_id() == Some(message_id))
        {
            pane.remove(index);
            self.shift_anchor(index);
        }
    }

    //WHO HEARTED A LOADED MESSAGE
    pub fn hearts_of(&self, message_id: u64) -> Option<Vec<String>>
    {
        self.messages.iter().chain(self.panes.get("").into_iter().flatten())
            .find(|entry| entry.message_id() == Some(message_id))
            .map(|entry| entry.hearts().to_vec())
    }

    //A MESSAGE'S HEARTS CHANGED
    pub fn set_hearts(&mut self, message_id: u64, hearts: Vec<String>)
    {
        self.update_message(message_id, |entry| entry.set_hearts(hearts));
    }

    //A MESSAGE WAS REWORDED
    pub fn edit_message(&mut self, message_id: u64, text: String)
    {
        self.update_message(message_id, |entry| entry.set_text(text));
    }

    //CHANGE A MESSAGE IN PLACE, WHICHEVER PANE HOLDS IT
    fn update_message(&mut self, message_id: u64, change: impl FnOnce(&mut Entry))
    {
        let Some(index) = self.messages.iter().position(|entry| entry.message_id() == Some(message_id)) else
        {
            //A PARKED PANE ONLY UPDATES THE ENTRY
            if let Some(entry) = self.panes.get_mut("").and_then(|pane| pane.iter_mut().find(|entry| entry.message_id() == Some(message_id)))
            {
                change(entry);
            }

            return;
        };

        self.rewrap(self.pane.width);

        let before = self.wrapped_len();
        let end = self.wrapped.as_ref().and_then(|wrapped| wrapped.4.get(index + 1).copied()).unwrap_or(before);

        change(&mut self.messages[index]);

        self.generation += 1;
        self.dirty = true;

        let delta = self.wrapped_rows() as i32 - before as i32;

        //ROWS BELOW IT MOVE WITH IT
        let shift = |row: u16| match row >= end
        {
            true => (row as i32 + delta).max(0) as u16,
            false => row,
        };

        if let Some(scroll) = self.scroll.as_mut() { *scroll = shift(*scroll); }

        if let Some(selection) = self.selection.as_mut()
        {
            selection.anchor.0 = shift(selection.anchor.0);
            selection.cursor.0 = shift(selection.cursor.0);
        }
    }

    //REMOVE AN ENTRY OF THE PANE BEING LOOKED AT
    fn remove_entry(&mut self, index: usize, lobby: bool)
    {
        self.rewrap(self.pane.width);

        let first = self.wrapped.as_ref().and_then(|wrapped| wrapped.4.get(index).copied()).unwrap_or(0);
        let before = self.wrapped_len();

        self.messages.remove(index);
        if lobby { self.shift_anchor(index); }

        self.generation += 1;
        self.dirty = true;

        let removed = before.saturating_sub(self.wrapped_rows());

        //ROWS BELOW IT MOVE UP, ROWS INSIDE IT LAND ON ITS START
        let shift = |row: u16| match row
        {
            row if row >= first + removed => row - removed,
            row if row > first => first,
            row => row,
        };

        if let Some(scroll) = self.scroll.as_mut() { *scroll = shift(*scroll); }

        if let Some(selection) = self.selection.as_mut()
        {
            selection.anchor.0 = shift(selection.anchor.0);
            selection.cursor.0 = shift(selection.cursor.0);
        }
    }

    fn shift_anchor(&mut self, index: usize) //THE REPLAYED ENTRIES MOVE UP WITH IT
    {
        if let Some(anchor) = self.history_anchor.as_mut() && index < *anchor { *anchor -= 1; }
    }

    //ROWS THE PANE WRAPS TO AT ITS LAST WIDTH
    fn wrapped_rows(&mut self) -> u16
    {
        self.rewrap(self.pane.width);
        self.wrapped_len()
    }

    //THE QUEUED FETCHES STILL WORTH MAKING, A FEW AT A TIME
    pub fn take_image_requests(&mut self) -> Vec<[u8; 32]>
    {
        if self.image_requests.is_empty() { return Vec::new(); }

        let drawn = self.pane.height > 0;
        let visible = match drawn
        {
            true => self.on_screen(),
            false => Vec::new(),
        };

        let mut send = Vec::new();
        let mut queued = Vec::new();

        for hash in mem::take(&mut self.image_requests)
        {
            let waiting: Vec<usize> = self.messages.iter().enumerate()
                .filter(|(_, entry)| matches!(entry, Entry::Image { hash: Some(h), picture: Picture::Waiting, .. } if *h == hash))
                .map(|(entry, _)| entry)
                .collect();

            //SCROLLED AWAY BEFORE ITS TURN
            if drawn && !waiting.is_empty() && !waiting.iter().any(|entry| visible.contains(entry))
            {
                for entry in waiting
                {
                    if let Some(Entry::Image { picture, .. }) = self.messages.get_mut(entry) { *picture = Picture::Deferred; }
                }

                continue;
            }

            match self.image_fetching.len() < consts::MAX_IMAGE_FETCHES
            {
                true =>
                {
                    self.image_fetching.push(hash);
                    send.push(hash);
                },

                false => queued.push(hash),
            }
        }

        self.image_requests = queued;

        send
    }

    //A FETCH CAME BACK
    pub fn fetched(&mut self, hash: &[u8; 32])
    {
        if let Some(index) = self.image_fetching.iter().position(|h| h == hash) { self.image_fetching.swap_remove(index); }
    }

    //ENTRIES WHOSE CAPTION IS WITHIN REACH
    fn on_screen(&mut self) -> Vec<usize>
    {
        let (offset, height) = (self.pane_offset, self.pane.height);

        self.placements(self.pane.width).into_iter()
            .filter(|placement| in_reach(placement, offset, height))
            .map(|placement| placement.entry)
            .collect()
    }

    //A PICTURE THAT CAME WITH ITS BYTES
    pub fn push_image(&mut self, username: String, filename: String, message_id: u64, timestamp: Option<u64>,
        image: Animation, username_color: Option<u8>)
    {
        let picture = self.fit(image);

        self.push_entry(Entry::Image { username, filename, message_id, timestamp, username_color, hash: None, picture, hearts: Vec::new() });
    }

    //A TRANSFER STARTING
    pub fn push_transfer(&mut self, uid: u64, filename: String, total: u64, upload: bool, image: bool)
    {
        self.push_entry(Entry::Transfer(Transfer { uid, upload, image, filename, done: 0, total, outcome: None }));
    }

    //MOVE A TRANSFER'S BAR ALONG
    pub fn update_transfer(&mut self, uid: u64, done: u64)
    {
        let Some(transfer) = self.transfer(uid) else { return };

        let before = percent(transfer.done, transfer.total);

        transfer.done = done;

        //THE ROW ONLY CHANGES WITH THE PERCENTAGE
        if percent(done, transfer.total) == before { return; }

        self.generation += 1;
        self.dirty = true;
    }

    //A TRANSFER THAT ENDED, WELL OR NOT
    pub fn finish_transfer(&mut self, uid: u64, ok: bool)
    {
        let Some(transfer) = self.transfer(uid) else { return };

        transfer.done = transfer.total;
        transfer.outcome = Some(ok);

        self.generation += 1;
        self.dirty = true;
    }

    //THE NEWEST ROW THAT TICK NAMES
    fn transfer(&mut self, uid: u64) -> Option<&mut Transfer>
    {
        self.messages.iter_mut().rev().find_map(|entry| match entry
        {
            Entry::Transfer(transfer) if transfer.uid == uid => Some(transfer),
            _ => None,
        })
    }

    //A CAPTION WITHOUT ITS PICTURE
    pub fn push_caption(&mut self, username: String, filename: String, message_id: u64, timestamp: Option<u64>,
        hash: [u8; 32], picture: Picture, username_color: Option<u8>)
    {
        self.push_entry(Entry::Image { username, filename, message_id, timestamp, username_color, hash: Some(hash), picture, hearts: Vec::new() });
    }

    //A CLICKED CAPTION
    pub fn request_image(&mut self, entry: usize) -> Option<[u8; 32]>
    {
        let Some(Entry::Image { hash, picture, .. }) = self.messages.get_mut(entry) else { return None };

        if !matches!(picture, Picture::Absent | Picture::Gone) { return None; }

        *picture = Picture::Waiting;

        self.generation += 1;
        self.dirty = true;

        *hash
    }

    //FILL THE OLDEST LINE STILL WAITING
    pub fn deliver_image(&mut self, hash: [u8; 32], image: Option<Animation>)
    {
        let (picture, asked) = match image
        {
            Some(image) => (self.fit(image), false),
            None => (Picture::Gone, true),
        };

        let waiting = self.messages.iter().position(|entry| match entry
        {
            Entry::Image { hash: Some(h), picture: slot, .. } if *h == hash => match asked
            {
                true => matches!(slot, Picture::Waiting),
                false => matches!(slot, Picture::Absent | Picture::Waiting),
            },

            _ => false,
        });

        let Some(entry) = waiting else { return };

        if let Some(Entry::Image { picture: slot, .. }) = self.messages.get_mut(entry) { *slot = picture; }

        self.generation += 1;
        self.dirty = true;
    }

    //CUT EVERY FRAME DOWN TO IMAGE_ROWS
    fn fit(&self, image: Animation) -> Picture
    {
        Picture::Ready(self.fit_rows(image, consts::IMAGE_ROWS))
    }

    //THE SAME, TO WHATEVER ROW BUDGET THE PICTURE IS DRAWN UNDER
    fn fit_rows(&self, image: Animation, rows: u16) -> Box<Fitted>
    {
        let font = self.picker.font_size();
        let limit = rows as u32 * font.height as u32;

        //EVERY FRAME IS HELD AT ONCE
        let frames = image.into_iter().map(|ImageFrame { image, delay }|
        {
            let image = match image.height() > limit
            {
                true => image.resize(image.width(), limit, FilterType::Triangle),
                false => image,
            };

            ImageFrame { image, delay }
        }).collect::<Animation>();

        let next = Instant::now() + frames.first().map(|frame| frame.delay).unwrap_or_default();

        Box::new(Fitted
        {
            frames,
            current: 0,
            next,
            rows: 1,
            fitted: 0,
            protocol: None,
            unloaded: None,
        })
    }

    //THE PROFILE PICTURE THE BOX ASKED FOR
    pub fn deliver_avatar(&mut self, hash: [u8; 32], image: Option<Animation>)
    {
        let Some(image) = image else { return };

        self.settings.picture = Some(self.fit_rows(image, consts::AVATAR_ROWS));
        self.settings.picture_of = Some(hash);
        self.dirty = true;
    }

    //WHETHER A PICTURE THAT CAME BACK IS THE ONE THE PROFILE BOX IS WAITING FOR
    pub fn wants_avatar(&self, hash: &[u8; 32]) -> bool
    {
        self.settings.open && self.settings.avatar.as_ref() == Some(hash)
            && self.settings.picture_of.as_ref() != Some(hash)
    }

    //BUILD THE PROFILE PICTURE AT THE SIZE THE BOX RESERVED FOR IT
    pub fn load_avatar(&mut self, width: u16)
    {
        let font = self.picker.font_size();

        let Some(ready) = self.settings.picture.as_mut() else { return };

        if ready.fitted == width && ready.protocol.is_some() { return; }

        let image = fit_image(&ready.frames[ready.current].image, width, consts::AVATAR_ROWS, font);

        //REUSE THE PROTOCOL TYPE TO KEEP THE IMAGE ID
        ready.protocol = match ready.protocol.take()
        {
            Some(protocol) => Some(StatefulProtocol::new(image, font,
                protocol.background_color(), protocol.protocol_type_owned())),

            None => Some(self.picker.new_resize_protocol(image)),
        };

        ready.fitted = width;
    }

    //AND STEP IT, WHICH THE PANE'S CLOCK DOES NOT REACH
    fn advance_avatar(&mut self)
    {
        if !self.settings.open { return; }

        let font = self.picker.font_size();
        let now = Instant::now();

        let Some(ready) = self.settings.picture.as_mut() else { return };

        //A STILL NEVER ADVANCES
        if ready.frames.len() < 2 || ready.protocol.is_none() || now < ready.next { return; }

        //TOO FAR BEHIND TO CATCH UP
        if now.duration_since(ready.next) > consts::ANIMATION_CATCHUP { ready.next = now; }

        while now >= ready.next
        {
            ready.current = (ready.current + 1) % ready.frames.len();
            ready.next += ready.frames[ready.current].delay;
        }

        let image = fit_image(&ready.frames[ready.current].image, ready.fitted, consts::AVATAR_ROWS, font);

        ready.protocol = ready.protocol.take().map(|protocol|
        {
            let background = protocol.background_color();

            StatefulProtocol::new(image, font, background, protocol.protocol_type_owned())
        });

        self.dirty = true;
    }

    //STEP EVERY ANIMATION THAT IS DUE
    pub fn advance_animations(&mut self)
    {
        self.advance_avatar();

        let pane = self.pane;

        if pane.width == 0 || pane.height == 0 { return; }

        let now = Instant::now();
        let offset = self.pane_offset;
        let font = self.picker.font_size();

        //ONLY THE PICTURES ON SCREEN
        let visible = self.placements(pane.width).into_iter()
            .filter(|placement| placement.height > 0
                && placement.row < offset + pane.height
                && placement.row + placement.height > offset)
            .map(|placement| placement.entry)
            .collect::<Vec<usize>>();

        for entry in visible
        {
            let Some(Entry::Image { picture: Picture::Ready(ready), .. }) = self.messages.get_mut(entry)
                else { continue };

            //A STILL NEVER ADVANCES
            if ready.frames.len() < 2 || ready.protocol.is_none() || now < ready.next { continue; }

            //TOO FAR BEHIND TO CATCH UP
            if now.duration_since(ready.next) > consts::ANIMATION_CATCHUP { ready.next = now; }

            while now >= ready.next
            {
                ready.current = (ready.current + 1) % ready.frames.len();
                ready.next += ready.frames[ready.current].delay;
            }

            let image = fit_image(&ready.frames[ready.current].image, ready.fitted, consts::IMAGE_ROWS, font);

            //REUSE THE PROTOCOL TYPE TO KEEP THE IMAGE ID
            ready.protocol = ready.protocol.take().map(|protocol|
            {
                let background = protocol.background_color();

                StatefulProtocol::new(image, font, background, protocol.protocol_type_owned())
            });

            self.dirty = true;
        }
    }

    //BUILD THE PICTURES WITHIN REACH AND PUT THE REST DOWN
    pub fn load_visible(&mut self, width: u16, offset: u16, height: u16)
    {
        let font = self.picker.font_size();

        //THE TOP OF THE LOBBY'S HISTORY IS WITHIN REACH
        if offset < height.saturating_mul(consts::PRELOAD_SCREENS + 1) && self.channel.is_empty() && !self.history_pending
            && let Some(cursor) = self.history_cursor
        {
            self.history_pending = true;
            self.history_request = Some(cursor);
        }

        for placement in self.placements(width)
        {
            let visible = in_reach(&placement, offset, height);

            let Some(Entry::Image { hash, picture, .. }) = self.messages.get_mut(placement.entry)
                else { continue };

            match picture
            {
                //OUT OF THE CACHE, NOW THAT IT IS NEARLY IN VIEW
                Picture::Deferred if visible =>
                {
                    if let Some(hash) = *hash { self.image_loads.push(hash); }

                    *picture = Picture::Waiting;
                },

                Picture::Ready(ready) => match visible
                {
                    true => if ready.fitted != width || ready.protocol.is_none()
                    {
                        let image = fit_image(&ready.frames[ready.current].image, width, consts::IMAGE_ROWS, font);

                        //REUSE THE PROTOCOL TYPE TO KEEP THE IMAGE ID
                        ready.protocol = Some(match ready.unloaded.take()
                        {
                            Some(Unloaded { kind, background }) =>
                                StatefulProtocol::new(image, font, background, kind),

                            None => self.picker.new_resize_protocol(image),
                        });

                        ready.fitted = width;
                    },

                    //OFF SCREEN KEEPS ONLY THE FRAMES
                    false => if let Some(protocol) = ready.protocol.take()
                    {
                        ready.unloaded = Some(Unloaded
                        {
                            background: protocol.background_color(),
                            kind: protocol.protocol_type_owned(),
                        });
                    },
                },

                _ => {},
            }
        }
    }

    //THE QUERY WANTS STDIO TO ITSELF
    pub fn init_picker(&mut self)
    {
        let options = QueryStdioOptions { terminal_background_color_osc: true, ..Default::default() };

        let Ok(picker) = Picker::from_query_stdio_with_options(options) else { return };

        //THE STRIPE FOLLOWS THE TERMINAL'S BACKGROUND
        let background = picker.capabilities().iter().find_map(|capability| match capability
        {
            Capability::Background(r, g, b) => Some((*r, *g, *b)),
            _ => None,
        });

        self.stripe_bg = theme::stripe(background);
        self.picker = picker;
        self.generation += 1;
    }

    fn push_entry(&mut self, entry: Entry)
    {
        self.messages.push_back(entry);

        while self.messages.len() > consts::HISTORY_LIMIT
        {
            if self.messages.pop_front().is_some_and(|entry| entry.striped()) { self.stripe = !self.stripe; }

            //THE REPLAYED ENTRIES MOVE UP, OR GO
            if self.channel.is_empty()
            {
                self.history_anchor = self.history_anchor.and_then(|anchor| anchor.checked_sub(1));
                if self.history_anchor.is_none() { self.history_cursor = None; }
            }
        }

        self.generation += 1;
        self.dirty = true;

        if self.scroll.is_some() { self.unread += 1; }
    }

    pub fn push_text(&mut self, text: impl Into<String>)
    {
        self.push(Line::from(Span::raw(text.into())));
    }

    pub fn push_styled(&mut self, text: impl Into<String>, style: Style)
    {
        self.push(Line::from(Span::styled(text.into(), style)));
    }

    //CLEAR THE PANE BEING LOOKED AT
    pub fn clear_messages(&mut self)
    {
        self.messages.clear();
        self.wrapped = None;
        self.selection = None;
        self.scroll = None;
        self.unread = 0;

        //NOTHING LEFT TO PAGE ABOVE
        if self.channel.is_empty()
        {
            self.history_anchor = None;
            self.history_cursor = None;
        }

        self.generation += 1;
        self.dirty = true;
    }

    //PARK THE OLD PANE, PUT BACK THE NEW ONE
    pub fn switch_channel(&mut self, channel: String)
    {
        if channel == self.channel { return; }

        let mut parked = mem::take(&mut self.messages);

        //AN ANSWER WOULD LAND IN THE WRONG PANE
        for entry in parked.iter_mut()
        {
            if let Entry::Image { picture: picture @ Picture::Waiting, .. } = entry { *picture = Picture::Deferred; }
        }

        if !parked.is_empty() { self.panes.insert(mem::take(&mut self.channel), parked); }

        self.messages = self.panes.remove(&channel).unwrap_or_default();
        self.channel = channel;

        self.typing_users.clear();

        self.wrapped = None;
        self.selection = None;
        self.scroll = None;
        self.unread = 0;

        self.generation += 1;
        self.dirty = true;
    }

    //US FIRST, THE REST BY ID
    pub fn sort_online(&mut self)
    {
        let me = self.username.clone();

        self.online.sort_by_key(|user| (user.username != me, user.id));
    }

    //DROP THE SCROLLBACK OF AN EMPTY CHANNEL
    pub fn prune_panes(&mut self)
    {
        self.panes.retain(|channel, _| channel.is_empty() || self.channels.contains(channel));
    }

    //RE-READ THE STYLING AND REPAINT THE HISTORY
    pub fn reload_theme(&mut self)
    {
        self.theme.reload();

        //THE WRAP CACHE HOLDS RENDERED LINES
        self.generation += 1;
        self.wrapped = None;
        self.dirty = true;
    }

    //PUT A REPLAYED ANSWER IN THE FIELD FOR THE TICK TO SEND
    pub fn answer_step(&mut self, stage: Stage)
    {
        let Some(answer) = self.reconnect.answer(stage) else { return };

        if let Some(login) = self.login.as_mut() { login.input.insert_str(&answer); }

        self.reconnect.submit = true;
    }

    //THROW THE SESSION AWAY, BRING BACK THE BOX
    pub fn disconnected(&mut self, reason: impl Into<String>)
    {
        //CARRY THE DIAL COUNTER OVER
        let attempt = self.login.as_ref().map_or(0, Login::attempt);
        //A LOGOUT IS NOT A NET FAIL - IT ASKED FOR THIS
        if self.logging_out { self.reconnect.forget(); }

        //DIAL BACK UNLESS WE HAVE RUN OUT OF TRIES - THE REASON IS WHAT IS LEFT ON SCREEN IF WE HAVE
        let retrying = self.reconnect.arm();

        self.login = Some(Login::again(&self.address, attempt, reason.into()));
        self.drop_stream = true; //THE WRITE HALF BELONGS TO THE EVENT LOOP

        //A NEW SESSION STARTS BLANK
        self.clear_messages();
        self.panes.clear();
        self.channel.clear();

        self.input = InputBuffer::new();
        self.palette.dismiss();
        self.settings.close();
        self.tofu = None;
        self.account = None;

        self.username.clear();
        self.role = Role::default(); //THE NEXT SERVER GRANTS ITS OWN
        self.server_name.clear();
        self.online.clear();
        self.offline.clear();
        self.offline_listed = false;
        self.devices.clear();
        self.channels.clear();
        self.voice.clear();
        self.voice_roster.clear();
        self.voice_activity.clear();
        self.voice_enabled = false;

        self.typing_users.clear();
        self.typing = false;
        self.typing_sent = None;

        self.list_requested = false;
        #[cfg(feature = "client_screen")]
        { self.screens_requested = false; }
        self.image_requests.clear();
        self.image_loads.clear();
        self.image_fetching.clear();
        self.history_anchor = None;
        self.history_cursor = None;
        self.history_pending = false;
        self.history_request = None;
        self.logging_out = false; //THE NEXT DROP IS THE NEXT SESSION'S TO EXPLAIN
        self.disconnect_reason = None;

        reset_session();

        //THE BOX IS BUSY UNTIL THE WAIT IS UP
        if retrying && let Some(login) = self.login.as_mut() { login.busy = true; }

        self.dirty = true;
    }

    //SCROLLING
    pub fn scroll_up(&mut self, amount: u16, viewport: u16)
    {
        let total = self.wrapped_len();
        let max_offset = total.saturating_sub(viewport);
        let current = self.scroll.unwrap_or(max_offset);

        self.scroll = Some(current.saturating_sub(amount));
        self.dirty = true;
    }

    pub fn scroll_down(&mut self, amount: u16, viewport: u16)
    {
        let total = self.wrapped_len();
        let max_offset = total.saturating_sub(viewport);

        if let Some(current) = self.scroll
        {
            let next = current.saturating_add(amount);

            if next >= max_offset { self.stick_to_bottom(); } else { self.scroll = Some(next); }
        }

        self.dirty = true;
    }

    pub fn stick_to_bottom(&mut self)
    {
        self.scroll = None;
        self.unread = 0;
        self.dirty = true;
    }

    //THE BACKGROUND OF A WRAPPED ROW, IF ANY
    pub fn tint(&self, row: u16) -> Option<Style>
    {
        self.wrapped.as_ref().and_then(|wrapped| wrapped.5.get(row as usize).copied().flatten())
    }

    //RECOMPUTE THE PALETTE FROM THE INPUT
    pub fn refresh_palette(&mut self)
    {
        let mut users = self.online.iter().map(|user| user.username.clone())
            .chain(self.offline.keys().cloned())
            .filter(|user| *user != self.username)
            .collect::<Vec<String>>();

        users.sort_unstable_by_key(|user| user.to_lowercase());
        users.dedup();

        self.palette.update(&self.input.text(), self.role, &users);
    }

    //WRAPPED VIEW (CACHED PER WIDTH + GENERATION)
    pub fn wrapped_lines(&mut self, width: u16) -> &[Line<'static>]
    {
        self.rewrap(width);

        &self.wrapped.as_ref().unwrap().2
    }

    //WHICH IMAGE'S CAPTION IS UNDER THE POINTER
    pub fn image_at(&mut self, column: u16, row: u16) -> Option<usize>
    {
        let pane = self.pane;

        if column < pane.x || column >= pane.x + pane.width { return None; }
        if row < pane.y || row >= pane.y + pane.height { return None; }

        let row = self.pane_offset + (row - pane.y);

        self.placements(pane.width).into_iter()
            .find(|placement| row >= placement.caption && row < placement.row)
            .map(|placement| placement.entry)
    }

    //AND WHICH URL IS, IF THE CELL IS ON ONE
    pub fn link_at(&mut self, column: u16, row: u16) -> Option<String>
    {
        let pane = self.pane;

        if column < pane.x || column >= pane.x + pane.width { return None; }
        if row < pane.y || row >= pane.y + pane.height { return None; }

        let (row, column) = self.pane_cell(column, row);
        let line = self.wrapped_lines(pane.width).get(row as usize)?;

        url(&word_at(line, column as usize)?)
    }

    //SELECTION
    //A PRESS STARTS ONE; A DRAG MAKES IT A SELECTION
    pub fn selection_start(&mut self, column: u16, row: u16) -> bool
    {
        let pane = self.pane;

        if column < pane.x || column >= pane.x + pane.width { return false; }
        if row < pane.y || row >= pane.y + pane.height { return false; }

        let cell = self.pane_cell(column, row);

        self.selection = Some(Selection { anchor: cell, cursor: cell, dragged: false });
        self.dirty = true;

        true
    }

    //A DRAG PAST AN EDGE SCROLLS THE PANE
    pub fn selection_extend(&mut self, column: u16, row: u16)
    {
        let pane = self.pane;

        if self.selection.is_none() || pane.height == 0 { return; }

        //NAME THE ROW THE SCROLL IS ABOUT TO REVEAL
        let cell = match row
        {
            _ if row < pane.y =>
            {
                self.scroll_up(1, pane.height);

                (self.pane_offset.saturating_sub(1), self.pane_cell(column, row).1)
            },

            _ if row >= pane.y + pane.height =>
            {
                self.scroll_down(1, pane.height);

                (self.pane_offset + pane.height, self.pane_cell(column, row).1)
            },

            _ => self.pane_cell(column, row),
        };

        if let Some(selection) = self.selection.as_mut()
        {
            selection.cursor = cell;
            selection.dragged = true;
        }

        self.dirty = true;
    }

    //TOAST
    //A TOAST IN THE PANE'S BOTTOM BORDER
    pub fn notify(&mut self, text: impl Into<String>)
    {
        self.notice = Some((text.into(), Instant::now()));
        self.dirty = true;
    }

    pub fn notice(&self) -> Option<&str>
    {
        self.notice.as_ref()
            .filter(|(_, shown)| shown.elapsed() < consts::NOTICE_DURATION)
            .map(|(text, _)| text.as_str())
    }

    //TAKE THE BOXES OF THIS FRAME, HANDING BACK THE LAST FRAME'S
    pub fn overlays_drawn(&mut self, overlays: &[Rect]) -> Vec<Rect>
    {
        let previous = std::mem::take(&mut self.overlays);

        self.overlays.extend_from_slice(overlays);

        previous
    }

    //WHETHER THE LAST FRAME SENT THIS ROW
    pub fn picture_row_sent(&self, y: u16, symbol: &str) -> bool
    {
        self.picture_rows.iter().any(|(row, sent)| *row == y && sent == symbol)
    }

    //TAKE THIS FRAME'S PICTURE ROWS, HANDING BACK THE ONES THAT CHANGED
    pub fn picture_rows_drawn(&mut self, rows: Vec<(u16, String)>) -> Vec<u16>
    {
        let changed = rows.iter()
            .filter(|row| !self.picture_rows.contains(row))
            .map(|(y, _)| *y).collect();

        self.picture_rows = rows;

        changed
    }

    //DROP THE TOAST ONCE IT IS OLD
    pub fn expire_notice(&mut self)
    {
        if self.notice.is_some() && self.notice().is_none()
        {
            self.notice = None;
            self.dirty = true;
        }
    }

    //TYPING
    //THE LINE CHANGED - WE ARE WRITING A MESSAGE
    pub fn typed(&mut self)
    {
        let text = self.input.text();
        let text = text.trim_start();

        //AN EMPTY LINE OR A COMMAND IS NOT A MESSAGE
        if text.is_empty() || text.starts_with('/')
        {
            self.typing = false;
            self.typing_sent = None;

            return;
        }

        self.typing = config::read_config::<bool>("typing_indicator");
    }

    //WHETHER THE TICK OWES THE SERVER A TypingRequest
    pub fn take_typing(&mut self) -> bool
    {
        if !mem::take(&mut self.typing) { return false; }

        //ONE PER TYPING_INTERVAL, WHATEVER WAS TYPED
        if self.typing_sent.is_some_and(|sent| sent.elapsed() < crate::consts::TYPING_INTERVAL) { return false; }

        self.typing_sent = Some(Instant::now());

        true
    }

    //SOMEBODY IN OUR CHANNEL IS WRITING
    pub fn set_typing(&mut self, username: String)
    {
        if !config::read_config::<bool>("typing_indicator") { return; }

        //A RESTATEMENT CHANGES NOTHING ON SCREEN
        if self.typing_users.insert(username, Instant::now()).is_none() { self.dirty = true; }
    }

    //A MESSAGE IS THE PROOF THEY STOPPED
    pub fn stopped_typing(&mut self, username: &str)
    {
        if self.typing_users.remove(username).is_some() { self.dirty = true; }
    }

    //DROP WHOEVER HAS NOT RESTATED IT
    pub fn expire_typing(&mut self)
    {
        let before = self.typing_users.len();

        self.typing_users.retain(|_, seen| seen.elapsed() < crate::consts::TYPING_TIMEOUT);

        if self.typing_users.len() != before { self.dirty = true; }
    }

    //WHAT THE PANE'S BORDER SAYS ABOUT IT
    pub fn typing_line(&self) -> Option<String>
    {
        let names: Vec<&str> = self.typing_users.keys().map(String::as_str).collect();

        match names.as_slice()
        {
            [] => None,
            [one] => Some(format!("{one} is typing…")),
            [one, two] => Some(format!("{one} and {two} are typing…")),
            _ => Some(format!("{} people are typing…", names.len())),
        }
    }

    pub fn clear_selection(&mut self)
    {
        if self.selection.take().is_some() { self.dirty = true; }
    }

    //WHICH COLUMNS OF A ROW ARE SELECTED
    pub fn selection_columns(&self, row: u16) -> Option<(u16, u16)>
    {
        let selection = self.selection?;

        if !selection.dragged { return None; }

        let (start, end) = selection.ordered();

        if row < start.0 || row > end.0 { return None; }

        let last = self.pane.width.saturating_sub(1);

        let first = if row == start.0 { start.1 } else { 0 };
        let final_column = if row == end.0 { end.1 } else { last };

        (first <= final_column).then_some((first, final_column))
    }

    //THE SELECTED TEXT
    pub fn selection_text(&mut self) -> Option<String>
    {
        let selection = self.selection?;

        if !selection.dragged { return None; }

        let width = self.pane.width;
        let (start, end) = selection.ordered();

        self.rewrap(width);

        let lines = &self.wrapped.as_ref().unwrap().2;
        let last = width.saturating_sub(1);

        let mut out: Vec<String> = Vec::new();

        for row in start.0..=end.0
        {
            let Some(line) = lines.get(row as usize) else { break };

            let first = if row == start.0 { start.1 } else { 0 };
            let final_column = if row == end.0 { end.1 } else { last };

            if first > final_column { continue; }

            out.push(slice_cells(line, first as usize, final_column as usize).trim_end().to_owned());
        }

        let text = out.join("\n");

        (!text.trim().is_empty()).then_some(text)
    }

    //A TERMINAL CELL AS A WRAPPED-VIEW PLACE
    fn pane_cell(&self, column: u16, row: u16) -> (u16, u16)
    {
        let pane = self.pane;

        let column = column.clamp(pane.x, pane.x + pane.width.saturating_sub(1)) - pane.x;
        let row = row.clamp(pane.y, pane.y + pane.height.saturating_sub(1)) - pane.y;

        (self.pane_offset + row, column)
    }

    //WHERE THE PICTURES SIT IN THE WRAPPED VIEW
    pub fn placements(&mut self, width: u16) -> Vec<Placement>
    {
        self.rewrap(width);

        self.wrapped.as_ref().unwrap().3.clone()
    }

    fn rewrap(&mut self, width: u16)
    {
        let stale = match &self.wrapped
        {
            Some((w, g, ..)) => *w != width || *g != self.generation,
            None => true,
        };

        if !stale { return; }

        let font = self.picker.font_size();

        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut placements: Vec<Placement> = Vec::new();
        let mut starts: Vec<u16> = Vec::with_capacity(self.messages.len());
        let mut tints: Vec<Option<Style>> = Vec::new();
        let mut stripe = self.stripe;

        //WHERE EACH MESSAGE SITS, FOR THE REPLIES
        let index: HashMap<u64, usize> = self.messages.iter().enumerate()
            .filter_map(|(entry, message)| Some((message.message_id()?, entry)))
            .collect();

        for entry in 0..self.messages.len()
        {
            let row = lines.len() as u16;
            starts.push(row);

            let target = self.messages[entry].reply().and_then(|id| index.get(&id)).map(|&target| &self.messages[target]);

            lines.extend(self.theme.render(&self.messages[entry], width, target, &self.username));

            //A MESSAGE FROM SOMEBODY ELSE NAMING US, OR ANSWERING US
            let mentioned = match &self.messages[entry]
            {
                Entry::Message { username, text, .. } | Entry::History { username, text, .. } |
                Entry::Private { sent: false, username, text, .. } =>
                    *username != self.username && (palette::mentions(text, &self.username)
                        || target.is_some_and(|target| target.username() == Some(self.username.as_str()))),

                _ => false,
            };

            //EVERY OTHER MESSAGE GETS A STRIPE
            let striped = self.theme.message_stripes && self.messages[entry].striped();
            if striped { stripe = !stripe; }

            let tint = match (mentioned, striped && stripe)
            {
                (true, _) => Some(theme::MENTION),
                (false, true) => Some(Style::new().bg(self.stripe_bg)),
                (false, false) => None,
            };

            tints.resize(lines.len(), None);
            tints[row as usize..].fill(tint);

            //AN IMAGE RESERVES ITS ROWS AS BLANK LINES
            if let Entry::Image { picture, .. } = &mut self.messages[entry]
            {
                let caption = row;
                let row = lines.len() as u16;

                //RESERVE THE ROWS HERE, BUILD THE PICTURE ONLY WHEN IT IS ON SCREEN
                let height = match picture
                {
                    Picture::Ready(ready) =>
                    {
                        let frame = &ready.frames[ready.current].image;
                        let (_, height) = fit_size(frame.width(), frame.height(), width, consts::IMAGE_ROWS, font);

                        ready.rows = (height.div_ceil(font.height as u32) as u16).clamp(1, consts::IMAGE_ROWS);
                        ready.rows
                    },

                    //A CAPTION WITHOUT A PICTURE RESERVES NOTHING
                    _ => 0,
                };

                placements.push(Placement { entry, caption, row, height });
                lines.extend(iter::repeat_n(Line::default(), height as usize));
                tints.resize(lines.len(), tint);
            }
        }

        tints.resize(lines.len(), None);

        self.wrapped = Some((width, self.generation, lines, placements, starts, tints));
    }

    fn wrapped_len(&self) -> u16
    {
        self.wrapped.as_ref().map(|(_, _, lines, ..)| lines.len() as u16).unwrap_or(0)
    }
}

//FUNCTIONS
//HOW FAR ALONG, IN WHOLE PERCENT
pub fn percent(done: u64, total: u64) -> u64
{
    match total
    {
        0 => 100,
        total => (done.min(total) * 100) / total,
    }
}

//THE SIZE A PICTURE IS DRAWN AT, NEVER LARGER THAN IT IS
//ON SCREEN OR WITHIN PRELOAD_SCREENS OF IT
fn in_reach(placement: &Placement, offset: u16, height: u16) -> bool
{
    let margin = height.saturating_mul(consts::PRELOAD_SCREENS);

    placement.caption < offset.saturating_add(height).saturating_add(margin)
        && placement.row + placement.height > offset.saturating_sub(margin)
}

fn fit_size(width: u32, height: u32, pane: u16, rows: u16, font: FontSize) -> (u32, u32)
{
    let available_width = pane.max(1) as u32 * font.width as u32;
    let available_height = rows as u32 * font.height as u32;

    if width <= available_width && height <= available_height { return (width, height); }

    let ratio = f64::min(available_width as f64 / width as f64, available_height as f64 / height as f64);

    (((width as f64 * ratio).round() as u32).max(1), ((height as f64 * ratio).round() as u32).max(1))
}

//THE CELLS A PICTURE CLAIMS AT THAT SIZE
pub fn picture_cells(image: &DynamicImage, pane: u16, rows: u16, font: FontSize) -> (u16, u16)
{
    let (width, height) = fit_size(image.width(), image.height(), pane, rows, font);

    ((width.div_ceil(font.width as u32) as u16).max(1).min(pane),
        (height.div_ceil(font.height as u32) as u16).clamp(1, rows))
}

//SHRINK A PICTURE INTO THE PANE, NEVER GROW IT
fn fit_image(image: &DynamicImage, width: u16, rows: u16, font: FontSize) -> DynamicImage
{
    let (fit_width, fit_height) = fit_size(image.width(), image.height(), width, rows, font);

    match (fit_width, fit_height) == (image.width(), image.height())
    {
        true => image.clone(),
        false => image.resize_exact(fit_width, fit_height, FilterType::Triangle),
    }
}

//ONE WRAPPED LINE BETWEEN TWO CELL COLUMNS
fn word_at(line: &Line<'static>, column: usize) -> Option<String> //THE WHITESPACE-DELIMITED WORD OVER A CELL
{
    let mut word = String::new();
    let mut start = 0usize;
    let mut cell = 0usize;

    for c in line.spans.iter().flat_map(|span| span.content.chars())
    {
        let w = c.width().unwrap_or(0).max(1);

        match c.is_whitespace()
        {
            true =>
            {
                if (start..cell).contains(&column) { return Some(word); }

                word.clear();
                start = cell + w;
            },

            false => word.push(c),
        }

        cell += w;
    }

    (start..cell).contains(&column).then_some(word)
}

//A WEB LINK, WITHOUT WHAT PUNCTUATION IS ONLY LEANING ON IT
fn url(word: &str) -> Option<String>
{
    let mut word = word.trim_start_matches(['(', '[', '<']);

    while word.ends_with([')', ']', '>', '.', ',', '!', '?', ';', ':'])
    {
        //A CLOSING BRACKET THE LINK OPENED ITSELF IS PART OF IT
        if word.ends_with(')') && word.matches('(').count() >= word.matches(')').count() { break; }

        word = &word[..word.len() - 1];
    }

    //THE SCHEME RULE IS THE LIBRARY'S, THE LENGTH IS THIS PANE'S
    (misc::is_web_url(word) && word.len() <= consts::MAX_URL).then(|| word.to_owned())
}

fn slice_cells(line: &Line<'static>, from: usize, to: usize) -> String
{
    let mut out = String::new();
    let mut column = 0usize;

    for span in &line.spans
    {
        for c in span.content.chars()
        {
            let w = c.width().unwrap_or(0).max(1);

            if column + w > from && column <= to { out.push(c); }

            column += w;

            if column > to { return out; }
        }
    }

    out
}

pub fn wrap_line(line: &Line<'static>, width: u16) -> Vec<Line<'static>> //WORD-WRAP ONE LOGICAL LINE, KEEPING SPAN STYLES
{
    let width = width.max(1) as usize;

    let mut out: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut column = 0usize;

    for span in &line.spans
    {
        let style = span.style;

        for word in split_words(span.content.as_ref())
        {
            let word_width = text_width(word);

            //BREAK BEFORE A WORD THAT NO LONGER FITS
            if column + word_width > width && column > 0
            {
                out.push(Line::from(mem::take(&mut current)));
                column = 0;

                if word.chars().all(char::is_whitespace) { continue; } //DROP THE SPACE THAT CAUSED THE BREAK
            }

            if word_width > width //A SINGLE WORD LONGER THAN THE PANE - HARD SPLIT IT
            {
                let mut chunk = String::new();

                for c in word.chars()
                {
                    let w = c.width().unwrap_or(0);

                    if column + w > width && column > 0
                    {
                        current.push(Span::styled(mem::take(&mut chunk), style));
                        out.push(Line::from(mem::take(&mut current)));
                        column = 0;
                    }

                    chunk.push(c);
                    column += w;
                }

                if !chunk.is_empty() { current.push(Span::styled(chunk, style)); }
            } else
            {
                current.push(Span::styled(word.to_owned(), style));
                column += word_width;
            }
        }
    }

    out.push(Line::from(current));
    out
}

fn split_words(text: &str) -> Vec<&str> //SPLIT INTO RUNS OF WHITESPACE AND WORDS
{
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut space: Option<bool> = None;

    for (i, c) in text.char_indices()
    {
        let is_space = c.is_whitespace();

        match space
        {
            Some(prev) if prev != is_space =>
            {
                out.push(&text[start..i]);
                start = i;
            },

            _ => {}
        }

        space = Some(is_space);
    }

    if start < text.len() { out.push(&text[start..]); }

    out
}

fn text_width(text: &str) -> usize
{
    text.chars().map(|c| c.width().unwrap_or(0)).sum()
}

//SESSION STATE THAT LIVES OUTSIDE App
fn reset_session()
{
    options::set_seq(0);
    options::set_server_seq(0);
    options::set_login_state(LoginState::None);
    options::set_sending_messages(false);
    options::set_asking_password(false);
    options::set_channel(String::new());
    options::set_server_username("");

    //A HALF-FINISHED UPLOAD IS GONE WITH THE SOCKET
    client::ACTIVE_UPLOADS.lock().unwrap().clear();

    #[cfg(feature = "client_voice")]
    voice_options::set_use_voice(false);

    #[cfg(feature = "client_screen")]
    {
        screen_options::set_use_screen(false);
        screen_options::set_attach_screen(false);
        screen_options::set_monitor(None);
    }
}
