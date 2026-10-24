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

//! # Voice Chat
//!
//! Voice runs over UDP on the same port as the TCP server, as Opus frames of
//! [`FRAME_MS`](consts::FRAME_MS) each.
//!
//! # Wire Format
//! Each datagram is a `wincode`-encoded [`VoicePacket`] sealed with
//! [`crypto::encrypt_packet`] under the session keys, on a smaller
//! [`GRID_WIDTH`](consts::GRID_WIDTH)×[`GRID_HEIGHT`](consts::GRID_HEIGHT) grid to keep the
//! per-packet overhead down. A datagram from the client is prefixed with the sender's session
//! id (8 bytes, big-endian) in the clear, which is how the server picks the keys to decrypt it
//! with; the decrypted packet must then name the same id.
//!
//! # Session
//! Joining voice is approved over TCP, which hands out a token. The client repeats a
//! [`Hello`](VoicePacketCode::Hello) carrying the token until it gets a
//! [`HelloAck`](VoicePacketCode::HelloAck), which binds its UDP address to the session; audio
//! then flows both ways, and the server fans each packet out to the sender's channel.
//!
//! # Submodules
//! - [`consts`] — codec, processing and limit constants.
//! - [`message`] — the voice message container.
//! - `client` / `server` — each side of a voice session.

//MODULES
pub mod consts;
pub mod message;

#[cfg(feature = "client_base")]
pub mod client;

#[cfg(feature = "server")]
pub mod server;

use std::
{
    io::Error,
    net::SocketAddr,
};

use tokio::net::UdpSocket;

use wincode::{ SchemaRead, SchemaWrite };

#[cfg(not(feature = "server"))]
use std::time::Duration;

use crate::
{
    crypto,
    consts::{ self as chat_consts, SharedKeys },
};

#[cfg(feature = "client_base")]
use crate::network::voice::client::options;

#[cfg(not(feature = "server"))]
use crate::options as chat_options;

/// What a voice packet says.
#[derive(SchemaRead, SchemaWrite)]
pub enum VoicePacketCode
{
    //INIT PACKET - CLAIMS THE VOICE SLOT
    /// *Client → server.* Claims the voice session; repeated until acknowledged.
    Hello
    {
        /// The token handed out over the authenticated TCP connection.
        token: [u8; 32], //TOKEN HANDED OUT OVER THE AUTHENTICATED TCP CHANNEL
    },

    //Hello ACCEPTED - THE CLIENT REPEATS UNTIL THIS
    /// *Server → client.* The [`Hello`](VoicePacketCode::Hello) was accepted.
    HelloAck,

    //AUDIO TRANSMIT
    /// *Both ways.* One Opus frame.
    Audio
    {
        /// The encoded frame.
        data: Vec<u8>,            //DATA
        /// The speaker's username, filled in by the server.
        username: Option<String>, //CLIENT USERNAME
    },

    //Pingu without you is just Ping
    /// *Both ways.* A latency probe; the server forwards it to the sender's channel.
    Ping
    {
        /// The prober's local clock.
        timestamp: u128, //LOCAL TIMESTAMP
    },

    //Chinese cousin of Pingu
    /// *Both ways.* A probe answered; the server forwards it to `target_id` alone.
    Pong
    {
        /// Session id of the client that sent the probe.
        target_id: usize, //REMOTE CLIENT ID
        /// The timestamp the probe carried.
        timestamp: u128,  //REMOTE TIMESTAMP
    },
}

/// One voice datagram.
#[derive(SchemaRead, SchemaWrite)]
pub struct VoicePacket //VOICE PACKET (WHAT IS BEING SENT)
{
    /// Session id of the client the packet is from (or about).
    pub id: usize,               //LOCAL CLIENT ID
    /// The payload.
    pub code: VoicePacketCode,   //CODE
    /// Sequence number, strictly increasing per direction.
    pub seq: usize,              //SEQUENCE NUMBER
}

/// Seals and sends one voice packet.
///
/// The sequence number is assigned here: from the recipient's voice connection on the server,
/// from the session counter on the client.
///
/// # Parameters
/// - `socket`: The UDP socket.
/// - `id`: The session id the packet is from or about.
/// - `code`: The payload.
/// - `addr`: The recipient's address (server only).
/// - `recipient_id`: The recipient's session id (server only).
/// - `keys`: The session keys.
///
/// # Returns
/// The number of bytes sent, or the socket error.
pub async fn send //SEND DATA TO UDP
(
    socket: &UdpSocket,
    id: usize,
    code: VoicePacketCode,
    #[cfg(feature = "server")] addr: &SocketAddr,
    #[cfg(feature = "server")] recipient_id: &usize,
    keys: &SharedKeys
) -> Result<usize, Error>
{
    //INIT PACKET
    let mut packet = VoicePacket
    {
        id,
        code,
        seq: 0,
    };

    //SET SERVER SEQ
    #[cfg(feature = "server")]
    {
        if let Some(mut conn) = server::CONNECTIONS.get_mut(recipient_id) &&
            let Some(conn) = conn.0.as_mut()
        {
            packet.seq = conn.server_seq() + 1;
            *conn.server_seq_mut() = packet.seq;
        }
    }

    //SET SEQ
    #[cfg(feature = "client_base")]
    {
        packet.seq = options::get_seq() + 1;
        options::set_seq(packet.seq);
    }

    //SERIALIZE PACKET
    let packet_bytes = wincode::config::serialize(&packet, chat_consts::PACKET_CONFIG).expect("Encoding packet failed");

    //ENCRYPT PACKET
    #[cfg(feature = "server")]
    let encrypted_bytes: Vec<u8>;

    #[cfg(not(feature = "server"))]
    let mut encrypted_bytes: Vec<u8>;

    encrypted_bytes = crypto::encrypt_packet::< { consts::GRID_WIDTH }, { consts::GRID_HEIGHT } >(&packet_bytes, keys);

    //PREPEND ID TO PACKET
    #[cfg(feature = "client_base")]
    {
        encrypted_bytes.splice(0..0, id.to_be_bytes());
    }

    #[cfg(feature = "server")]
    {
        socket.send_to(&encrypted_bytes, addr).await
    }

    #[cfg(not(feature = "server"))]
    {
        socket.send(&encrypted_bytes).await
    }
}

/// Receives the next valid voice packet.
///
/// A datagram that is too short, names an unknown session, fails verification or does not
/// decode is skipped. On the client the wait is polled so it notices voice being turned off.
///
/// # Returns
/// - `Some((packet, sender address))`.
/// - `None` once the client has left voice.
pub async fn receive(socket: &UdpSocket) -> Option<(VoicePacket, SocketAddr)> //RECEIVE UDP PACKET & DECODE
{
    let mut buffer = [0u8; 2048];
    loop //WAIT UNTIL PACKET ARRIVES
    {
        //CHECK FOR VOICE DISABLE
        #[cfg(feature = "client_base")]
        if !options::get_use_voice() { break None; }

        let (len, addr) =
        {
            #[cfg(feature = "server")]
            {
                match socket.recv_from(&mut buffer).await
                {
                    Ok(result) => result,
                    Err(_) => continue
                }
            }

            //POLL SO THE VOICE DISABLE CHECK STAYS RESPONSIVE
            #[cfg(not(feature = "server"))]
            {
                match tokio::time::timeout(Duration::from_millis(consts::RECV_TIMEOUT), socket.recv_from(&mut buffer)).await
                {
                    Ok(Ok(result)) => result,
                    _ => continue
                }
            }
        };

        let buffer_offset: usize;

        //ID THE KEYS WERE PICKED BY (SERVER ONLY)
        #[cfg(feature = "server")]
        let sender_id: usize;

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
                sender_id = id;

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
        let decrypted_bytes = match crypto::decrypt_packet::<{ consts::GRID_WIDTH }, { consts::GRID_HEIGHT }>
            (buffer[buffer_offset..len].to_vec(), &keys)
        {
            Some(d) => d,
            None => continue
        };

        //PACKET ARRIVED, DESERIALIZE
        let packet = match wincode::config::deserialize::<VoicePacket, _>(&decrypted_bytes, chat_consts::PACKET_CONFIG)
        {
            Ok(packet) => packet,
            Err(_) => continue
        };

        #[cfg(feature = "server")]
        if packet.id != sender_id { continue; }

        return Some((packet, addr))
    }
}
