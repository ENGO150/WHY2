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

//! # TUI Constants
//!
//! Every tuning knob of the terminal UI: pane and layout sizes, the redraw interval, popup row
//! counts, reconnect behaviour and the markup and math limits.

use std::time::Duration;

//MESSAGE PANE
/// Most entries the message pane keeps, so rewrapping stays cheap.
pub const HISTORY_LIMIT: usize        = 5000;                      //CAP THE MESSAGE PANE SO RE-WRAPPING EACH FRAME STAYS CHEAP
/// Tallest a picture may be drawn, in rows.
pub const IMAGE_ROWS: u16             = 20;                        //TALLEST AN IMAGE MAY BE DRAWN
/// Pictures requested from the server at once.
pub const MAX_IMAGE_FETCHES: usize    = 2;                         //PICTURES ASKED OF THE SERVER AT ONCE
/// Screens above and below the view whose pictures are loaded ahead.
pub const PRELOAD_SCREENS: u16        = 1;                         //SCREENS ABOVE AND BELOW WHOSE PICTURES ARE LOADED
/// How long the pane's toast stays up.
pub const NOTICE_DURATION: Duration   = Duration::from_secs(2);    //HOW LONG THE PANE'S TOAST STAYS UP
/// An animation behind by more than this restarts from now.
pub const ANIMATION_CATCHUP: Duration = Duration::from_secs(1);    //BEHIND BY MORE THAN THIS AND IT RESTARTS
/// Longest URL a click opens.
pub const MAX_URL: usize              = 2048;                      //LONGER THAN THIS AND A CLICK OPENS NOTHING
/// Width of a transfer's progress bar, in cells.
pub const PROGRESS_CELLS: usize       = 24;                        //WIDE A TRANSFER'S BAR IS DRAWN
/// Narrowest a progress bar may be before it is dropped.
pub const MIN_PROGRESS_CELLS: usize   = 8;                         //NARROWER THAN THIS AND THE BAR IS DROPPED

//EVENT LOOP
/// Interval of the redraw tick; also the floor on animation frame times.
pub const REDRAW_INTERVAL: Duration   = Duration::from_millis(33); //COALESCE REDRAWS - VoiceActivity FIRES PER VOICE PACKET
/// Rows one scroll step moves.
pub const SCROLL_STEP: u16            = 3;

//RECONNECT
/// How long a dropped session waits before dialing again.
pub const RECONNECT_DELAY: Duration   = Duration::from_secs(3);    //HOW LONG A DROPPED SESSION WAITS BEFORE DIALLING AGAIN
/// How many dials a reconnect tries before leaving the connect box up.
pub const RECONNECT_ATTEMPTS: u32     = 5;                         //AND HOW MANY TIMES IT TRIES BEFORE LEAVING THE BOX UP

//LAYOUT
/// Width of the sidebar.
pub const SIDEBAR_WIDTH: u16          = 24;
/// Narrowest terminal that keeps the sidebar.
pub const SIDEBAR_MIN_TERM_WIDTH: u16 = 70;                        //BELOW THIS THE SIDEBAR IS DROPPED
/// Lowest the input box may be.
pub const INPUT_MIN_HEIGHT: u16       = 3;
/// Tallest the input box may grow.
pub const INPUT_MAX_HEIGHT: u16       = 8;
/// Sidebar rows the channel list needs.
pub const CHANNELS_MIN_HEIGHT: u16    = 12;                        //SIDEBAR ROWS THE CHANNEL LIST NEEDS
/// Width of the settings overlay, capped to the terminal.
pub const SETTINGS_WIDTH: u16         = 62;                        //SETTINGS OVERLAY, CAPPED TO THE TERMINAL
/// Width of the server identity overlay, capped to the terminal.
pub const TOFU_WIDTH: u16             = 64;                        //SERVER IDENTITY OVERLAY, CAPPED TO THE TERMINAL
/// Width of the connect box, capped to the terminal.
pub const LOGIN_WIDTH: u16            = 52;                        //CONNECT PROMPT, CAPPED TO THE TERMINAL
/// Rows between the connect box's label and its field.
pub const FIELD_ROW: u16              = 1;                         //THE ADDRESS FIELD SITS ONE ROW UNDER ITS OWN LABEL
/// Rows a profile picture claims in the profile box.
pub const AVATAR_ROWS: u16            = 10;                        //ROWS A PROFILE PICTURE CLAIMS IN THE BOX
/// Rows the server icon claims in the sidebar.
pub const ICON_ROWS: u16              = 6;                         //ROWS THE SERVER'S PICTURE CLAIMS IN THE SIDEBAR
/// Sidebar rows the server icon needs.
pub const ICON_MIN_HEIGHT: u16        = 20;                        //SIDEBAR ROWS THE SERVER'S PICTURE NEEDS
/// Narrowest the settings value column may get.
pub const SETTINGS_VALUE_WIDTH: u16   = 20;                        //NARROWEST THE VALUE COLUMN MAY GET (BAR + PERCENTAGE)
/// Rows kept between the selection and a list's edge.
pub const SCROLL_GAP: usize           = 4;                         //SELECTION GAP FROM A LIST'S EDGES

/// Cells of a volume slider.
#[cfg(feature = "client_voice")]
pub const SLIDER_WIDTH: usize         = 14;                        //CELLS OF VOLUME BAR

//POPUPS
/// Visible rows of the command palette.
pub const MAX_ROWS: usize             = 8;                         //VISIBLE PALETTE ROWS
/// Visible rows of the device picker before it scrolls.
pub const MAX_PICKER_ROWS: usize      = 8;                         //VISIBLE DEVICE ROWS BEFORE THE PICKER SCROLLS
/// Most directory entries offered for one path.
pub const MAX_PATHS: usize            = 256;                       //DIRECTORY ENTRIES OFFERED FOR ONE PATH
/// The mention that reaches everybody (`@everyone`).
pub const MENTION_EVERYONE: &str      = "everyone";                //THE MENTION THAT REACHES EVERYBODY

/// Key of the profile box's avatar row.
pub const AVATAR_KEY: &str           = "avatar";                  //THE AVATAR ROW'S KEY
/// Translation key of the settings box's save button.
pub const SAVE_LABEL: &str            = "settings.save";           //THE BUTTON THE SERVER ROWS ARE SENT BACK WITH (TRANSLATION KEY)
/// Translation key of the settings box's restart button.
pub const RESTART_LABEL: &str         = "settings.restart";        //AND THE ONE THAT PUTS THE STARTUP-ONLY ONES IN USE

/// Percent one key press moves a volume row by.
#[cfg(feature = "client_voice")]
pub const VOLUME_STEP: u32            = 5;                         //WHAT ONE KEYPRESS MOVES A VOLUME ROW BY

//VOICE MESSAGES
/// A push-to-talk press shorter than this is a tap, which toggles.
#[cfg(feature = "client_voice")]
pub const KEY_TAP: Duration           = Duration::from_millis(400); //SHORTER PRESS IS A TAP
/// Presses closer than this are key repeats (covers the initial repeat delay).
#[cfg(feature = "client_voice")]
pub const KEY_REPEAT_GAP: Duration    = Duration::from_millis(700); //CLOSER PRESSES ARE KEY REPEATS
/// No repeat for this long is a release, where key releases are not reported.
#[cfg(feature = "client_voice")]
pub const KEY_RELEASE_GAP: Duration   = Duration::from_millis(250); //NO REPEATS THIS LONG IS A RELEASE

//MARKUP
/// Width of a code block's bar and the space after it.
pub const GUTTER: u16                 = 2;                         //A CODE BLOCK'S BAR AND THE SPACE AFTER IT
/// Spaces a tab expands to.
pub const TAB: usize                  = 4;                         //A TAB IS EXPANDED, SINCE A CELL GRID HAS NO TAB STOPS
/// Longest first word of a fence taken as its language.
pub const MAX_LANG: usize             = 20;                        //LONGER THAN THIS AND THE FIRST WORD IS CODE

//MARKDOWN
/// Delimiter kinds the markup parser gives up on separately.
pub const MARKUP_KINDS: usize         = 9;                         //DELIMITERS THE PARSER GIVES UP ON SEPARATELY
/// Characters a backslash takes the markup off.
pub const ESCAPABLE: &str             = "`$*_~[\\#>-";             //WHAT A BACKSLASH TAKES THE MARKUP OFF
/// Most hashes a heading may have.
pub const MAX_HEADING: usize          = 3;                         //MORE HASHES THAN THIS AND THE ROW IS TEXT
/// Most digits an ordered list number may have.
pub const MAX_ORDINAL: usize          = 9;                         //AND MORE DIGITS THAN THIS IS NOT A LIST
/// Fewest markers a horizontal rule is drawn from.
pub const MIN_RULE: usize             = 3;                         //MARKERS A HORIZONTAL RULE IS DRAWN FROM
/// Most delimiters one emphasis run may have.
pub const MAX_RUN: usize              = 3;                         //AND DELIMITERS ONE EMPHASIS IS OPENED WITH
/// What a list marker is drawn as.
pub const BULLET: &str                = "• ";                      //WHAT A LIST MARKER IS DRAWN AS
/// What a blockquote's edge is drawn as.
pub const QUOTE: &str                 = "▏ ";                      //AND A BLOCKQUOTE'S EDGE
/// What a horizontal rule is drawn with.
pub const RULE: char                  = '─';
/// What a reply's quote opens with.
pub const REPLY: &str                 = "╭─ ";                     //WHAT A REPLY'S QUOTE OPENS WITH
/// A message's heart count marker.
pub const HEART: &str                 = "♥";                       //A MESSAGE'S HEART COUNT

/// Indent of display math.
pub const INDENT: &str                = "  ";                      //DISPLAY MATH IS SET IN FROM THE PANE, THE WAY A BLOCK IS
/// Deepest brace nesting the math parser follows.
pub const MAX_DEPTH: usize            = 32;                        //A BRACE THIS DEEP IS A BRACE
