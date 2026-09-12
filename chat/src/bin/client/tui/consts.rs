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

use std::time::Duration;

//MESSAGE PANE
pub const HISTORY_LIMIT: usize        = 5000;                      //CAP THE MESSAGE PANE SO RE-WRAPPING EACH FRAME STAYS CHEAP
pub const IMAGE_ROWS: u16             = 20;                        //TALLEST AN IMAGE MAY BE DRAWN
pub const NOTICE_DURATION: Duration   = Duration::from_secs(2);    //HOW LONG THE PANE'S TOAST STAYS UP
pub const ANIMATION_CATCHUP: Duration = Duration::from_secs(1);    //BEHIND BY MORE THAN THIS AND IT RESTARTS

//EVENT LOOP
pub const REDRAW_INTERVAL: Duration   = Duration::from_millis(33); //COALESCE REDRAWS - VoiceActivity FIRES PER VOICE PACKET
pub const SCROLL_STEP: u16            = 3;

//RECONNECT
pub const RECONNECT_DELAY: Duration   = Duration::from_secs(3);    //HOW LONG A DROPPED SESSION WAITS BEFORE DIALLING AGAIN
pub const RECONNECT_ATTEMPTS: u32     = 5;                         //AND HOW MANY TIMES IT TRIES BEFORE LEAVING THE BOX UP

//LAYOUT
pub const SIDEBAR_WIDTH: u16          = 24;
pub const SIDEBAR_MIN_TERM_WIDTH: u16 = 70;                        //BELOW THIS THE SIDEBAR IS DROPPED
pub const INPUT_MIN_HEIGHT: u16       = 3;
pub const INPUT_MAX_HEIGHT: u16       = 8;
pub const CHANNELS_MIN_HEIGHT: u16    = 12;                        //SIDEBAR ROWS THE CHANNEL LIST NEEDS
pub const SETTINGS_WIDTH: u16         = 62;                        //SETTINGS OVERLAY, CAPPED TO THE TERMINAL
pub const TOFU_WIDTH: u16             = 64;                        //SERVER IDENTITY OVERLAY, CAPPED TO THE TERMINAL
pub const LOGIN_WIDTH: u16            = 52;                        //CONNECT PROMPT, CAPPED TO THE TERMINAL
pub const FIELD_ROW: u16              = 1;                         //THE ADDRESS FIELD SITS ONE ROW UNDER ITS OWN LABEL
pub const SETTINGS_VALUE_WIDTH: u16   = 20;                        //NARROWEST THE VALUE COLUMN MAY GET (BAR + PERCENTAGE)
pub const SCROLL_GAP: usize           = 4;                         //SELECTION GAP FROM A LIST'S EDGES

#[cfg(feature = "client_voice")]
pub const SLIDER_WIDTH: usize         = 14;                        //CELLS OF VOLUME BAR

//POPUPS
pub const MAX_ROWS: usize             = 8;                         //VISIBLE PALETTE ROWS
pub const MAX_PICKER_ROWS: usize      = 8;                         //VISIBLE DEVICE ROWS BEFORE THE PICKER SCROLLS

pub const SAVE_LABEL: &str            = "Save";                    //THE BUTTON THE SERVER ROWS ARE SENT BACK WITH
pub const RESTART_LABEL: &str         = "Restart server";          //AND THE ONE THAT PUTS THE STARTUP-ONLY ONES IN USE
pub const CHALLENGE: &str             = "yes";                     //WHAT TOFU'S SECOND STAGE WANTS TYPED OUT

#[cfg(feature = "client_voice")]
pub const DEFAULT_DEVICE: &str        = "System default";          //SHOWN FOR AN EMPTY input_device/output_device

#[cfg(feature = "client_voice")]
pub const VOLUME_STEP: u32            = 5;                         //WHAT ONE KEYPRESS MOVES A VOLUME ROW BY

//MARKUP
pub const GUTTER: u16                 = 2;                         //A CODE BLOCK'S BAR AND THE SPACE AFTER IT
pub const TAB: usize                  = 4;                         //A TAB IS EXPANDED, SINCE A CELL GRID HAS NO TAB STOPS
pub const MAX_LANG: usize             = 20;                        //LONGER THAN THIS AND THE FIRST WORD IS CODE

pub const INDENT: &str                = "  ";                      //DISPLAY MATH IS SET IN FROM THE PANE, THE WAY A BLOCK IS
pub const MAX_DEPTH: usize            = 32;                        //A BRACE THIS DEEP IS A BRACE
