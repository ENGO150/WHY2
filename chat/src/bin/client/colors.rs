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
//THE CODE TABLE IS THE LIBRARY'S - IT IS THE WIRE, AND THE SERVER STORES IT - SO WHAT LIVES HERE IS ONLY THE
//CROSSTERM SIDE OF IT: EVERY ONE OF THOSE NAMES IS ONE crossterm PARSES, WHICH IS WHERE THE Color COMES FROM.
//A TYPED NAME BECOMES A CODE THROUGH THE LIBRARY'S OWN LOOKUP, RE-EXPORTED SO THE CLIENT ASKS IN ONE PLACE
pub use why2_chat::colors::code;

pub fn u8_to_color(val: u8) -> Option<Color> //COLOR CODE TO COLOR
{
    by_name(COLORS.get(val as usize)?)
}

pub fn by_name(name: &str) -> Option<Color> //NAME AS THE PALETTE OFFERS IT BACK TO ITS COLOR
{
    COLORS.iter().find(|n| n.eq_ignore_ascii_case(name)).and_then(|n| Color::try_from(*n).ok())
}

//THE ORDER TO OFFER THEM IN: THE BRIGHT HALF FIRST, THEN THE DARK ONE, EACH ALPHABETICAL. PICKING A COLOR IS A
//DIFFERENT QUESTION FROM SENDING ONE, SO IT GETS ITS OWN ORDER RATHER THAN INHERITING THE CODE TABLE'S.
//THE SPLIT IS THE ANSI ONE (CODES 8-15 ARE THE BRIGHT HALF), WHICH IS WHY grey SITS WITH THE DARK COLORS AND
//dark_grey WITH THE BRIGHT ONES - THAT IS WHERE THE TERMINAL PUTS THEM, WHATEVER THE NAMES SUGGEST.
//EACH HALF IS EXACTLY palette::MAX_ROWS LONG, SO AN UNFILTERED POPUP SHOWS ONE HALF AT A TIME
pub fn offered() -> Vec<&'static str>
{
    let mut names = COLORS.iter().skip(BRIGHT).copied().collect::<Vec<&'static str>>();
    let mut dark = COLORS.iter().take(BRIGHT).copied().collect::<Vec<&'static str>>();

    names.sort_unstable();
    dark.sort_unstable();

    names.extend(dark);
    names
}
