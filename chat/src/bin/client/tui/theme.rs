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

use ratatui::
{
    text::{ Line, Span },
    backend::FromCrossterm,
    style::
    {
        Color,
        Modifier,
        Style,
    },
};

use crate::{ colors, config };

use super::state::{ Entry, Picture };

//STRUCTS
pub struct Theme //CACHED CONFIG-DRIVEN STYLING
{
    pub disable_colors: bool,
    pub disable_logo: bool,
    pub show_id: bool,
}

//IMPLEMENTATIONS
impl Theme
{
    pub fn load() -> Self
    {
        Self
        {
            disable_colors: config::read_config::<bool>("disable_colors"),
            disable_logo: config::read_config::<bool>("disable_logo"),
            show_id: config::read_config::<bool>("show_id"),
        }
    }

    pub fn reload(&mut self) //RE-READ AFTER A config::client_write
    {
        *self = Self::load();
    }

    //ONE HISTORY ENTRY, STYLED WITH THE CURRENT CONFIG - CHAT MESSAGES ARE RENDERED HERE, NOT WHERE THEY ARRIVE,
    //SO A show_id/disable_colors CHANGE REACHES THE MESSAGES THAT ARE ALREADY IN THE PANE
    pub fn render(&self, entry: &Entry) -> Line<'static>
    {
        match entry
        {
            Entry::Line(line) => line.clone(),

            Entry::Message { username, id, text, colors } =>
            {
                let id = if self.show_id { format!(" ({id})") } else { String::new() };

                Line::from(vec!
                [
                    self.colorize(username.clone(), colors.username_color),
                    Span::styled(id, DIM),
                    Span::raw(": "),
                    self.colorize(text.clone(), colors.message_color),
                ])
            },

            //THE SAME LINE WITHOUT THE ID COLUMN - THE HISTORY KEEPS NO IDS, AND show_id MUST NOT
            //INVENT ONE FOR IT
            Entry::History { username, text, colors } => Line::from(vec!
            [
                self.colorize(username.clone(), colors.username_color),
                Span::raw(": "),
                self.colorize(text.clone(), colors.message_color),
            ]),

            //ONLY THE CAPTION - THE PICTURE IS DRAWN OVER THE ROWS THE WRAP RESERVES UNDER IT. WHILE THERE
            //ARE NONE THE CAPTION SAYS WHY, AND OFFERS THE CLICK THAT FETCHES THE PICTURE
            Entry::Image { username, filename, picture, .. } =>
            {
                let mut spans = vec!
                [
                    Span::styled(username.clone(), ACCENT),
                    Span::styled(format!(" sent an image ({filename})"), DIM),
                ];

                match picture
                {
                    Picture::Absent => spans.push(Span::styled(" [ show ]", ACCENT)),
                    Picture::Waiting => spans.push(Span::styled(" [ loading... ]", DIM)),
                    Picture::Gone => spans.push(Span::styled(" [ unavailable ]", ERROR)),
                    Picture::Ready(..) => {},
                }

                Line::from(spans)
            },
        }
    }

    pub fn colorize(&self, text: String, color: Option<u8>) -> Span<'static> //COLORIZE text IF PASSED COLOR
    {
        match color.and_then(colors::u8_to_color)
        {
            Some(c) if !self.disable_colors => Span::styled(text, Style::new().fg(Color::from_crossterm(c))),
            _ => Span::raw(text),
        }
    }
}

//CONSTS
pub const TEXT: Style = Style::new().fg(Color::Rgb(0xEE, 0xD1, 0xD6));          //WARM OFF-WHITE - THE BASE FOREGROUND
pub const BORDER: Style = Style::new().fg(Color::Rgb(0xCA, 0xB4, 0xB7));        //MUTED ROSE GREY
pub const BORDER_ACTIVE: Style = Style::new().fg(Color::Rgb(0x9D, 0xCE, 0xFF)); //SKY BLUE
pub const TITLE: Style = Style::new().fg(Color::Rgb(0x9D, 0xCE, 0xFF)).add_modifier(Modifier::BOLD);
pub const DIM: Style = Style::new().fg(Color::Rgb(0xCA, 0xB4, 0xB7));
pub const ACCENT: Style = Style::new().fg(Color::Rgb(0x9D, 0xCE, 0xFF));
pub const NOTICE: Style = Style::new().fg(Color::Rgb(0xFF, 0xDD, 0xE2));        //PALE PINK
pub const ERROR: Style = Style::new().fg(Color::Rgb(0xF6, 0x46, 0xC6));         //HOT MAGENTA
pub const OK: Style = Style::new().fg(Color::Rgb(0xFF, 0xBB, 0xBA));            //SALMON
pub const SPEAKING: Style = Style::new().fg(Color::Rgb(0xFF, 0xBB, 0xBA)).add_modifier(Modifier::BOLD);

pub const LOGO_COLOR: Color = Color::Rgb(0x5C, 0x46, 0x4B);                     //DEEP ROSE - THE WATERMARK BEHIND EVERYTHING
pub const LOGO: Style = Style::new().fg(LOGO_COLOR);                            //ON A FREE CELL THE GLYPH ITSELF IS DRAWN...
pub const LOGO_UNDER: Style = Style::new().bg(LOGO_COLOR);                      //...UNDER TEXT ONLY THE BACKGROUND IS, SO THE SHAPE RUNS ON BEHIND IT

pub const SELECTED: Style = Style::new().bg(Color::Rgb(0x00, 0x5F, 0x5F));

pub const ARG_REQUIRED: Style = Style::new().fg(Color::Rgb(0xD7, 0xAF, 0x87));  //SOFT SAND
pub const ARG_OPTIONAL: Style = Style::new().fg(Color::Rgb(0xFF, 0xB4, 0xAB));  //FADED CORAL
pub const ARG_ACTIVE: Style = Style::new().fg(Color::Rgb(0xFF, 0xAF, 0x5F))     //WARM AMBER - THE PARAMETER BEING TYPED
    .add_modifier(Modifier::BOLD)
    .add_modifier(Modifier::UNDERLINED);
