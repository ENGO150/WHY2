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

pub fn color_to_u8(color: &Color) -> u8 //MAP COLOR TO COLOR CODE
{
    match color
    {
        Color::Black => 0,
        Color::DarkRed => 1,
        Color::DarkGreen => 2,
        Color::DarkYellow => 3,
        Color::DarkBlue => 4,
        Color::DarkMagenta => 5,
        Color::DarkCyan => 6,
        Color::Grey => 7,
        Color::DarkGrey => 8,
        Color::Red => 9,
        Color::Green => 10,
        Color::Yellow => 11,
        Color::Blue => 12,
        Color::Magenta => 13,
        Color::Cyan => 14,
        Color::White => 15,
        _ => 255, //UNKNOWN
    }
}

pub fn u8_to_color(val: u8) -> Option<Color> //COLOR CODE TO COLOR
{
    match val
    {
        0 => Some(Color::Black),
        1 => Some(Color::DarkRed),
        2 => Some(Color::DarkGreen),
        3 => Some(Color::DarkYellow),
        4 => Some(Color::DarkBlue),
        5 => Some(Color::DarkMagenta),
        6 => Some(Color::DarkCyan),
        7 => Some(Color::Grey),
        8 => Some(Color::DarkGrey),
        9 => Some(Color::Red),
        10 => Some(Color::Green),
        11 => Some(Color::Yellow),
        12 => Some(Color::Blue),
        13 => Some(Color::Magenta),
        14 => Some(Color::Cyan),
        15 => Some(Color::White),
        _ => None,
    }
}
