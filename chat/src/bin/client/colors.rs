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

use crossterm::style::Color;

//CONSTS
//THE WHOLE VOCABULARY OF /color AND /ucolor, IN CODE ORDER - THE NAMES ARE THE ONES crossterm ITSELF PARSES,
//SO WHAT THE PALETTE OFFERS IS EXACTLY WHAT to_color ACCEPTS AND WHAT LANDS IN client.toml
pub const COLORS: [(&str, Color); 16] =
[
    ("black",        Color::Black),
    ("dark_red",     Color::DarkRed),
    ("dark_green",   Color::DarkGreen),
    ("dark_yellow",  Color::DarkYellow),
    ("dark_blue",    Color::DarkBlue),
    ("dark_magenta", Color::DarkMagenta),
    ("dark_cyan",    Color::DarkCyan),
    ("grey",         Color::Grey),
    ("dark_grey",    Color::DarkGrey),
    ("red",          Color::Red),
    ("green",        Color::Green),
    ("yellow",       Color::Yellow),
    ("blue",         Color::Blue),
    ("magenta",      Color::Magenta),
    ("cyan",         Color::Cyan),
    ("white",        Color::White),
];

//FUNCTIONS
pub fn color_to_u8(color: &Color) -> u8 //MAP COLOR TO COLOR CODE
{
    COLORS.iter().position(|(_, c)| c == color).map_or(255, |i| i as u8) //255 - UNKNOWN
}

pub fn u8_to_color(val: u8) -> Option<Color> //COLOR CODE TO COLOR
{
    COLORS.get(val as usize).map(|(_, color)| *color)
}

pub fn by_name(name: &str) -> Option<Color> //NAME AS THE PALETTE OFFERS IT BACK TO ITS COLOR
{
    COLORS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, color)| *color)
}
