/*
This is part of WHY2
Copyright (C) 2022-2025 Václav Šmejkal

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
pub mod options;

#[cfg(feature = "client")]
pub mod client;

#[cfg(feature = "server")]
pub mod server;

use std::
{
    io::Error,
    net::{ UdpSocket, SocketAddr },
};

use wincode::{ SchemaRead, SchemaWrite };

#[derive(SchemaRead, SchemaWrite)]
pub struct VoicePacket //VOICE PACKET (WHAT IS BEING SENT)
{
    pub voice: Vec<u8>,    //MESSAGE
    pub id: Option<usize>, //ID OF USER
    pub seq: usize,        //SEQUENCE NUMBER
}

impl Default for VoicePacket
{
    fn default() -> Self
    {
        VoicePacket
        {
            voice: Vec::new(),
            id: None,
            seq: 0,
        }
    }
}

pub fn send //SEND DATA TO UDP
(
    socket: &UdpSocket,
    packet: VoicePacket,
    #[cfg(feature = "server")] addr: &SocketAddr
) -> Result<usize, Error>
{
    //SERIALIZE PACKET
    let packet_bytes = wincode::serialize(&packet).expect("Encoding packet failed");

    #[cfg(feature = "server")]
    {
        socket.send_to(&packet_bytes, addr)
    }

    #[cfg(not(feature = "server"))]
    {
        socket.send(&packet_bytes)
    }
}

pub fn receive(socket: &UdpSocket) -> (VoicePacket, SocketAddr) //RECEIVE UDP PACKET & DECODE
{
    let mut buffer = [0u8; 2048];
    loop //BLOCK READING UNTIL PACKET ARRIVES
    {
        let (len, addr) = match socket.recv_from(&mut buffer)
        {
            Ok(result) => result,
            Err(_) => continue
        };

        //PACKET ARRIVED, DESERIALIZE
        match wincode::deserialize::<VoicePacket>(&buffer[..len])
        {
            Ok(packet) => return (packet, addr),
            Err(_) => continue
        }
    }
}
