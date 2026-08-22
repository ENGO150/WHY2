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

//CONSTS (I HIGHLY RECOMMEND NOT CHANGING THOSE)
pub const SAMPLE_RATE: u32          = 48000;                                    //put some text here
pub const FRAME_MS: u32             = 20;                                       //LENGTH OF ONE FRAME
pub const FRAME_SIZE: usize         = (SAMPLE_RATE * FRAME_MS / 1000) as usize; //960 SAMPLES PER FRAME

pub const AGC_TARGET_RMS: f32       = 0.05;                                     //TARGET RMS AFTER NORMALIZATION
pub const AGC_ATTACK: f32           = 0.3;                                      //ATTACK
pub const AGC_RELEASE: f32          = 0.0005;                                   //RELEASE
pub const AGC_MAX_GAIN: f32         = 10.0;                                     //MAX GAIN
pub const AGC_MIN_GAIN: f32         = 0.3;                                      //MIN GAIN

pub const NOISE_FLOOR_ALPHA: f32    = 0.05;                                     //EMA SMOOTHING FACTOR
pub const NOISE_OPEN_MULT: f32      = 3.5;                                      //OPEN TRESHOLD
pub const NOISE_CLOSE_MULT: f32     = 2.0;                                      //CLOSE TRESHOLD (HYSTERESIS)
pub const MIN_TRESHOLD_OPEN: f32    = 0.0008;                                   //HARD MINIMUM FOR OPEN
pub const MIN_TRESHOLD_CLOSE: f32   = 0.0004;                                   //HARD MINIMUM FOR CLOSE
pub const INITIAL_NOISE_FLOOR: f32  = 0.003;                                    //INIT

pub const HOLD_FRAMES: usize        = 10;                                       //~200ms HOLD TIME
pub const MIXING_TRESHOLD: f32      = 0.001;                                    //SPEAKER DETECTION NOISE TRESHOLD
pub const ACTIVITY_TRESHOLD: usize  = 100;                                      //SERVER ACTIVITY TIMER RESET TRESHOLD (~2000ms)
pub const SOUND_EFFECT_VOLUME: f32  = 0.1;                                      //VOLUME OF SOUND EFFECTS (10%)

pub const ACTIVITY_HOLD: usize      = (SAMPLE_RATE / 10) as usize;              //HOW LONG AFTER SPEAKING CLIENT BECOMES INACTIVE (~100ms)
pub const DISPLAY_HOLD: usize       = 200;                                      //ACTIVITY_HOLD BUT MS FOR DISPLAY WINDOW

pub const JITTER_BUFFER_SIZE: usize = 20;                                       //FRAME SIZE OF JITTER BUFFER

pub const GRID_WIDTH: usize         = 4;                                        //GRID WIDTH FOR VOICE PACKETS
pub const GRID_HEIGHT: usize        = 4;                                        //GRID HEIGHT FOR VOICE PACKETS

pub const RECV_TIMEOUT: u64         = 200;                                      //UDP RECEIVE POLL TIMEOUT (MS)
pub const SEND_CHANNEL_BOUND: usize = 8;                                        //AUDIO CALLBACK -> NETWORK TASK BUFFER
