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

#[cfg(feature = "client")]
use std::sync::atomic::
{
    AtomicBool,
    AtomicUsize,
    Ordering,
};

//CONSTS (I HIGHLY RECOMMEND NOT CHANGING THOSE)
pub const SAMPLE_RATE: u32         = 48000;                                    //put some text here
pub const FRAME_MS: u32            = 20;                                       //LENGTH OF ONE FRAME
pub const FRAME_SIZE: usize        = (SAMPLE_RATE * FRAME_MS / 1000) as usize; //960 SAMPLES PER FRAME

pub const TRESHOLD_OPEN: f32       = 0.002;                                    //INPUT THRESHOLD
pub const TRESHOLD_CLOSE: f32      = 0.001;                                    //INPUT THRESHOLD (HYSTERESIS)
pub const HOLD_FRAMES: usize       = 10;                                       //~200ms HOLD TIME
pub const MIXING_TRESHOLD: f32     = 0.001;                                    //SPEAKER DETECTION NOISE TRESHOLD
pub const ACTIVITY_TRESHOLD: usize = 100;                                      //SERVER ACTIVITY TIMER RESET TRESHOLD (~2000ms)

//GLOBAL VARIABLES
#[cfg(feature = "client")]
static SEQ: AtomicUsize = AtomicUsize::new(0); //PACKET SEQUENCE NUMBER (CLIENT -> SERVER)

#[cfg(feature = "client")]
static SERVER_SEQ: AtomicUsize = AtomicUsize::new(0); //PACKET SEQUENCE NUMBER (SERVER -> CLIENT)

#[cfg(feature = "client")]
static USE_VOICE: AtomicBool = AtomicBool::new(false);

//SEQ
#[cfg(feature = "client")]
pub fn get_seq() -> usize //GET SEQUENCE NUMBER
{
    SEQ.load(Ordering::Relaxed)
}

#[cfg(feature = "client")]
pub fn set_seq(value: usize) //SET SEQUENCE NUMBER
{
    SEQ.store(value, Ordering::Relaxed)
}

//SERVER SEQ
#[cfg(feature = "client")]
pub fn get_server_seq() -> usize //GET SERVER SEQUENCE NUMBER
{
    SERVER_SEQ.load(Ordering::Relaxed)
}

#[cfg(feature = "client")]
pub fn set_server_seq(value: usize) //SET SERVER SEQUENCE NUMBER
{
    SERVER_SEQ.store(value, Ordering::Relaxed)
}

//USE VOICE
#[cfg(feature = "client")]
pub fn get_use_voice() -> bool //GET USE VOICE
{
    USE_VOICE.load(Ordering::Relaxed)
}

#[cfg(feature = "client")]
pub fn swap_use_voice() -> bool //SET USE VOICE
{
    let value = !USE_VOICE.load(Ordering::Relaxed);
    USE_VOICE.store(value, Ordering::Relaxed);
    value
}
