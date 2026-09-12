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

use why2_chat::colors::COLORS;

const BRIGHT: usize = 8; //WHERE THE BRIGHT HALF OF THE CODE TABLE STARTS

//FUNCTIONS
//THE CROSSTERM SIDE OF THE CODE TABLE
pub use why2_chat::colors::code;

pub fn u8_to_color(val: u8) -> Option<Color> //COLOR CODE TO COLOR
{
    by_name(COLORS.get(val as usize)?)
}

pub fn by_name(name: &str) -> Option<Color> //NAME AS THE PALETTE OFFERS IT BACK TO ITS COLOR
{
    COLORS.iter().find(|n| n.eq_ignore_ascii_case(name)).and_then(|n| Color::try_from(*n).ok())
}

//THE ORDER TO OFFER COLORS IN
pub fn offered() -> Vec<&'static str>
{
    let mut names = COLORS.iter().skip(BRIGHT).copied().collect::<Vec<&'static str>>();
    let mut dark = COLORS.iter().take(BRIGHT).copied().collect::<Vec<&'static str>>();

    names.sort_unstable();
    dark.sort_unstable();

    names.extend(dark);
    names
}
