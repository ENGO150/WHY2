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

//! # Voice Session State
//!
//! Atomics read by the voice session and its realtime audio callbacks: the sequence counters,
//! whether voice is on, and the audio preferences.
//!
//! The preferences are loaded from `client.toml` on first use and changed live by the
//! front end; the callbacks read them on every buffer, so a change applies within one
//! audio period without restarting anything.

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

use crate::{ config, network::voice::consts };

//OPTIONS
/// Sequence number of the next voice packet to the server.
static SEQ: AtomicUsize = AtomicUsize::new(0); //PACKET SEQUENCE NUMBER (CLIENT -> SERVER)

/// Sequence number expected on the next voice packet from the server.
static SERVER_SEQ: AtomicUsize = AtomicUsize::new(0); //PACKET SEQUENCE NUMBER (SERVER -> CLIENT)

/// Whether the client is in voice.
static USE_VOICE: AtomicBool = AtomicBool::new(false);

//AUDIO PREFERENCES - client.toml PLUS /settings
/// Microphone volume, in percent.
static INPUT_VOLUME: LazyLock<AtomicUsize> = LazyLock::new(|| AtomicUsize::new(clamp_volume(config::read_config::<u32>("input_volume")) as usize));
/// Playback volume, in percent.
static OUTPUT_VOLUME: LazyLock<AtomicUsize> = LazyLock::new(|| AtomicUsize::new(clamp_volume(config::read_config::<u32>("output_volume")) as usize));
/// Whether captured frames are denoised.
static NOISE_SUPPRESSION: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(config::read_config::<bool>("noise_suppression")));
/// Whether captured frames are normalised.
static AUTOMATIC_GAIN: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(config::read_config::<bool>("automatic_gain")));

/// Bumped whenever the configured devices change; the session rebuilds its streams when it moves.
static DEVICE_GENERATION: AtomicUsize = AtomicUsize::new(0); //BUMPED WHENEVER THE CONFIGURED DEVICES CHANGE

//SEQ
/// Returns the sequence number of the next voice packet.
pub fn get_seq() -> usize //GET SEQUENCE NUMBER
{
    SEQ.load(Ordering::Relaxed)
}

/// Sets the sequence number of the next voice packet.
pub fn set_seq(value: usize) //SET SEQUENCE NUMBER
{
    SEQ.store(value, Ordering::Relaxed)
}

//SERVER SEQ
/// Returns the sequence number expected from the server.
pub fn get_server_seq() -> usize //GET SERVER SEQUENCE NUMBER
{
    SERVER_SEQ.load(Ordering::Relaxed)
}

/// Sets the sequence number expected from the server.
pub fn set_server_seq(value: usize) //SET SERVER SEQUENCE NUMBER
{
    SERVER_SEQ.store(value, Ordering::Relaxed)
}

//USE VOICE
/// Returns whether the client is in voice.
pub fn get_use_voice() -> bool //GET USE VOICE
{
    USE_VOICE.load(Ordering::Relaxed)
}

/// Sets whether the client is in voice; setting it `false` ends the running session.
pub fn set_use_voice(value: bool) //FORCE USE VOICE (A LOST SESSION TAKES THE CALL WITH IT)
{
    USE_VOICE.store(value, Ordering::Relaxed)
}

//AUDIO PREFERENCES
/// Clamps a volume to [`VOLUME_MAX`](consts::VOLUME_MAX).
pub fn clamp_volume(percent: u32) -> u32 //KEEP A VOLUME INSIDE THE SUPPORTED RANGE
{
    percent.min(consts::VOLUME_MAX)
}

/// Loads every audio preference from `client.toml`, if not loaded yet.
pub fn init_audio() //TOUCH EVERY PREFERENCE
{
    get_input_volume();
    get_output_volume();
    noise_suppression();
    automatic_gain();
}

/// Returns the microphone volume, in percent.
pub fn get_input_volume() -> u32 //MICROPHONE VOLUME (PERCENT)
{
    INPUT_VOLUME.load(Ordering::Relaxed) as u32
}

/// Sets the microphone volume, in percent.
pub fn set_input_volume(percent: u32)
{
    INPUT_VOLUME.store(clamp_volume(percent) as usize, Ordering::Relaxed);
}

/// Returns the playback volume, in percent.
pub fn get_output_volume() -> u32 //PLAYBACK VOLUME (PERCENT)
{
    OUTPUT_VOLUME.load(Ordering::Relaxed) as u32
}

/// Sets the playback volume, in percent.
pub fn set_output_volume(percent: u32)
{
    OUTPUT_VOLUME.store(clamp_volume(percent) as usize, Ordering::Relaxed);
}

/// Returns the microphone volume as a multiplier.
pub fn get_input_gain() -> f32 //MICROPHONE VOLUME AS A MULTIPLIER
{
    get_input_volume() as f32 / 100.
}

/// Returns the playback volume as a multiplier.
pub fn get_output_gain() -> f32 //PLAYBACK VOLUME AS A MULTIPLIER
{
    get_output_volume() as f32 / 100.
}

/// Returns whether captured frames are denoised.
pub fn noise_suppression() -> bool //RUN THE DENOISER ON CAPTURED FRAMES
{
    NOISE_SUPPRESSION.load(Ordering::Relaxed)
}

/// Sets whether captured frames are denoised.
pub fn set_noise_suppression(value: bool)
{
    NOISE_SUPPRESSION.store(value, Ordering::Relaxed);
}

/// Returns whether captured frames are normalised (AGC and limiter).
pub fn automatic_gain() -> bool //NORMALIZE CAPTURED FRAMES (AGC + LIMITER)
{
    AUTOMATIC_GAIN.load(Ordering::Relaxed)
}

/// Sets whether captured frames are normalised.
pub fn set_automatic_gain(value: bool)
{
    AUTOMATIC_GAIN.store(value, Ordering::Relaxed);
}

//DEVICE GENERATION
/// Returns the device generation.
pub fn device_generation() -> usize //BUMPED WHEN THE PICKED DEVICES CHANGE
{
    DEVICE_GENERATION.load(Ordering::Relaxed)
}

/// Signals that `input_device` or `output_device` was rewritten.
pub fn mark_devices_changed() //input_device/output_device WAS REWRITTEN
{
    DEVICE_GENERATION.fetch_add(1, Ordering::Relaxed);
}
