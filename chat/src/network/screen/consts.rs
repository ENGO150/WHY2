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

pub const ZSTD_LEVEL: i32                = 1;                                               //LEVEL OF ZSTD COMPRESSION

pub const BUFFER_SIZE: u32               = 960;                                             //CPAL BUFFER SIZE
pub const MAX_PACKET_SIZE: usize         = 4000;                                            //MAX OPUS PACKET SIZE

pub const FRAME_SAMPLES: usize           = 1920;                                            //OPUS FRAME SAMPLES

pub const TARGET_FPS: u32                = 30;                                              //TARGET FPS
pub const FRAME_POLL_INTERVAL: Duration  = Duration::from_millis(1000 / TARGET_FPS as u64); //~30 FPS POLL

pub const JPEG_QUALITY: i32              = 80;                                              //JPEG COMPRESSION QUALITY
pub const COMPRESSION_TRESHOLD: u32      = 1;                                               //TRESHOLD FOR JPEG/ZSTD (IN %)

pub const WINIT_SIZE: (u32, u32)         = (1920, 1080);                                    //DEFAULT WINIT WINDOW SIZE

pub const MULTIPLEX_CHANNEL_BOUND: usize = 2;                                               //NETWORK BUFFER (VIDEO/AUDIO HANDOFF)
pub const CAPTURE_CHANNEL_BOUND: usize   = MULTIPLEX_CHANNEL_BOUND * 4;                     //CAPTURE BUFFER (~160ms)
pub const PLAYBACK_CHANNEL_BOUND: usize  = MULTIPLEX_CHANNEL_BOUND + 1;                     //PLAYBACK BUFFER (~60ms)
pub const NETWORK_CHANNEL_BOUND: usize   = MULTIPLEX_CHANNEL_BOUND * 8;                     //NETWORK RECEIVE BUFFER (~320ms)
