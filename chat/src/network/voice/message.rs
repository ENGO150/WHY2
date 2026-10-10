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

use wincode::{ SchemaRead, SchemaWrite };

use crate::
{
    consts as chat_consts,
    network::voice::consts,
};

//CONSTS
const MAGIC: &[u8; 8] = b"WHY2VOX\x01"; //FORMAT MARKER

//STRUCTS
#[derive(SchemaRead, SchemaWrite)]
pub struct Clip //ONE RECORDED VOICE MESSAGE
{
    pub channels: u8,         //1 OR 2
    pub frames: Vec<Vec<u8>>, //OPUS PACKETS, ONE PER FRAME
}

//IMPLEMENTATIONS
impl Clip
{
    pub fn encode(&self) -> Vec<u8> //MARKER + WINCODE
    {
        let mut bytes = MAGIC.to_vec();
        bytes.extend(wincode::config::serialize(self, chat_consts::PACKET_CONFIG).expect("Encoding voice message failed"));

        bytes
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> //PARSE AND CHECK THE BOUNDS
    {
        let clip = wincode::config::deserialize::<Self, _>(bytes.strip_prefix(MAGIC)?, chat_consts::PACKET_CONFIG).ok()?;

        let valid = matches!(clip.channels, 1 | 2)
            && !clip.frames.is_empty()
            && clip.frames.len() <= consts::MESSAGE_MAX_FRAMES
            && clip.frames.iter().all(|frame| !frame.is_empty() && frame.len() <= consts::MESSAGE_MAX_PACKET);

        valid.then_some(clip)
    }

    pub fn duration(&self) -> u32 //LENGTH IN MS
    {
        self.frames.len() as u32 * consts::FRAME_MS
    }
}

//FUNCTIONS
pub fn is_clip(header: &[u8]) -> bool //CHECK FOR A VOICE MESSAGE
{
    header.starts_with(MAGIC)
}
