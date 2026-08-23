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

use ratatui::text::{ Line, Span };

use unicode_width::UnicodeWidthChar;

//STRUCTS
pub struct InputBuffer //MULTI-LINE INPUT BUFFER
{
    chars: Vec<char>,
    cursor: usize,
    history: Vec<String>,
    history_pos: usize,
    stash: Option<String>, //IN-PROGRESS LINE PARKED WHILE PAGING HISTORY
}

//IMPLEMENTATIONS
impl Default for InputBuffer
{
    fn default() -> Self { Self::new() }
}

impl InputBuffer
{
    pub fn new() -> Self
    {
        Self { chars: Vec::new(), cursor: 0, history: Vec::new(), history_pos: 0, stash: None }
    }

    //QUERIES
    pub fn is_empty(&self) -> bool { self.chars.is_empty() }
    pub fn text(&self) -> String { self.chars.iter().collect() }
    pub fn cursor(&self) -> usize { self.cursor }

    //EDITING
    pub fn insert(&mut self, c: char)
    {
        self.chars.insert(self.cursor, c);
        self.cursor += 1;
    }

    pub fn insert_str(&mut self, text: &str)
    {
        for c in text.chars() { self.insert(c); }
    }

    pub fn backspace(&mut self)
    {
        if self.cursor > 0
        {
            self.cursor -= 1;
            self.chars.remove(self.cursor);
        }
    }

    pub fn delete(&mut self)
    {
        if self.cursor < self.chars.len() { self.chars.remove(self.cursor); }
    }

    pub fn delete_word(&mut self) //CTRL+W
    {
        let start = self.word_start();
        self.chars.drain(start..self.cursor);
        self.cursor = start;
    }

    pub fn kill_to_start(&mut self) //CTRL+U
    {
        let start = self.line_start();
        self.chars.drain(start..self.cursor);
        self.cursor = start;
    }

    pub fn kill_to_end(&mut self) //CTRL+K
    {
        let end = self.line_end();
        self.chars.drain(self.cursor..end);
    }

    //MOTION
    pub fn left(&mut self) { self.cursor = self.cursor.saturating_sub(1); }
    pub fn right(&mut self) { if self.cursor < self.chars.len() { self.cursor += 1; } }
    pub fn home(&mut self) { self.cursor = self.line_start(); }
    pub fn end(&mut self) { self.cursor = self.line_end(); }

    pub fn word_left(&mut self) { self.cursor = self.word_start(); }

    pub fn word_right(&mut self)
    {
        let len = self.chars.len();
        let mut i = self.cursor;

        while i < len && self.chars[i].is_whitespace() { i += 1; }
        while i < len && !self.chars[i].is_whitespace() { i += 1; }

        self.cursor = i;
    }

    //HISTORY
    pub fn history_up(&mut self)
    {
        if self.history.is_empty() || self.history_pos == 0 { return; }

        //PARK THE LINE THE USER WAS TYPING
        if self.history_pos == self.history.len() { self.stash = Some(self.text()); }

        self.history_pos -= 1;
        self.set(&self.history[self.history_pos].clone());
    }

    pub fn history_down(&mut self)
    {
        if self.history_pos >= self.history.len() { return; }

        self.history_pos += 1;

        let new = if self.history_pos < self.history.len()
        {
            self.history[self.history_pos].clone()
        } else
        {
            self.stash.take().unwrap_or_default()
        };

        self.set(&new);
    }

    pub fn push_history(&mut self, input: &str)
    {
        if self.history.last().map(String::as_str) != Some(input)
        {
            self.history.push(input.to_owned());
        }

        self.history_pos = self.history.len();
        self.stash = None;
    }

    pub fn reset_history_position(&mut self)
    {
        self.history_pos = self.history.len();
        self.stash = None;
    }

    //LIFECYCLE
    pub fn clear(&mut self)
    {
        self.chars.clear();
        self.cursor = 0;
    }

    pub fn take(&mut self) -> String //COLLECT AND CLEAR
    {
        let read = self.text();

        self.clear();
        self.reset_history_position();

        read
    }

    //RENDERING
    //WRAPS THE BUFFER TO width AND RETURNS THE LINES PLUS THE (column, row) OF THE CURSOR
    pub fn render(&self, width: u16, mask: bool) -> (Vec<Line<'static>>, (u16, u16))
    {
        let width = width.max(1) as usize;

        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut current = String::new();
        let mut column = 0usize;
        let mut cursor_at = (0u16, 0u16);

        for (i, c) in self.chars.iter().enumerate()
        {
            if i == self.cursor { cursor_at = (column as u16, lines.len() as u16); }

            if *c == '\n'
            {
                lines.push(Line::from(Span::raw(std::mem::take(&mut current))));
                column = 0;
                continue;
            }

            let shown = if mask { '*' } else { *c };
            let w = shown.width().unwrap_or(0);

            //SOFT WRAP
            if column + w > width
            {
                lines.push(Line::from(Span::raw(std::mem::take(&mut current))));
                column = 0;

                if i == self.cursor { cursor_at = (0, lines.len() as u16); }
            }

            current.push(shown);
            column += w;
        }

        if self.cursor == self.chars.len() { cursor_at = (column as u16, lines.len() as u16); }

        lines.push(Line::from(Span::raw(current)));

        (lines, cursor_at)
    }

    //PRIVATE
    fn set(&mut self, text: &str)
    {
        self.chars = text.chars().collect();
        self.cursor = self.chars.len();
    }

    fn line_start(&self) -> usize
    {
        self.chars[..self.cursor].iter().rposition(|c| *c == '\n').map(|i| i + 1).unwrap_or(0)
    }

    fn line_end(&self) -> usize
    {
        self.chars[self.cursor..].iter().position(|c| *c == '\n')
            .map(|i| self.cursor + i).unwrap_or(self.chars.len())
    }

    fn word_start(&self) -> usize
    {
        let mut i = self.cursor;

        while i > 0 && self.chars[i - 1].is_whitespace() { i -= 1; }
        while i > 0 && !self.chars[i - 1].is_whitespace() { i -= 1; }

        i
    }
}
