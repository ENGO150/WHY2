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

use std::sync::
{
    LazyLock,
    atomic::
    {
        AtomicBool,
        AtomicUsize,
        Ordering,
    },
};

use crate::config;

//CONSTS
pub const VOLUME_MAX: u32 = 200; //LOUDEST SETTING (PERCENT)

//OPTIONS
static SEQ: AtomicUsize = AtomicUsize::new(0); //PACKET SEQUENCE NUMBER (CLIENT -> SERVER)

static SERVER_SEQ: AtomicUsize = AtomicUsize::new(0); //PACKET SEQUENCE NUMBER (SERVER -> CLIENT)

static USE_VOICE: AtomicBool = AtomicBool::new(false);

//AUDIO PREFERENCES - SEEDED FROM client.toml, LIVE-EDITED BY /settings
static INPUT_VOLUME: LazyLock<AtomicUsize> = LazyLock::new(|| AtomicUsize::new(clamp_volume(config::read_config::<u32>("input_volume")) as usize));
static OUTPUT_VOLUME: LazyLock<AtomicUsize> = LazyLock::new(|| AtomicUsize::new(clamp_volume(config::read_config::<u32>("output_volume")) as usize));
static NOISE_SUPPRESSION: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(config::read_config::<bool>("noise_suppression")));
static AUTOMATIC_GAIN: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(config::read_config::<bool>("automatic_gain")));

static DEVICE_GENERATION: AtomicUsize = AtomicUsize::new(0); //BUMPED WHENEVER THE CONFIGURED DEVICES CHANGE

//SEQ
pub fn get_seq() -> usize //GET SEQUENCE NUMBER
{
    SEQ.load(Ordering::Relaxed)
}

pub fn set_seq(value: usize) //SET SEQUENCE NUMBER
{
    SEQ.store(value, Ordering::Relaxed)
}

//SERVER SEQ
pub fn get_server_seq() -> usize //GET SERVER SEQUENCE NUMBER
{
    SERVER_SEQ.load(Ordering::Relaxed)
}

pub fn set_server_seq(value: usize) //SET SERVER SEQUENCE NUMBER
{
    SERVER_SEQ.store(value, Ordering::Relaxed)
}

//USE VOICE
pub fn get_use_voice() -> bool //GET USE VOICE
{
    USE_VOICE.load(Ordering::Relaxed)
}

pub fn swap_use_voice() -> bool //SET USE VOICE
{
    !USE_VOICE.fetch_xor(true, Ordering::Relaxed)
}

//AUDIO PREFERENCES
pub fn clamp_volume(percent: u32) -> u32 //KEEP A VOLUME INSIDE THE SUPPORTED RANGE
{
    percent.min(VOLUME_MAX)
}

pub fn init_audio() //TOUCH EVERY PREFERENCE SO NO AUDIO CALLBACK EVER PAYS FOR THE CONFIG READ
{
    get_input_volume();
    get_output_volume();
    noise_suppression();
    automatic_gain();
}

pub fn get_input_volume() -> u32 //MICROPHONE VOLUME (PERCENT)
{
    INPUT_VOLUME.load(Ordering::Relaxed) as u32
}

pub fn set_input_volume(percent: u32)
{
    INPUT_VOLUME.store(clamp_volume(percent) as usize, Ordering::Relaxed);
}

pub fn get_output_volume() -> u32 //PLAYBACK VOLUME (PERCENT)
{
    OUTPUT_VOLUME.load(Ordering::Relaxed) as u32
}

pub fn set_output_volume(percent: u32)
{
    OUTPUT_VOLUME.store(clamp_volume(percent) as usize, Ordering::Relaxed);
}

pub fn get_input_gain() -> f32 //MICROPHONE VOLUME AS A MULTIPLIER
{
    get_input_volume() as f32 / 100.
}

pub fn get_output_gain() -> f32 //PLAYBACK VOLUME AS A MULTIPLIER
{
    get_output_volume() as f32 / 100.
}

pub fn noise_suppression() -> bool //RUN THE DENOISER ON CAPTURED FRAMES
{
    NOISE_SUPPRESSION.load(Ordering::Relaxed)
}

pub fn set_noise_suppression(value: bool)
{
    NOISE_SUPPRESSION.store(value, Ordering::Relaxed);
}

pub fn automatic_gain() -> bool //NORMALIZE CAPTURED FRAMES (AGC + LIMITER)
{
    AUTOMATIC_GAIN.load(Ordering::Relaxed)
}

pub fn set_automatic_gain(value: bool)
{
    AUTOMATIC_GAIN.store(value, Ordering::Relaxed);
}

//DEVICE GENERATION
pub fn device_generation() -> usize //A RUNNING VOICE SESSION REBUILDS ITS STREAMS WHEN THIS MOVES
{
    DEVICE_GENERATION.load(Ordering::Relaxed)
}

pub fn mark_devices_changed() //input_device/output_device WAS REWRITTEN
{
    DEVICE_GENERATION.fetch_add(1, Ordering::Relaxed);
}
