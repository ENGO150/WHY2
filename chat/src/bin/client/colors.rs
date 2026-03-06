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

use colored::Color;

pub fn color_to_u8(color: &Color) -> u8 //MAP COLOR TO COLOR CODE
{
    match color
    {
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::White => 7,
        Color::BrightBlack => 8,
        Color::BrightRed => 9,
        Color::BrightGreen => 10,
        Color::BrightYellow => 11,
        Color::BrightBlue => 12,
        Color::BrightMagenta => 13,
        Color::BrightCyan => 14,
        Color::BrightWhite => 15,
        _ => 255, //UNKNOWN
    }
}

pub fn u8_to_color(val: u8) -> Option<Color> //COLOR CODE TO COLOR
{
    match val
    {
        0 => Some(Color::Black),
        1 => Some(Color::Red),
        2 => Some(Color::Green),
        3 => Some(Color::Yellow),
        4 => Some(Color::Blue),
        5 => Some(Color::Magenta),
        6 => Some(Color::Cyan),
        7 => Some(Color::White),
        8 => Some(Color::BrightBlack),
        9 => Some(Color::BrightRed),
        10 => Some(Color::BrightGreen),
        11 => Some(Color::BrightYellow),
        12 => Some(Color::BrightBlue),
        13 => Some(Color::BrightMagenta),
        14 => Some(Color::BrightCyan),
        15 => Some(Color::BrightWhite),
        _ => None,
    }
}
