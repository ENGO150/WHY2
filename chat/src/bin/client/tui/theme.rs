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
    style::{ Color, Modifier, Style },
    text::Span,
    backend::FromCrossterm,
};

use crate::{ colors, config };

//STRUCTS
pub struct Theme //CACHED CONFIG-DRIVEN STYLING (config::read_config RE-PARSES THE TOML, NEVER CALL IT PER FRAME)
{
    pub disable_colors: bool,
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
            show_id: config::read_config::<bool>("show_id"),
        }
    }

    pub fn reload(&mut self) //RE-READ AFTER A config::client_write
    {
        *self = Self::load();
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
pub const BORDER: Style = Style::new().fg(Color::DarkGray);
pub const BORDER_ACTIVE: Style = Style::new().fg(Color::Cyan);
pub const TITLE: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
pub const DIM: Style = Style::new().fg(Color::DarkGray);
pub const ACCENT: Style = Style::new().fg(Color::Cyan);
pub const NOTICE: Style = Style::new().fg(Color::Yellow);
pub const ERROR: Style = Style::new().fg(Color::Red);
pub const OK: Style = Style::new().fg(Color::Green);
pub const SPEAKING: Style = Style::new().fg(Color::Green).add_modifier(Modifier::BOLD);
//BACKGROUND ONLY, SO THE ROW KEEPS ITS OWN COLORS. A GREY WASH READS AS A BLACK BAR ON A DARK
//TERMINAL, SO THE SELECTION IS TINTED TOWARDS THE CYAN ACCENT INSTEAD - A HUE DIFFERENCE, NOT A
//BRIGHTNESS ONE, WHICH STAYS SUBTLE WHATEVER THE BACKGROUND IS.
pub const SELECTED: Style = Style::new().bg(Color::Indexed(23)); //DEEP TEAL

//THE PALETTE USES 256-COLOR INDICES RATHER THAN THE SIXTEEN NAMED ANSI COLORS: THOSE ARE REMAPPED BY
//THE TERMINAL'S OWN SCHEME (MAGENTA COMES OUT A VIVID RED IN PLENTY OF THEM), THESE DO NOT MOVE.
pub const ARG_REQUIRED: Style = Style::new().fg(Color::Indexed(180)); //SOFT SAND
pub const ARG_OPTIONAL: Style = Style::new().fg(Color::Indexed(244)); //MID GREY
pub const ARG_ACTIVE: Style = Style::new().fg(Color::Indexed(215))    //WARM AMBER - THE PARAMETER BEING TYPED
    .add_modifier(Modifier::BOLD)
    .add_modifier(Modifier::UNDERLINED);
