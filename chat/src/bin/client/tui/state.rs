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

use std::collections::{ BTreeSet, VecDeque };

use ratatui::
{
    style::Style,
    text::{ Line, Span },
};

use unicode_width::UnicodeWidthChar;

use crate::network::
{
    codes::{ MessageColors, OnlineUser },
    client::VoiceUser,
};

use super::
{
    input::InputBuffer,
    palette::Palette,
    theme::Theme,
};

//CONSTS
pub const HISTORY_LIMIT: usize = 5000; //CAP THE MESSAGE PANE SO RE-WRAPPING EACH FRAME STAYS CHEAP

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
}

//STRUCTS
pub struct App
{
    //MESSAGE PANE
    pub messages: VecDeque<Entry>,
    pub scroll: Option<u16>, //None = STUCK TO THE BOTTOM
    pub unread: usize,       //MESSAGES ARRIVED WHILE SCROLLED AWAY

    //SIDEBAR
    pub username: String, //OUR OWN USERNAME (options::get_server_username IS THE SERVER'S NAME)
    pub online: Vec<OnlineUser>,
    pub channels: BTreeSet<String>, //NAMED CHANNELS THE SERVER CURRENTLY HOLDS
    pub voice: Vec<VoiceUser>,
    pub voice_enabled: bool,

    //CONNECTION (SHOWN IN THE MESSAGE PANE TITLE)
    pub address: String,     //AS THE USER TYPED IT - NO IMPLICIT PORT
    pub server_name: String, //THE SERVER'S OWN NAME, ONCE IT HAS INTRODUCED ITSELF
    pub login_hint: Option<String>, //USERNAME/PASSWORD RULES, SHOWN IN THE INPUT TITLE INSTEAD OF THE PANE

    //INPUT
    pub input: InputBuffer,
    pub palette: Palette,
    pub theme: Theme,

    //REQUEST BOOKKEEPING (A LIST/SCREENS RESPONSE IS ONLY ECHOED WHEN THE USER ASKED FOR IT)
    pub list_requested: bool,
    #[cfg(feature = "client_screen")]
    pub screens_requested: bool,
    pub refresh_online: bool, //THE LOOP SHOULD SEND A SILENT PacketCode::List

    //LIFECYCLE
    pub should_quit: bool,
    pub exit_code: i32,
    pub quit_message: Option<String>, //PRINTED ON THE NORMAL SCREEN AFTER TEARDOWN
    pub dirty: bool,

    //WRAP CACHE
    generation: u64,
    wrapped: Option<(u16, u64, Vec<Line<'static>>)>,
}

//IMPLEMENTATIONS
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
            scroll: None,
            unread: 0,
            username: String::new(),
            online: Vec::new(),
            channels: BTreeSet::new(),
            voice: Vec::new(),
            voice_enabled: false,
            address: String::new(),
            server_name: String::new(),
            login_hint: None,
            input: InputBuffer::new(),
            palette: Palette::new(),
            theme: Theme::load(),
            list_requested: false,
            #[cfg(feature = "client_screen")]
            screens_requested: false,
            refresh_online: false,
            should_quit: false,
            exit_code: 0,
            quit_message: None,
            dirty: true,
            generation: 0,
            wrapped: None,
        }
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

    //CLEARS THE MESSAGE HISTORY (E.G. ON CHANNEL SWITCH)
    pub fn clear_messages(&mut self)
    {
        self.messages.clear();
        self.wrapped = None;
        self.scroll = None;
        self.unread = 0;

        self.generation += 1;
        self.dirty = true;
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

    //DRAINS NEWLY PUSHED LINES AS PLAIN TEXT (USED BY THE PRE-TUI PHASE, WHICH HAS NO FRAME TO DRAW)
    pub fn drain_plain(&mut self) -> Vec<String>
    {
        let out = self.messages.iter().map(|entry| plain(&self.theme.render(entry))).collect();

        self.messages.clear();
        self.wrapped = None;

        out
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
        let stale = match &self.wrapped
        {
            Some((w, g, _)) => *w != width || *g != self.generation,
            None => true,
        };

        if stale
        {
            let theme = &self.theme;
            let lines = self.messages.iter().flat_map(|entry| wrap_line(&theme.render(entry), width)).collect();

            self.wrapped = Some((width, self.generation, lines));
        }

        &self.wrapped.as_ref().unwrap().2
    }

    fn wrapped_len(&self) -> u16
    {
        self.wrapped.as_ref().map(|(_, _, l)| l.len() as u16).unwrap_or(0)
    }
}

//FUNCTIONS
pub fn plain(line: &Line<'_>) -> String //STRIP STYLING (PRE-TUI OUTPUT)
{
    line.spans.iter().map(|s| s.content.as_ref()).collect()
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
                out.push(Line::from(std::mem::take(&mut current)));
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
                        current.push(Span::styled(std::mem::take(&mut chunk), style));
                        out.push(Line::from(std::mem::take(&mut current)));
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
