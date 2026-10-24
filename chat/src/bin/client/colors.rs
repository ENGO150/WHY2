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

//! # Terminal Colors
//!
//! The crossterm side of the chat color table: turning a wire color code or a typed name into a
//! terminal [`Color`], and the order the palette offers names in.

use crossterm::style::Color;

use why2_chat::colors::COLORS;

/// Index where the bright half of the color table starts.
const BRIGHT: usize = 8; //WHERE THE BRIGHT HALF OF THE CODE TABLE STARTS

//FUNCTIONS
//THE CROSSTERM SIDE OF THE CODE TABLE
pub use why2_chat::colors::code;

/// Returns the terminal color of a wire color code.
pub fn u8_to_color(val: u8) -> Option<Color> //COLOR CODE TO COLOR
{
    by_name(COLORS.get(val as usize)?)
}

/// Returns the terminal color of a color name, matched case-insensitively.
pub fn by_name(name: &str) -> Option<Color> //NAME AS THE PALETTE OFFERS IT BACK TO ITS COLOR
{
    COLORS.iter().find(|n| n.eq_ignore_ascii_case(name)).and_then(|n| Color::try_from(*n).ok())
}

//THE ORDER TO OFFER COLORS IN
/// Returns the color names in the order the palette offers them: bright colors first, each half
/// sorted.
pub fn offered() -> Vec<&'static str>
{
    let mut names = COLORS.iter().skip(BRIGHT).copied().collect::<Vec<&'static str>>();
    let mut dark = COLORS.iter().take(BRIGHT).copied().collect::<Vec<&'static str>>();

    names.sort_unstable();
    dark.sort_unstable();

    names.extend(dark);
    names
}
