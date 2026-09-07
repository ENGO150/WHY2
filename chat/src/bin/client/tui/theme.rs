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

use super::
{
    markup,
    state::{ self, Entry, Picture },
};

//STRUCTS
pub struct Theme //CACHED CONFIG-DRIVEN STYLING
{
    pub disable_colors: bool,
    pub disable_logo: bool,
    pub show_id: bool,
    pub render_math: bool,
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
            render_math: config::read_config::<bool>("render_math"),
        }
    }

    pub fn reload(&mut self) //RE-READ AFTER A config::client_write
    {
        *self = Self::load();
    }

    //ONE HISTORY ENTRY, STYLED WITH THE CURRENT CONFIG - CHAT MESSAGES ARE RENDERED HERE, NOT WHERE THEY ARRIVE,
    //SO A show_id/disable_colors CHANGE REACHES THE MESSAGES THAT ARE ALREADY IN THE PANE.
    //IT COMES BACK WRAPPED RATHER THAN AS ONE LOGICAL LINE, BECAUSE MARKUP IS WHAT DECIDES HOW MANY ROWS
    //A MESSAGE TAKES: A FENCED BLOCK IS ROWS PADDED TO THE PANE, NOT TEXT TO BE WORD-WRAPPED AFTERWARDS
    pub fn render(&self, entry: &Entry, width: u16) -> Vec<Line<'static>>
    {
        match entry
        {
            Entry::Line(line) => state::wrap_line(line, width),

            Entry::Message { username, id, text, colors } =>
            {
                let id = if self.show_id { format!(" ({id})") } else { String::new() };

                let prefix = vec!
                [
                    self.colorize(username.clone(), colors.username_color),
                    Span::styled(id, DIM),
                    Span::raw(": "),
                ];

                markup::render(prefix, text, self.style(colors.message_color), width, self.render_math)
            },

            //THE SAME LINE WITHOUT THE ID COLUMN - THE HISTORY KEEPS NO IDS, AND show_id MUST NOT
            //INVENT ONE FOR IT
            Entry::History { username, text, colors } => markup::render(vec!
            [
                self.colorize(username.clone(), colors.username_color),
                Span::raw(": "),
            ], text, self.style(colors.message_color), width, self.render_math),

            Entry::Prefixed { prefix, text } =>
                markup::render(prefix.clone(), text, Style::new(), width, self.render_math),

            //ONLY THE CAPTION - THE PICTURE IS DRAWN OVER THE ROWS THE WRAP RESERVES UNDER IT. WHILE THERE
            //ARE NONE THE CAPTION SAYS WHY, AND OFFERS THE CLICK THAT FETCHES THE PICTURE
            Entry::Image { username, filename, username_color, picture, .. } =>
            {
                let mut spans = vec!
                [
                    //THE SENDER'S OWN COLOR WHERE THERE IS ONE, THE CHROME'S ACCENT WHERE THERE IS NOT
                    match username_color.filter(|_| !self.disable_colors).and_then(colors::u8_to_color)
                    {
                        Some(color) => Span::styled(username.clone(), Style::new().fg(Color::from_crossterm(color))),
                        None => Span::styled(username.clone(), ACCENT),
                    },
                    Span::styled(format!(" sent an image ({filename})"), DIM),
                ];

                match picture
                {
                    Picture::Absent => spans.push(Span::styled(" [ show ]", ACCENT)),
                    Picture::Waiting => spans.push(Span::styled(" [ loading... ]", DIM)),
                    Picture::Gone => spans.push(Span::styled(" [ unavailable ]", ERROR)),
                    Picture::Ready(..) => {},
                }

                state::wrap_line(&Line::from(spans), width)
            },
        }
    }

    pub fn colorize(&self, text: String, color: Option<u8>) -> Span<'static> //COLORIZE text IF PASSED COLOR
    {
        Span::styled(text, self.style(color))
    }

    pub fn style(&self, color: Option<u8>) -> Style //THE USER'S OWN COLOUR, WHERE THEY HAVE ONE AND IT IS WANTED
    {
        match color.and_then(colors::u8_to_color)
        {
            Some(c) if !self.disable_colors => Style::new().fg(Color::from_crossterm(c)),
            _ => Style::new(),
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

//CODE. THE BLOCK IS A BOX RATHER THAN HIGHLIGHTED WORDS - ITS ROWS ARE PADDED TO THE PANE, SO THE
//BACKGROUND IS WHAT SEPARATES IT FROM THE CONVERSATION AROUND IT
pub const CODE_BG: Color = Color::Rgb(0x2E, 0x24, 0x28);                        //DEEP ROSE-BROWN
pub const CODE: Style = Style::new().fg(Color::Rgb(0xFF, 0xBB, 0xBA)).bg(CODE_BG);        //INLINE `code`
pub const CODE_BLOCK: Style = Style::new().fg(Color::Rgb(0xEE, 0xD1, 0xD6)).bg(CODE_BG);
pub const CODE_BAR: Style = Style::new().fg(Color::Rgb(0x9D, 0xCE, 0xFF)).bg(CODE_BG);    //THE BLOCK'S LEFT EDGE
pub const CODE_LANG: Style = Style::new().fg(Color::Rgb(0xCA, 0xB4, 0xB7)).bg(CODE_BG)
    .add_modifier(Modifier::ITALIC);

pub const MATH: Style = Style::new().fg(Color::Rgb(0xFF, 0xDD, 0xE2));         //MATH THE MESSAGE GAVE NO COLOUR

pub const SELECTED: Style = Style::new().bg(Color::Rgb(0x00, 0x5F, 0x5F));

//THE DRAG-SELECTED RUN OF THE MESSAGE PANE - THE CHROME'S SKY BLUE TAKEN DOWN TO A BACKGROUND, AND A
//BACKGROUND ONLY: EVERY GLYPH KEEPS ITS OWN COLOUR, SO A USERNAME STAYS THE COLOUR IT IS BEING COPIED AS.
//THE FULL ACCENT WOULD MEAN REPAINTING THE TEXT DARK TO STAY READABLE ON IT, WHICH IS THE ONE THING A
//SELECTION MUST NOT DO
pub const SELECTION: Style = Style::new().bg(Color::Rgb(0x30, 0x45, 0x63));

pub const ARG_REQUIRED: Style = Style::new().fg(Color::Rgb(0xD7, 0xAF, 0x87));  //SOFT SAND
pub const ARG_OPTIONAL: Style = Style::new().fg(Color::Rgb(0xFF, 0xB4, 0xAB));  //FADED CORAL
pub const ARG_ACTIVE: Style = Style::new().fg(Color::Rgb(0xFF, 0xAF, 0x5F))     //WARM AMBER - THE PARAMETER BEING TYPED
    .add_modifier(Modifier::BOLD)
    .add_modifier(Modifier::UNDERLINED);
