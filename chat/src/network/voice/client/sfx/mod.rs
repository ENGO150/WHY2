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

//! # Sound Effects
//!
//! Sounds decoded once from embedded Ogg Vorbis. Join and leave sounds are mixed into
//! voice output, and private message notifications have their own output stream.

use std::
{
    io::Cursor,
    time::Duration,
    sync::
    {
        Arc,
        Mutex,
        LazyLock,
        mpsc,
    },
};

use tokio::task;

use cpal::traits::{ DeviceTrait, StreamTrait };
use lewton::inside_ogg::OggStreamReader;

use crate::
{
    config,
    network::voice::
    {
        client as voice,
        options,
        consts,
    },
};

//STRUCTS
/// A sound effect being played.
struct ActiveEffect
{
    /// The decoded samples.
    buffer: Arc<Vec<f32>>,
    /// The next sample to play.
    position: usize,
}

//ENUMS
/// A sound effect.
pub enum SoundEffect
{
    /// Somebody joined voice.
    Join,
    /// Somebody left voice.
    Leave,
}

//GLOBAL VARIABLES
/// Sound effects being played.
static ACTIVE_EFFECTS: Mutex<Vec<ActiveEffect>> = Mutex::new(Vec::new());

//ASSETS
/// The decoded join sound, mono.
pub static JOIN_SOUND: LazyLock<Arc<Vec<f32>>> = LazyLock::new(|| //DI DING
{
    let bytes = include_bytes!("./join.ogg");
    Arc::new(decode_ogg_from_bytes(bytes))
});

/// The decoded leave sound, mono.
pub static LEAVE_SOUND: LazyLock<Arc<Vec<f32>>> = LazyLock::new(|| //(DI DING).rev()
{
    let bytes = include_bytes!("./leave.ogg");
    Arc::new(decode_ogg_from_bytes(bytes))
});

/// The decoded private message sound, mono.
static PM_SOUND: LazyLock<Vec<f32>> = LazyLock::new(|| decode_ogg_from_bytes(include_bytes!("./pm.ogg"))); //(DI DING).fuck_up()

//FUNCTIONS
//PRIVATE
/// Decodes Ogg Vorbis to mono samples in `[-1, 1]`.
///
/// # Panics
/// If the data is not a readable Ogg stream.
fn decode_ogg_from_bytes(bytes: &[u8]) -> Vec<f32> //DECODING HELPER
{
    let cursor = Cursor::new(bytes);
    let mut reader = OggStreamReader::new(cursor).expect("Failed to read ogg");

    let mut samples = Vec::new();
    let channels = reader.ident_hdr.audio_channels as usize;

    //LOAD PACKETS
    while let Ok(Some(packet)) = reader.read_dec_packet_itl()
    {
        //DOWNMIX TO MONO
        for chunk in packet.chunks(channels)
        {
            let mut sum = 0.0;
            for sample in chunk
            {
                sum += *sample as f32;
            }

            samples.push((sum / channels as f32) / 32768.0);
        }
    }
    samples
}

//PUBLIC
/// Plays a private message notification, including outside a voice call.
pub fn play_private_message()
{
    task::spawn_blocking(||
    {
        let Some(device) = voice::pick_device(&config::read_config::<String>("output_device"), false) else { return; };
        let (Ok(supported), Ok(default)) = (device.supported_output_configs(), device.default_output_config()) else { return; };
        let config = voice::configure_device(&device, supported, default, false);
        let channels = config.channels as usize;
        let step = consts::SAMPLE_RATE as f64 / config.sample_rate as f64;
        let samples = &*PM_SOUND;
        let mut position = 0_f64;
        options::get_sound_effect_volume();
        options::get_output_volume();
        let (done, finished) = mpsc::sync_channel(1);
        let failed = done.clone();

        let Ok(stream) = device.build_output_stream(config, move |data: &mut [f32], _: &_|
        {
            if position as usize >= samples.len()
            {
                data.fill(0.);
                let _ = done.try_send(());
                return;
            }

            let gain = options::get_output_gain() * options::get_sound_effect_gain();
            for frame in data.chunks_mut(channels)
            {
                let index = position as usize;
                let current = samples.get(index).copied().unwrap_or(0.);
                let next = samples.get(index + 1).copied().unwrap_or(0.);
                let sample = current + (next - current) * position.fract() as f32;
                frame.fill((sample * gain).tanh());
                position += step;
            }
        }, move |_| { let _ = failed.try_send(()); }, None) else { return; };

        if stream.play().is_ok()
        {
            let duration = Duration::from_secs_f64(samples.len() as f64 / consts::SAMPLE_RATE as f64);
            let _ = finished.recv_timeout(duration + Duration::from_secs(2));
        }
    });
}

/// Starts playing a sound effect.
pub fn queue_effect(effect: SoundEffect) //ADD SOUND TO ACTIVE_EFFECTS
{
    let sound = match effect
    {
        SoundEffect::Join => JOIN_SOUND.clone(),
        SoundEffect::Leave => LEAVE_SOUND.clone(),
    };

    if let Ok(mut effects) = ACTIVE_EFFECTS.lock()
    {
        effects.push(ActiveEffect
        {
            buffer: sound,
            position: 0,
        });
    }
}

/// Stops every sound effect.
pub fn clear_effects() //STOP ALL SOUNDS
{
    if let Ok(mut effects) = ACTIVE_EFFECTS.lock()
    {
        effects.clear();
    }
}

/// Mixes the next sample of every playing effect at the configured volume and drops effects that have ended.
///
/// Takes the lock with `try_lock`, so the realtime callback never blocks.
pub fn play_effects(mixed_sample: &mut f32) //PLAY ALL SOUND IN ACTIVE_EFFECTS
{
    if let Ok(mut effects) = ACTIVE_EFFECTS.try_lock() && !effects.is_empty()
    {
        let gain = options::get_sound_effect_gain();
        effects.retain_mut(|effect|
        {
            if effect.position < effect.buffer.len() //SOUND STILL PLAYING
            {
                *mixed_sample += effect.buffer[effect.position] * gain;
                effect.position += 1;

                true
            } else { false } //SOUND ENDED, REMOVE
        });
    }
}

/// Returns whether any sound effect is playing.
pub fn is_playing() -> bool //CHECK IF ANY SOUND EFFECT IS QUEUED
{
    ACTIVE_EFFECTS.lock().map(|e| !e.is_empty()).unwrap_or(false)
}
