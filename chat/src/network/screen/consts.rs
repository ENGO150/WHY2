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

pub const H264_BITRATE: u32               = 4_000_000;                                       //H.264 ENCODER TARGET BITRATE (4 Mbps)

pub const BUFFER_SIZE: u32                = 960;                                             //CPAL BUFFER SIZE
pub const MAX_PACKET_SIZE: usize          = 4000;                                            //MAX OPUS PACKET SIZE

pub const FRAME_SAMPLES: usize            = 1920;                                            //OPUS FRAME SAMPLES

pub const TARGET_FPS: u32                 = 30;                                              //TARGET FPS
pub const FRAME_POLL_INTERVAL: Duration   = Duration::from_millis(1000 / TARGET_FPS as u64); //~30 FPS POLL

pub const WINIT_SIZE: (u32, u32)          = (1920, 1080);                                    //DEFAULT WINIT WINDOW SIZE
pub const WAYLAND_RECONNECT_FAILURES: u32 = 3;                                               //CONSECUTIVE CAPTURE FAILURES BEFORE RECONNECTING
pub const WAYLAND_LEAK_BUDGET: u64        = 128 * 1024 * 1024;                               //COMPOSITOR MEMORY A SHARE MAY STRAND BEFORE THE CONNECTION IS RECYCLED (SEE capture_loop_wayshot)

pub const MULTIPLEX_CHANNEL_BOUND: usize  = 2;                                               //NETWORK BUFFER (VIDEO/AUDIO HANDOFF)
pub const CAPTURE_CHANNEL_BOUND: usize    = MULTIPLEX_CHANNEL_BOUND * 4;                     //CAPTURE BUFFER (~160ms)
pub const PLAYBACK_CHANNEL_BOUND: usize   = MULTIPLEX_CHANNEL_BOUND + 1;                     //PLAYBACK BUFFER (~60ms)
pub const NETWORK_CHANNEL_BOUND: usize    = MULTIPLEX_CHANNEL_BOUND * 8;                     //NETWORK RECEIVE BUFFER (~320ms)
pub const AUDIO_BACKLOG_TARGET: usize     = MULTIPLEX_CHANNEL_BOUND * 2 + 1;                 //AUDIO FRAMES THE RECEIVE QUEUE IS SHED BACK DOWN TO (~100ms OF JITTER TOLERANCE, SEE spawn_audio_playback)

pub const SOCKET_BUFFER: usize            = 128 * 1024;                                      //KERNEL QUEUE A SCREEN SHARE SOCKET MAY HOLD (SEE cap_socket_buffers)

pub const FORCED_INTRA_INTERVAL: Duration  = Duration::from_secs(2);      //MAX GAP BETWEEN ENCODED FRAMES (KEEPS A LATE VIEWER SYNCED)
pub const RECORDER_POLL_INTERVAL: Duration = Duration::from_millis(100);  //HOW OFTEN THE RECORDER LOOP RECHECKS `running` WHILE IDLE
pub const BACKEND_OVERRIDE_VAR: &str       = "WHY2_CAPTURE_BACKEND";      //PINS A CAPTURE BACKEND ("recorder" / "legacy")
pub const PROBE_TIMEOUT_VAR: &str          = "WHY2_CAPTURE_PROBE_TIMEOUT"; //OVERRIDES THE PROBE TIMEOUT, IN SECONDS
pub const RECORDER_PROBE_TIMEOUT: Duration = Duration::from_secs(30);     //HOW LONG THE RECORDER PROBE MAY BLOCK BEFORE WE FALL BACK
pub const CONVERTER_OVERRIDE_VAR: &str     = "WHY2_CAPTURE_CONVERTER"; //PINS THE RGBA -> I420 PATH ("gpu" / "cpu")
pub const MONITOR_LIST_TTL: Duration       = Duration::from_secs(5);      //HOW LONG THE PALETTE'S MONITOR LIST IS REUSED BEFORE THE DISPLAY SERVER IS ASKED AGAIN
pub const RECORDER_FIRST_FRAME: Duration  = Duration::from_secs(5);   //A RECORDER THAT DELIVERS NOTHING IN THIS LONG IS TREATED AS BROKEN

pub const MUTED_FRAME_INTERVAL: Duration  = Duration::from_millis(100); //FRAME DURATION OF THE PLACEHOLDER SHOWN WHILE A SHARER IS MUTED (10 FPS)
