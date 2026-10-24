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

//! # Voice Message Container
//!
//! A recorded voice message is a [`Clip`]: a format marker followed by a `wincode`-encoded list
//! of Opus packets, one per [`FRAME_MS`](consts::FRAME_MS) frame. It lives here rather than in
//! the client because the server parses it too — to validate an upload and to measure its
//! duration — without decoding any audio.

use wincode::{ SchemaRead, SchemaWrite };

use crate::
{
    consts as chat_consts,
    network::voice::consts,
};

//CONSTS
/// Marker at the start of every clip.
const MAGIC: &[u8; 8] = b"WHY2VOX\x01"; //FORMAT MARKER

//STRUCTS
/// One recorded voice message.
#[derive(SchemaRead, SchemaWrite)]
pub struct Clip //ONE RECORDED VOICE MESSAGE
{
    /// Channel count, 1 or 2.
    pub channels: u8,         //1 OR 2
    /// Opus packets, one per frame.
    pub frames: Vec<Vec<u8>>, //OPUS PACKETS, ONE PER FRAME
}

//IMPLEMENTATIONS
impl Clip
{
    /// Encodes the clip with its marker.
    pub fn encode(&self) -> Vec<u8> //MARKER + WINCODE
    {
        let mut bytes = MAGIC.to_vec();
        bytes.extend(wincode::config::serialize(self, chat_consts::PACKET_CONFIG).expect("Encoding voice message failed"));

        bytes
    }

    /// Parses and validates a clip.
    ///
    /// A valid clip has 1 or 2 channels and between one and
    /// [`MESSAGE_MAX_FRAMES`](consts::MESSAGE_MAX_FRAMES) frames, each a non-empty packet within
    /// Opus's [`MESSAGE_MAX_PACKET`](consts::MESSAGE_MAX_PACKET) ceiling.
    ///
    /// # Returns
    /// `None` if the marker is missing, the clip does not decode, or it is out of bounds.
    pub fn decode(bytes: &[u8]) -> Option<Self> //PARSE AND CHECK THE BOUNDS
    {
        let clip = wincode::config::deserialize::<Self, _>(bytes.strip_prefix(MAGIC)?, chat_consts::PACKET_CONFIG).ok()?;

        let valid = matches!(clip.channels, 1 | 2)
            && !clip.frames.is_empty()
            && clip.frames.len() <= consts::MESSAGE_MAX_FRAMES
            && clip.frames.iter().all(|frame| !frame.is_empty() && frame.len() <= consts::MESSAGE_MAX_PACKET);

        valid.then_some(clip)
    }

    /// Returns the clip's length in milliseconds.
    pub fn duration(&self) -> u32 //LENGTH IN MS
    {
        self.frames.len() as u32 * consts::FRAME_MS
    }
}

//FUNCTIONS
/// Checks whether a file starts with the clip marker.
pub fn is_clip(header: &[u8]) -> bool //CHECK FOR A VOICE MESSAGE
{
    header.starts_with(MAGIC)
}
