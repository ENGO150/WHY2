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

use std::
{
    io::Cursor,
    sync::
    {
        Arc,
        Mutex,
        LazyLock,
    },
};

use lewton::inside_ogg::OggStreamReader;

use crate::network::voice::consts;

//STRUCTS
struct ActiveEffect
{
    buffer: Arc<Vec<f32>>,
    position: usize,
}

//ENUMS
pub enum SoundEffect
{
    Join,
    Leave,
}

//GLOBAL VARIABLES
static ACTIVE_EFFECTS: LazyLock<Mutex<Vec<ActiveEffect>>> = LazyLock::new(|| Mutex::new(Vec::new()));

//ASSETS
pub static JOIN_SOUND: LazyLock<Arc<Vec<f32>>> = LazyLock::new(|| //DI DING
{
    let bytes = include_bytes!("./join.ogg");
    Arc::new(decode_ogg_from_bytes(bytes))
});

pub static LEAVE_SOUND: LazyLock<Arc<Vec<f32>>> = LazyLock::new(|| //(DI DING).rev()
{
    let bytes = include_bytes!("./leave.ogg");
    Arc::new(decode_ogg_from_bytes(bytes))
});

//FUNCTIONS
//PRIVATE
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

pub fn clear_effects() //STOP ALL SOUNDS
{
    if let Ok(mut effects) = ACTIVE_EFFECTS.lock()
    {
        effects.clear();
    }
}

pub fn play_effects(mixed_sample: &mut f32) //PLAY ALL SOUND IN ACTIVE_EFFECTS
{
    if let Ok(mut effects) = ACTIVE_EFFECTS.try_lock()
    {
        effects.retain_mut(|effect|
        {
            if effect.position < effect.buffer.len() //SOUND STILL PLAYING
            {
                *mixed_sample += effect.buffer[effect.position] * consts::SOUND_EFFECT_VOLUME;
                effect.position += 1;

                true
            } else { false } //SOUND ENDED, REMOVE
        });
    }
}

pub fn is_playing() -> bool //CHECK IF ANY SOUND EFFECT IS QUEUED
{
    ACTIVE_EFFECTS.lock().map(|e| !e.is_empty()).unwrap_or(false)
}
