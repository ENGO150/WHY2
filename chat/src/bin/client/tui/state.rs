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
    time::{ Duration, Instant },
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
    style::Style,
    text::{ Line, Span },
};

use unicode_width::UnicodeWidthChar;

use image::DynamicImage;

use ratatui_image::
{
    FontSize,
    FilterType,
    picker::Picker,
    protocol::StatefulProtocol,
};

use crate::
{
    role::Role,
    options::{ self, LoginState },
    network::
    {
        codes::{ MessageColors, OnlineUser },
        client::{ self, VoiceUser },
    },
};

#[cfg(feature = "client_voice")]
use crate::network::voice::client::options as voice_options;

#[cfg(feature = "client_screen")]
use crate::network::screen::client::options as screen_options;

use super::
{
    input::InputBuffer,
    login::Login,
    palette::Palette,
    settings::Settings,
    tofu::Prompt,
    theme::Theme,
};

//CONSTS
pub const HISTORY_LIMIT: usize = 5000; //CAP THE MESSAGE PANE SO RE-WRAPPING EACH FRAME STAYS CHEAP
pub const IMAGE_ROWS: u16 = 20;        //TALLEST AN IMAGE MAY BE DRAWN - THE PANE IS A CHAT, NOT A VIEWER
pub const NOTICE_DURATION: Duration = Duration::from_secs(2); //HOW LONG THE PANE'S TOAST STAYS UP

//ENUMS
pub enum Entry //ONE ROW OF HISTORY
{
    Line(Line<'static>), //ALREADY STYLED - CLIENT OUTPUT, NOTICES, BLOCK COMMANDS

    //A CHAT MESSAGE KEEPS ITS PARTS, SO show_id/disable_colors CAN BE APPLIED TO IT AGAIN LATER
    Message
    {
        username: String,
        id: usize,
        text: String,
        colors: MessageColors,
    },

    //A REPLAYED MESSAGE FROM THE SERVER'S HISTORY - THE SAME THING WITHOUT AN ID, SINCE THE SESSION
    //THAT SAID IT IS GONE AND WHOEVER HOLDS THAT ID NOW IS SOMEBODY ELSE
    History
    {
        username: String,
        text: String,
        colors: MessageColors,
    },

    //A PICTURE SOMEBODY SENT. THE LINE IS ITS CAPTION - THE PICTURE IS DRAWN OVER THE ROWS RESERVED UNDER
    //IT, ONCE THERE IS ONE TO DRAW: A REPLAYED IMAGE ARRIVES AS A HASH AND IS ONLY FETCHED IF IT IS ASKED
    //FOR, WHICH IS WHAT KEEPS A LOGIN FROM PULLING EVERY PICTURE EVER POSTED
    Image
    {
        username: String,
        filename: String,
        hash: Option<[u8; 32]>, //WHAT TO ASK THE SERVER FOR - None WHEN IT ARRIVED WITH ITS PICTURE
        picture: Picture,
    },
}

//WHAT THERE IS TO DRAW UNDER A CAPTION - AND, WHILE THERE IS NOTHING, WHAT THE CAPTION OFFERS INSTEAD
pub enum Picture
{
    Absent,            //NOT ASKED FOR YET
    Waiting,           //ASKED FOR, NOT HERE YET
    Gone,              //THE SERVER DOES NOT HAVE IT ANY MORE
    Ready(Box<Fitted>),
}

//STRUCTS
pub struct Fitted //A PICTURE AT THE SIZE THE PANE DRAWS IT AT
{
    pub source: DynamicImage,               //THE PICTURE ITSELF, KEPT TO FIT AGAIN AT A NEW PANE WIDTH
    pub rows: u16,                          //ROWS IT RESERVES AT THAT WIDTH
    pub fitted: u16,                        //THE WIDTH `protocol` WAS FITTED TO
    pub protocol: Option<StatefulProtocol>, //None UNTIL THE FIRST WRAP KNOWS HOW WIDE THE PANE IS
}

#[derive(Clone, Copy)]
pub struct Placement //WHERE ONE IMAGE SITS IN THE WRAPPED VIEW
{
    pub entry: usize,  //WHICH App::messages ENTRY IT BELONGS TO
    pub caption: u16,  //FIRST ROW OF THE CAPTION, WHICH IS ALSO WHAT IS CLICKED TO FETCH THE PICTURE
    pub row: u16,      //FIRST RESERVED ROW (WHERE THE CAPTION ENDS)
    pub height: u16,   //RESERVED ROWS - 0 WHILE THERE IS NO PICTURE
}

//A DRAG IN THE MESSAGE PANE. BOTH ENDS ARE ROWS OF THE WRAPPED VIEW RATHER THAN TERMINAL ROWS, SO
//SCROLLING DURING (OR AFTER) A DRAG MOVES THE HIGHLIGHT WITH THE TEXT INSTEAD OF LEAVING IT BEHIND
#[derive(Clone, Copy)]
pub struct Selection
{
    pub anchor: (u16, u16), //(ROW IN THE WRAPPED VIEW, COLUMN INSIDE THE PANE)
    pub cursor: (u16, u16),
    pub dragged: bool,      //A DRAG EVER ARRIVED - UNTIL THEN THE PRESS IS STILL A PLAIN CLICK
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
    pub channels: BTreeSet<String>, //NAMED CHANNELS THE SERVER CURRENTLY HOLDS
    pub voice: Vec<VoiceUser>, //WHAT THE VOICE PANEL DRAWS - rebuild_voice MAKES IT OUT OF THE TWO BELOW
    pub voice_roster: BTreeMap<usize, String>, //WHO THE SERVER SAYS IS IN VOICE IN OUR CHANNEL (US EXCLUDED)
    pub voice_activity: Vec<VoiceUser>, //WHO WE ARE ACTUALLY HEARING - EMPTY WHILE WE ARE NOT IN VOICE
    pub voice_enabled: bool,

    //CONNECTION (SHOWN IN THE MESSAGE PANE TITLE)
    pub address: String,     //AS THE USER TYPED IT - NO IMPLICIT PORT
    pub server_name: String, //THE SERVER'S OWN NAME, ONCE IT HAS INTRODUCED ITSELF

    //INPUT
    pub input: InputBuffer,
    pub palette: Palette,
    pub settings: Settings, //SETTINGS OVERLAY (CLOSED UNLESS THE USER OPENED IT)
    pub login: Option<Login>, //CONNECT BOX - UP FROM THE FIRST FRAME UNTIL THE SERVER ACCEPTS US
    pub tofu: Option<Prompt>, //SERVER IDENTITY PROMPT - OUTRANKS EVERY OTHER OVERLAY WHILE IT IS UP
    pub theme: Theme,
    pub picker: Picker, //WHAT THE TERMINAL CAN DRAW, AND HOW BIG ITS CELLS ARE

    //WHERE THE MESSAGE PANE WAS LAST DRAWN, WHICH IS THE ONLY WAY A CLICK CAN BE TURNED INTO A LINE
    pub pane: Rect,
    pub pane_offset: u16,
    pub selection: Option<Selection>, //A DRAG-SELECTED RUN OF THE PANE, KEPT UNTIL THE NEXT PRESS

    //SOMETHING THAT HAPPENED RATHER THAN SOMETHING THAT WAS SAID, SO IT GOES IN THE CHROME AND EXPIRES
    pub notice: Option<(String, Instant)>,

    //REQUEST BOOKKEEPING (A LIST/SCREENS RESPONSE IS ONLY ECHOED WHEN THE USER ASKED FOR IT)
    pub list_requested: bool,
    #[cfg(feature = "client_screen")]
    pub screens_requested: bool,
    pub refresh_online: bool, //THE LOOP SHOULD SEND A SILENT PacketCode::List

    //LIFECYCLE
    pub leaving: bool,      //THE USER ASKED TO LEAVE, SO THE DISCONNECT THAT FOLLOWS ENDS THE CLIENT
    pub logging_out: bool,  //THE USER ASKED TO LOG OUT, SO THAT DISCONNECT IS NOT AN ERROR - IT IS THE POINT
    pub drop_stream: bool,  //THE LOOP OWNS THE WRITE HALF - IT HAS TO CLOSE IT AFTER A LOST SESSION
    pub should_quit: bool,
    pub exit_code: i32,
    pub quit_message: Option<String>, //PRINTED ON THE NORMAL SCREEN AFTER TEARDOWN
    pub dirty: bool,

    //WRAP CACHE
    generation: u64,
    wrapped: Option<(u16, u64, Vec<Line<'static>>, Vec<Placement>)>,
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
            theme: Theme::load(),
            picker: Picker::halfblocks(), //UNTIL init_picker HAS ASKED THE TERMINAL FOR SOMETHING BETTER
            pane: Rect::ZERO,
            pane_offset: 0,
            selection: None,
            notice: None,
            list_requested: false,
            #[cfg(feature = "client_screen")]
            screens_requested: false,
            refresh_online: false,
            leaving: false,
            logging_out: false,
            drop_stream: false,
            should_quit: false,
            exit_code: 0,
            quit_message: None,
            dirty: true,
            generation: 0,
            wrapped: None,
        }
    }

    //THE PANEL IS THE SERVER'S ROSTER, DRESSED WITH WHATEVER THE LOCAL VOICE SESSION KNOWS ABOUT IT.
    //THE TWO ARE SEPARATE BECAUSE ONLY ONE OF THEM ARRIVES WHILE WE ARE NOT IN VOICE OURSELVES
    pub fn rebuild_voice(&mut self)
    {
        let mut users: Vec<VoiceUser> = Vec::with_capacity(self.voice_roster.len() + 1);

        //US - THE ROSTER NEVER NAMES US, AND ONLY THE LOCAL SESSION KNOWS WE ARE SPEAKING
        if self.voice_enabled
        {
            users.push(match self.voice_activity.iter().find(|user| user.is_local)
            {
                Some(local) => VoiceUser { username: self.username.clone(), ..*local },

                //THE FIRST activity TICK IS UP TO 100 ms AWAY - DO NOT BLINK OUT OF OUR OWN PANEL UNTIL THEN
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

        //EVERYBODY ELSE, IN ID ORDER (BTreeMap). A ROSTER ENTRY WE HAVE NO STREAM FOR IS STILL IN VOICE -
        //IT IS US WHO CANNOT HEAR THEM, SO IT IS DRAWN WITHOUT A LATENCY RATHER THAN LEFT OUT
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

    //A CHAT MESSAGE IS STORED UNRENDERED - draw RE-APPLIES THE THEME TO IT ON EVERY WRAP
    pub fn push_message(&mut self, username: String, id: usize, text: String, colors: MessageColors)
    {
        self.push_entry(Entry::Message { username, id, text, colors });
    }

    //A REPLAYED MESSAGE IS STORED UNRENDERED FOR THE SAME REASON A LIVE ONE IS
    pub fn push_history(&mut self, username: String, text: String, colors: MessageColors)
    {
        self.push_entry(Entry::History { username, text, colors });
    }

    //A PICTURE THAT ARRIVED WITH ITS OWN BYTES - A LIVE ONE, OR A REPLAYED ONE THAT WAS ASKED FOR
    pub fn push_image(&mut self, username: String, filename: String, image: DynamicImage)
    {
        let picture = self.fit(image);

        self.push_entry(Entry::Image { username, filename, hash: None, picture });
    }

    //AND ONE THE HISTORY ONLY NAMED. NOTHING IS FETCHED UNTIL THE CAPTION IS CLICKED
    pub fn push_caption(&mut self, username: String, filename: String, hash: [u8; 32])
    {
        self.push_entry(Entry::Image { username, filename, hash: Some(hash), picture: Picture::Absent });
    }

    //A CLICKED CAPTION. THE HASH IT COMES BACK WITH IS WHAT THE CALLER ASKS THE SERVER FOR - None MEANS
    //THERE IS NOTHING TO ASK FOR (THE PICTURE IS ALREADY HERE, OR ALREADY ON ITS WAY)
    pub fn request_image(&mut self, entry: usize) -> Option<[u8; 32]>
    {
        let Some(Entry::Image { hash, picture, .. }) = self.messages.get_mut(entry) else { return None };

        if !matches!(picture, Picture::Absent | Picture::Gone) { return None; }

        *picture = Picture::Waiting;

        self.generation += 1;
        self.dirty = true;

        *hash
    }

    //THE ANSWER TO ONE OF THOSE. THE SAME PICTURE CAN BE IN THE PANE TWICE, SO IT FILLS THE OLDEST LINE
    //STILL WAITING FOR IT - THE SECOND ONE ASKED FOR ITSELF AND IS ANSWERED BY ITS OWN PACKET
    pub fn deliver_image(&mut self, hash: [u8; 32], image: Option<DynamicImage>)
    {
        let picture = match image
        {
            Some(image) => self.fit(image),
            None => Picture::Gone,
        };

        let waiting = self.messages.iter().position(|entry| matches!(entry,
            Entry::Image { hash: Some(h), picture: Picture::Waiting, .. } if *h == hash));

        let Some(entry) = waiting else { return };

        if let Some(Entry::Image { picture: slot, .. }) = self.messages.get_mut(entry) { *slot = picture; }

        self.generation += 1;
        self.dirty = true;
    }

    //IT IS NEVER DRAWN TALLER THAN IMAGE_ROWS, SO NOTHING ABOVE THAT IS WORTH KEEPING - AND THE PROTOCOL
    //HOLDS ON TO WHAT IT IS GIVEN, WHICH FOR A PHONE PHOTO IS TENS OF MEGABYTES DECODED
    fn fit(&self, image: DynamicImage) -> Picture
    {
        let font = self.picker.font_size();
        let limit = IMAGE_ROWS as u32 * font.height as u32;

        let source = match image.height() > limit
        {
            true => image.resize(image.width(), limit, FilterType::Triangle),
            false => image,
        };

        Picture::Ready(Box::new(Fitted { source, rows: 1, fitted: 0, protocol: None }))
    }

    //THE QUERY WANTS STDIO TO ITSELF: AFTER THE ALTERNATE SCREEN IS UP, BEFORE ANYTHING READS EVENTS.
    //A TERMINAL THAT DOES NOT ANSWER STILL GETS PICTURES - HALFBLOCKS ARE A FALLBACK, NOT A FAILURE
    pub fn init_picker(&mut self)
    {
        if let Ok(picker) = Picker::from_query_stdio() { self.picker = picker; }
    }

    fn push_entry(&mut self, entry: Entry)
    {
        self.messages.push_back(entry);

        while self.messages.len() > HISTORY_LIMIT { self.messages.pop_front(); }

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

    //CLEARS THE PANE BEING LOOKED AT (A CHANNEL SWITCH PARKS IT INSTEAD - SEE switch_channel)
    pub fn clear_messages(&mut self)
    {
        self.messages.clear();
        self.wrapped = None;
        self.selection = None;
        self.scroll = None;
        self.unread = 0;

        self.generation += 1;
        self.dirty = true;
    }

    //A CHANNEL SWITCH PARKS THE PANE WE ARE LEAVING INSTEAD OF THROWING IT AWAY, AND PUTS BACK THE ONE
    //WE ARE ENTERING - STEPPING OUT OF THE LOBBY AND BACK NO LONGER COSTS WHAT WAS SAID IN IT
    pub fn switch_channel(&mut self, channel: String)
    {
        if channel == self.channel { return; }

        let parked = mem::take(&mut self.messages);

        if !parked.is_empty() { self.panes.insert(mem::take(&mut self.channel), parked); }

        self.messages = self.panes.remove(&channel).unwrap_or_default();
        self.channel = channel;

        self.wrapped = None;
        self.selection = None;
        self.scroll = None;
        self.unread = 0;

        self.generation += 1;
        self.dirty = true;
    }

    //A CHANNEL EXISTS EXACTLY AS LONG AS SOMEBODY SITS IN IT, SO THE SCROLLBACK OF ONE NOBODY IS IN ANY
    //MORE IS NOT WORTH KEEPING. THE LOBBY IS NOT IN THE LIST AND ALWAYS EXISTS; THE PANE WE ARE READING
    //IS NOT IN THE MAP AT ALL
    pub fn prune_panes(&mut self)
    {
        self.panes.retain(|channel, _| channel.is_empty() || self.channels.contains(channel));
    }

    //RE-READS THE CONFIG-DRIVEN STYLING AND REPAINTS THE WHOLE HISTORY WITH IT
    pub fn reload_theme(&mut self)
    {
        self.theme.reload();

        //THE WRAP CACHE HOLDS RENDERED LINES, SO IT HAS TO GO WITH IT
        self.generation += 1;
        self.wrapped = None;
        self.dirty = true;
    }

    //THE SERVER CLOSED THE SOCKET ON US. THE SESSION IS OVER, THE CLIENT IS NOT: EVERYTHING THE SESSION
    //BUILT UP IS THROWN AWAY AND THE CONNECT BOX COMES BACK AT THE ADDRESS STEP, PREFILLED WITH THE ADDRESS
    //AND CARRYING THE REASON, SO THE NEXT TRY (HERE OR ELSEWHERE) IS ONE KEYSTROKE AWAY.
    pub fn disconnected(&mut self, reason: impl Into<String>)
    {
        //A DIAL CANCELLED BEFORE THIS SESSION MUST NOT LAND ON THE NEW PROMPT EITHER, SO THE COUNTER LIVES ON
        let attempt = self.login.as_ref().map_or(0, Login::attempt);

        self.login = Some(Login::again(&self.address, attempt, reason.into()));
        self.drop_stream = true; //THE WRITE HALF BELONGS TO THE EVENT LOOP - IT CLOSES IT ON THE NEXT PASS

        //A NEW SESSION STARTS BLANK - AND UNLIKE A CHANNEL SWITCH, NOTHING IS PARKED FOR LATER
        self.clear_messages();
        self.panes.clear();
        self.channel.clear();

        self.input = InputBuffer::new();
        self.palette.dismiss();
        self.settings.close();
        self.tofu = None;

        self.username.clear();
        self.role = Role::default(); //THE NEXT SERVER GRANTS ITS OWN
        self.server_name.clear();
        self.online.clear();
        self.channels.clear();
        self.voice.clear();
        self.voice_roster.clear();
        self.voice_activity.clear();
        self.voice_enabled = false;

        self.list_requested = false;
        #[cfg(feature = "client_screen")]
        { self.screens_requested = false; }
        self.refresh_online = false;
        self.logging_out = false; //THE NEXT DROP IS THE NEXT SESSION'S TO EXPLAIN

        reset_session();

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

    //WRAPPED VIEW (CACHED PER WIDTH + HISTORY GENERATION)
    pub fn wrapped_lines(&mut self, width: u16) -> &[Line<'static>]
    {
        self.rewrap(width);

        &self.wrapped.as_ref().unwrap().2
    }

    //WHERE THE PICTURES SIT IN THAT VIEW - draw NEEDS THE ROWS AND THE ENTRY BEHIND EACH OF THEM
    //WHICH IMAGE'S CAPTION IS UNDER THE POINTER. THE WHOLE CAPTION IS THE TARGET RATHER THAN THE PROMPT
    //IN IT - IT IS ONE LINE OF A CHAT PANE, AND MISSING IT BY A COLUMN WOULD BE THE COMMON CASE
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

    //SELECTION
    //A PRESS INSIDE THE PANE STARTS ONE. IT IS NOT A SELECTION YET - UNTIL A DRAG ARRIVES IT IS A CLICK,
    //WHICH IS WHAT KEEPS AN IMAGE CAPTION CLICKABLE
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

    //A DRAG PAST EITHER EDGE SCROLLS THE PANE INSTEAD OF STOPPING AT IT - THE ANCHOR IS A WRAPPED-VIEW
    //ROW, SO WHAT WAS ALREADY SELECTED STAYS SELECTED WHILE THE VIEW MOVES UNDER IT
    pub fn selection_extend(&mut self, column: u16, row: u16)
    {
        let pane = self.pane;

        if self.selection.is_none() || pane.height == 0 { return; }

        //THE ROW THE SCROLL IS ABOUT TO REVEAL IS THE ONE BEING DRAGGED ONTO, AND pane_offset ONLY
        //CATCHES UP AT THE NEXT DRAW - SO IT IS NAMED HERE RATHER THAN READ BACK A FRAME LATE
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
    //A LINE IN THE PANE'S BOTTOM BORDER FOR THINGS THE USER DID, NOT THINGS ANYBODY SAID - THE HISTORY IS
    //THE CONVERSATION AND NOTHING ELSE BELONGS IN IT
    pub fn notify(&mut self, text: impl Into<String>)
    {
        self.notice = Some((text.into(), Instant::now()));
        self.dirty = true;
    }

    pub fn notice(&self) -> Option<&str>
    {
        self.notice.as_ref()
            .filter(|(_, shown)| shown.elapsed() < NOTICE_DURATION)
            .map(|(text, _)| text.as_str())
    }

    //THE TOAST GOES AWAY ON ITS OWN, SO SOMETHING HAS TO NOTICE THAT NOTHING HAPPENED - THE REDRAW TICK
    //ASKS EVERY PASS, AND ONLY THE PASS IT EXPIRES ON COSTS A FRAME
    pub fn expire_notice(&mut self)
    {
        if self.notice.is_some() && self.notice().is_none()
        {
            self.notice = None;
            self.dirty = true;
        }
    }

    pub fn clear_selection(&mut self)
    {
        if self.selection.take().is_some() { self.dirty = true; }
    }

    //WHICH COLUMNS OF ONE WRAPPED ROW ARE SELECTED (INCLUSIVE), IF ANY - THIS IS WHAT draw PAINTS
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

    //THE SELECTED TEXT, ROW BY ROW. THE ROWS ARE THE WRAPPED ONES RATHER THAN THE MESSAGES BEHIND THEM,
    //SO WHAT IS COPIED IS EXACTLY WHAT IS HIGHLIGHTED
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

    //A TERMINAL CELL AS A PLACE IN THE WRAPPED VIEW. OUT-OF-PANE COORDINATES ARE CLAMPED RATHER THAN
    //REFUSED - A DRAG ROUTINELY LEAVES THE PANE AND STILL MEANS SOMETHING
    fn pane_cell(&self, column: u16, row: u16) -> (u16, u16)
    {
        let pane = self.pane;

        let column = column.clamp(pane.x, pane.x + pane.width.saturating_sub(1)) - pane.x;
        let row = row.clamp(pane.y, pane.y + pane.height.saturating_sub(1)) - pane.y;

        (self.pane_offset + row, column)
    }

    pub fn placements(&mut self, width: u16) -> Vec<Placement>
    {
        self.rewrap(width);

        self.wrapped.as_ref().unwrap().3.clone()
    }

    fn rewrap(&mut self, width: u16)
    {
        let stale = match &self.wrapped
        {
            Some((w, g, _, _)) => *w != width || *g != self.generation,
            None => true,
        };

        if !stale { return; }

        let font = self.picker.font_size();

        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut placements: Vec<Placement> = Vec::new();

        for entry in 0..self.messages.len()
        {
            let row = lines.len() as u16;

            lines.extend(wrap_line(&self.theme.render(&self.messages[entry]), width));

            //AN IMAGE RESERVES ITS ROWS AS BLANK LINES, SO THE SCROLL OFFSET STAYS EXACT AND THE PANE
            //STAYS A LIST OF LINES - THE PICTURE IS PAINTED OVER THEM AFTERWARDS
            if let Entry::Image { picture, .. } = &mut self.messages[entry]
            {
                let caption = row;
                let row = lines.len() as u16;

                //THE FIT HAPPENS HERE AND NOWHERE ELSE, SO THE PROTOCOL ALWAYS HOLDS THE PICTURE AT THE
                //SIZE IT IS DRAWN AT - WHICH IS WHAT LETS draw CROP IT INSTEAD OF SHRINKING IT
                let height = match picture
                {
                    Picture::Ready(ready) =>
                    {
                        if ready.fitted != width || ready.protocol.is_none()
                        {
                            let image = fit_image(&ready.source, width, font);

                            ready.rows = (image.height().div_ceil(font.height as u32) as u16).clamp(1, IMAGE_ROWS);
                            ready.protocol = Some(self.picker.new_resize_protocol(image));
                            ready.fitted = width;
                        }

                        ready.rows
                    },

                    //A CAPTION WITHOUT A PICTURE RESERVES NOTHING - IT IS ONE LINE OFFERING TO FETCH ONE
                    _ => 0,
                };

                placements.push(Placement { entry, caption, row, height });
                lines.extend(iter::repeat_n(Line::default(), height as usize));
            }
        }

        self.wrapped = Some((width, self.generation, lines, placements));
    }

    fn wrapped_len(&self) -> u16
    {
        self.wrapped.as_ref().map(|(_, _, lines, _)| lines.len() as u16).unwrap_or(0)
    }
}

//FUNCTIONS
//A PICTURE SHRUNK INTO THE PANE - NEVER GROWN INTO IT, SO A SMALL ONE KEEPS ITS OWN SIZE. WHAT COMES BACK
//IS EXACTLY WHAT THE TERMINAL DRAWS, SO THE ROWS IT RESERVES CANNOT DISAGREE WITH THE PICTURE IN THEM
fn fit_image(image: &DynamicImage, width: u16, font: FontSize) -> DynamicImage
{
    let available_width = width.max(1) as u32 * font.width as u32;
    let available_height = IMAGE_ROWS as u32 * font.height as u32;

    match image.width() > available_width || image.height() > available_height
    {
        true => image.resize(available_width, available_height, FilterType::Triangle),
        false => image.clone(),
    }
}

//THE TEXT OF ONE WRAPPED LINE BETWEEN TWO CELL COLUMNS, BOTH INCLUSIVE. COLUMNS ARE CELLS AND NOT
//CHARACTERS, SO A WIDE GLYPH IS TAKEN WHOLE THE MOMENT THE SELECTION TOUCHES EITHER HALF OF IT
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

fn split_words(text: &str) -> Vec<&str> //SPLIT INTO ALTERNATING RUNS OF WHITESPACE AND NON-WHITESPACE
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

//EVERY PIECE OF SESSION STATE THAT LIVES OUTSIDE App - THE NEXT HANDSHAKE HAS TO START FROM THE SAME
//PLACE THE FIRST ONE DID, AND ANY TASK STILL WATCHING THESE (THE VOICE SESSION, A SCREEN SHARE) HAS TO STOP
fn reset_session()
{
    options::set_seq(0);
    options::set_server_seq(0);
    options::set_login_state(LoginState::None);
    options::set_sending_messages(false);
    options::set_asking_password(false);
    options::set_channel(String::new());
    options::set_server_username("");

    //A HALF-FINISHED UPLOAD BELONGS TO THE SOCKET THAT IS GONE
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
