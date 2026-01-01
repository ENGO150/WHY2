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

use crate::chat::
{
    crypto,
    options::SharedKeys,
};

#[cfg(not(feature = "server"))]
use crate::chat::options as chat_options;

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
    mut packet: VoicePacket,
    #[cfg(feature = "server")] addr: &SocketAddr,
    keys: &SharedKeys
) -> Result<usize, Error>
{
    //SET SERVER SEQ
    #[cfg(feature = "server")]
    {
        if let Some(mut conn) = server::CONNECTIONS.get_mut(&packet.id.unwrap()) &&
            let Some(conn) = conn.as_mut()
        {
            packet.seq = conn.server_seq() + 1;
            *conn.server_seq_mut() = packet.seq;
        }
    }

    //SET SEQ
    #[cfg(feature = "client")]
    {
        packet.seq = options::get_seq() + 1;
        options::set_seq(packet.seq);
    }

    //SERIALIZE PACKET
    let packet_bytes = wincode::serialize(&packet).expect("Encoding packet failed");

    //ENCRYPT PACKET
    #[cfg(feature = "server")]
    let encrypted_bytes: Vec<u8>;

    #[cfg(not(feature = "server"))]
    let mut encrypted_bytes: Vec<u8>;

    encrypted_bytes = crypto::encrypt_packet(packet_bytes, keys);

    //PREPEND ID TO PACKET
    #[cfg(feature = "client")]
    {
        let id_be_bytes = packet.id.unwrap().to_be_bytes();
        encrypted_bytes.splice(0..0, id_be_bytes);
    }

    #[cfg(feature = "server")]
    {
        socket.send_to(&encrypted_bytes, addr)
    }

    #[cfg(not(feature = "server"))]
    {
        socket.send(&encrypted_bytes)
    }
}


pub fn receive(socket: &UdpSocket) -> Option<(VoicePacket, SocketAddr)> //RECEIVE UDP PACKET & DECODE
{
    let mut buffer = [0u8; 2048];
    loop //BLOCK READING UNTIL PACKET ARRIVES
    {
        //CHECK FOR VOICE DISABLE
        #[cfg(feature = "client")]
        if !options::get_use_voice() { break None; }

        let (len, addr) = match socket.recv_from(&mut buffer)
        {
            Ok(result) => result,
            Err(_) => continue
        };

        let buffer_offset: usize;

        //GET ID ON SERVER
        let keys =
        {
            #[cfg(feature = "server")]
            {
                if len <= 8 { continue; } //INVALID PACKET

                let id = match buffer[..8].try_into()
                {
                    Ok(id_be_bytes) => usize::from_be_bytes(id_be_bytes),
                    Err(_) => continue
                };

                //REMOVE ID FROM BUFFER
                buffer_offset = 8;

                match server::find_key(&id)
                {
                    Some(k) => k,
                    None => continue
                }
            }

            #[cfg(not(feature = "server"))]
            {
                buffer_offset = 0;
                chat_options::get_keys().unwrap()
            }
        };

        //DECRYPT
        let decrypted_bytes = match crypto::decrypt_packet(buffer[buffer_offset..len].to_vec(), &keys)
        {
            Some(d) => d,
            None => continue
        };

        //PACKET ARRIVED, DESERIALIZE
        return match wincode::deserialize::<VoicePacket>(&decrypted_bytes)
        {
            Ok(packet) => Some((packet, addr)),
            Err(_) => continue
        }
    }
}
