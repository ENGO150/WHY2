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

//MODULES
pub mod consts;

#[cfg(feature = "client")]
pub mod client;

#[cfg(feature = "server")]
pub mod server;

use std::net::TcpStream;

use wincode::{ SchemaRead, SchemaWrite };

use crate::
{
    consts::{ Streams, SharedKeys },
    network::{ self, SequencedPacket },
};

//STRUCTS
#[derive(SchemaWrite, SchemaRead, Clone)]
pub struct ScreenPacket //SCREEN PACKET
{
    pub frame: Option<Vec<u8>>, //COMPRESSED FRAME
    pub audio: Option<Vec<u8>>, //AUDIO FRAME
    pub seq: usize,             //SEQUENTIAL NUMBER
}

//IMPLEMENTATIONS
impl Default for ScreenPacket
{
    fn default() -> Self
    {
        Self
        {
            frame: None,
            audio: None,
            seq: 0,
        }
    }
}

impl SequencedPacket for ScreenPacket
{
    fn seq(&self) -> usize { self.seq }
    fn set_seq(&mut self, seq: usize) { self.seq = seq; }
}

//FUNCTIONS
//UTILS
pub fn send_frame //SEND frame TO stream
(
    stream: &mut TcpStream,
    packet: ScreenPacket,
    keys: Option<&SharedKeys>,
    #[cfg(feature = "server")] seq: Option<&mut usize>, //LOCAL/GLOBAL SEQ COUNTER
)
{
    network::send_tcp
    (
        stream,
        packet,
        keys,
        #[cfg(feature = "server")] seq,
    );
}

pub fn receive_frame
(
    streams: &mut Streams,
    keys: Option<&SharedKeys>,
    seq: &mut usize //LOCAL/GLOBAL SEQ
) -> Option<ScreenPacket>
{
    let read = network::read_tcp
    (
        streams,
        keys,
        #[cfg(feature = "server")] false,
    )?;

    //DESERIALIZE AND RETURN
    match wincode::deserialize::<ScreenPacket>(&read.data)
    {
        Ok(packet) =>
        {
            //VERIFY SEQUENCE NUMBER (CLIENT)
            if packet.seq > *seq || *seq == 0 //VALID
            {
                *seq = packet.seq;
            } else { return None; }

            Some(packet)
        },

        _ => { None } //TODO: Implement
    }
}
