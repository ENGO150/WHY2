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

//! # Network Layer
//!
//! The packet-level plumbing shared by the client and the server, and the protocol extensions
//! built on it.
//!
//! # Framing
//! Every packet on a TCP socket is a 4-byte big-endian length followed by that many bytes of
//! protected payload. The payload is a `wincode`-encoded [`Packet`] (or another
//! [`SequencedPacket`] on the auxiliary sockets) after one of three transforms, chosen by
//! [`EncryptionMode`]:
//!
//! | Mode                       | Used for                        | Transform                                        |
//! |----------------------------|---------------------------------|--------------------------------------------------|
//! | `OneShot(Some(keys))`      | the main connection             | [`crypto::encrypt_packet`] (encrypt-then-MAC)    |
//! | `Stream(stream)`           | file and screen sockets         | [`RexPacketStream::seal`] (counter-bound EtM)    |
//! | `OneShot(None)`            | the handshake, before any keys  | XOR obfuscation only — **not** encryption        |
//!
//! [`send_tcp`] and [`read_tcp`] never touch a cipher themselves; they hand the payload to
//! [`crypto`].
//!
//! # Sequence Numbers
//! Every packet carries a sequence number that must strictly increase per direction. A packet
//! that does not is dropped by the client and ends the connection on the server, which rejects
//! replayed and reordered packets on top of what the MAC already guarantees.
//!
//! # Server-Side Policing
//! On the server, [`receive`] is the funnel every authenticated packet passes through, so it is
//! also where the per-connection limits are enforced: the packet size ceiling, the message
//! length and minimum message gap (`min_message_delay`, with `SpamWarning`s), and the
//! token-bucket packet rate, which throttles an overdrawn client by delaying its own read loop
//! rather than refusing it.
//!
//! # Submodules
//! - [`codes`] — the [`PacketCode`] enum: every message of the protocol.
//! - [`schema`] — `wincode` schemas for foreign types carried in packets.
//! - [`file`](mod@file) — file and picture transfer on auxiliary sockets.
//! - [`voice`] — voice chat over UDP.
//! - [`screen`] — screen sharing on auxiliary sockets.
//! - `client` / `server` — the connection logic of each side.

//MODULES
#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "client_base")]
pub mod client;

pub mod file;
pub mod codes;
pub mod schema;

#[cfg(any(feature = "server", feature = "client_screen"))]
pub mod screen;

#[cfg(any(feature = "server", feature = "client_voice"))]
pub mod voice;

use tokio::
{
    net::tcp::OwnedWriteHalf,
    io::{ AsyncReadExt, AsyncWriteExt },
};

use wincode::
{
    SchemaWrite,
    SchemaRead,
};

use zeroize::Zeroizing;

use why2::consts;

use crate::
{
    network::codes::PacketCode,
    crypto::{ self, RexPacketStream },
    consts::
    {
        self as chat_consts,
        PacketConfig,
        Streams,
        SharedKeys,
    },
};

#[cfg(feature = "client_base")]
use crate::options;

#[cfg(feature = "server")]
use std::
{
    net::SocketAddr,
    time::{ Instant, Duration },
};

#[cfg(feature = "server")]
use tokio::time;

#[cfg(feature = "server")]
use crate::config;

//TRAITS
/// A packet type that carries a sequence number.
pub trait SequencedPacket: SchemaWrite<PacketConfig, Src = Self>
{
    /// Returns the packet's sequence number.
    fn seq(&self) -> usize;
    /// Sets the packet's sequence number.
    fn set_seq(&mut self, seq: usize);
}

//ENUMS
/// How a packet's payload is protected on the wire.
pub enum EncryptionMode<'a> //MODE OF ENCRYPTION
{
    /// One-shot authenticated encryption under the session keys, or XOR obfuscation when `None`.
    OneShot(Option<&'a SharedKeys>), //ONE-SHOT ENCRYPTION
    /// Counter-bound encrypt-then-MAC stream, for auxiliary sockets.
    Stream(&'a mut RexPacketStream), //STREAM ENCRYPTION (AUTHENTICATED)
}

//STRUCTS
/// A packet on the main connection.
#[derive(SchemaWrite, SchemaRead, Clone)]
pub struct Packet //MESSAGE PACKET (WHAT IS BEING SENT)
{
    /// What the packet says.
    pub code: PacketCode, //CONTROL CODE
    /// Sequence number, strictly increasing per direction.
    pub seq: usize,       //SEQUENCE NUMBER
}

/// The verified, decrypted payload of one TCP read.
pub struct ReadResult //RESULT OF TCP READ
{
    /// Address of the peer the packet came from.
    #[cfg(feature = "server")] pub peer_addr: SocketAddr,
    /// The decrypted, still serialized packet.
    pub data: Zeroizing<Vec<u8>>,
}

//IMPLEMENTATIONS
impl SequencedPacket for Packet
{
    fn seq(&self) -> usize { self.seq }
    fn set_seq(&mut self, seq: usize) { self.seq = seq; }
}

//FUNCTIONS
//PRIVATE
/// XORs bytes with a repeating 32-byte key; obfuscation, not encryption.
fn obfuscate_data(data: &[u8], obfuscation_key: [u8; 32]) -> Vec<u8> //XOR BYTES (USED FOR OBFUSCATION)
{
    let mut result = Vec::with_capacity(data.len());
    for (i, byte) in data.iter().enumerate()
    {
        //XOR EACH BYTE WITH OBFUSCATION KEY
        result.push(*byte ^ obfuscation_key[i % obfuscation_key.len()]);
    }

    result
}

//PUBLIC
//HANDLERS
/// Serializes, protects, frames and writes one packet.
///
/// The packet's sequence number is assigned here: from `seq` if given, else from the session's
/// global counter (the client's session state, or the server's connection entry). Main-connection
/// callers ignore the write error; the read side notices the dead socket. Auxiliary sockets that
/// must act on a failure ([`screen::send_frame`]) propagate it.
///
/// # Parameters
/// - `write_stream`: The socket to write to.
/// - `packet`: The packet; its sequence number is overwritten.
/// - `encryption_mode`: How to protect the payload.
/// - `seq`: A local sequence counter to use instead of the global one (auxiliary sockets).
///
/// # Returns
/// The write's result.
///
/// # Panics
/// If the packet cannot be serialized.
pub async fn send_tcp //SEND packet TO stream
(
    write_stream: &mut OwnedWriteHalf,
    mut packet: impl SequencedPacket,
    encryption_mode: EncryptionMode<'_>,
    seq: Option<&mut usize>, //LOCAL/GLOBAL SEQ COUNTER
) -> std::io::Result<()>
{
    //ADD SEQUENCE NUMBER TO packet (FROM CLIENT)
    #[cfg(feature = "client_base")]
    {
        match seq
        {
            Some(local_seq) =>
            {
                *local_seq += 1;
                packet.set_seq(*local_seq);
            },
            None =>
            {
                packet.set_seq(options::get_seq() + 1);
                options::set_seq(packet.seq());
            }
        }
    }

    //ADD SEQUENCE NUMBER TO packet (FROM SERVER)
    #[cfg(feature = "server")]
    {
        match seq
        {
            Some(local_seq) =>
            {
                *local_seq += 1;
                packet.set_seq(*local_seq);
            },

            None =>
            {
                let peer_addr = write_stream.peer_addr().ok();
                if peer_addr.is_some() && let Some(mut conn) = server::CONNECTIONS.get_mut(&peer_addr.unwrap())
                {
                    if conn.is_authenticated()
                    {
                        packet.set_seq(conn.server_seq().unwrap() + 1);
                        *conn.server_seq_mut().unwrap() = packet.seq();
                    }
                }
            }
        }
    }

    //ENCODE THE PACKET STRUCT TO Vec<u8>
    let packet_bytes = Zeroizing::new(wincode::config::serialize(&packet, chat_consts::PACKET_CONFIG).expect("Encoding packet failed"));

    let mut final_bytes = match encryption_mode
    {
        //KEYS PASSED
        EncryptionMode::OneShot(Some(keys)) =>
        {
            crypto::encrypt_packet::<{ consts::DEFAULT_GRID_WIDTH }, { consts::DEFAULT_GRID_HEIGHT }>(&packet_bytes, keys)
        },

        //NO KEYS
        EncryptionMode::OneShot(None) =>
        {
            let key =
            {
                #[cfg(feature = "server")]
                {
                    write_stream.peer_addr().ok()
                        .and_then(|addr| server::CONNECTIONS.get(&addr))
                        .and_then(|c| c.obfuscation_key().cloned())
                        .unwrap_or_else(|| [0u8; 32])
                }

                #[cfg(feature = "client_base")]
                {
                    options::get_obfuscation_key()
                }
            };

            obfuscate_data(&packet_bytes, key) //NO ENCRYPTION, OBFUSCATE
        },

        //STREAMED ENCRYPTION (ENCRYPT-THEN-MAC)
        EncryptionMode::Stream(rex_stream) =>
        {
            rex_stream.seal(&packet_bytes)
        },
    };

    //CONVERT ENCRYPTED OUTPUT TO BYTES
    let packet_len = final_bytes.len();
    let mut transmission_packet = Vec::with_capacity(4 + packet_len);
    transmission_packet.extend_from_slice(&(packet_len as u32).to_be_bytes());
    transmission_packet.append(&mut final_bytes);

    //SEND
    write_stream.write_all(&transmission_packet).await?;
    write_stream.flush().await
}

/// Reads, unframes and verifies one packet.
///
/// On the server, the length is checked against the connection's limit before anything is
/// allocated — [`MAX_AUXILIARY_PACKET_SIZE`](chat_consts::MAX_AUXILIARY_PACKET_SIZE) on side
/// channels, `max_packet_size` with spam protection on, and
/// [`MAX_PACKET_CEILING`](chat_consts::MAX_PACKET_CEILING) otherwise. A closed socket or an
/// oversized packet removes the connection.
///
/// # Parameters
/// - `streams`: The connection's read half and shared write half.
/// - `encryption_mode`: How the payload is protected.
/// - `auxiliary`: Whether this is a file or screen side channel (server only).
///
/// # Returns
/// - `Some(ReadResult)` with the plaintext payload.
/// - `None` if the socket closed, the packet was too large, or it failed verification.
pub async fn read_tcp
(
    streams: &mut Streams<'_>,
    encryption_mode: EncryptionMode<'_>,
    #[cfg(feature = "server")] auxiliary: bool,
) -> Option<ReadResult>
{
    //SERVER SIDE PACKET SIZE LIMIT
    #[cfg(feature = "server")]
    let max_packet_size: usize;

    //SERVER SIDE SPAM PROTECTION
    #[cfg(feature = "server")]
    let spam_protection = config::read_config::<bool>("spam_protection");

    #[cfg(feature = "server")]
    let peer_addr = streams.0.peer_addr().ok()?; //GET CURRENT PEER ADDRESS

    //SETUP LIMITS
    #[cfg(feature = "server")]
    {
        //CHECK IF CLIENT IS AUTHENTICATED
        let authenticated = server::CONNECTIONS.get(&peer_addr)
            .map(|conn| conn.is_authenticated())
            .unwrap_or(false);

        max_packet_size = if auxiliary
        {
            //SIDE CHANNELS ONLY CARRY CHUNKS AND FRAMES
            chat_consts::MAX_AUXILIARY_PACKET_SIZE
        } else if !spam_protection && authenticated
        {
            //BIG MESSAGES, NOT UNBOUNDED ALLOCATIONS
            chat_consts::MAX_PACKET_CEILING
        } else //SET MAX PACKET SIZE IF SPAM PROTECTION IS ENABLED
        {
            config::read_config("max_packet_size")
        };
    }

    //USE SERVER'S ABSOLUTE MAX PACKET SIZE FOR CLIENT
    #[cfg(feature = "client_base")]
    let max_packet_size = chat_consts::MAX_PACKET_CEILING;

    //READ MESSAGE LENGTH
    let mut len_buf = [0u8; 4];
    if streams.0.read_exact(&mut len_buf).await.is_err() //READ LENGTH
    {
        #[cfg(feature = "server")]
        {
            log::debug!("Socket closed while reading length: {peer_addr}");
            server::remove_connection(&peer_addr, false, Some("length")).await;
        }

        return None;
    }
    let len = u32::from_be_bytes(len_buf) as usize;

    //CHECK PACKET SIZE
    #[cfg(feature = "server")]
    if len > max_packet_size
    {
        log::warn!("Packet over the limit ({len} > {max_packet_size}): {peer_addr}");

        server::remove_connection(&peer_addr, true, Some("length")).await;
        return None;
    }

    #[cfg(feature = "client_base")]
    if len > max_packet_size { return None; } //DISCONNECT

    //READ REST OF PACKET
    let mut decoded_packet = Zeroizing::new(vec![0u8; len]);
    if streams.0.read_exact(&mut decoded_packet).await.is_err() //READ
    {
        #[cfg(feature = "server")]
        {
            log::debug!("Socket closed mid-packet ({len} bytes expected): {peer_addr}");
            server::remove_connection(&peer_addr, false, Some("length")).await;
        }

        return None;
    }

    //DECRYPT
    decoded_packet = match encryption_mode
    {
        //KEYS PASSED
        EncryptionMode::OneShot(Some(keys)) =>
        {
            match crypto::decrypt_packet::<{ consts::DEFAULT_GRID_WIDTH }, { consts::DEFAULT_GRID_HEIGHT }>(decoded_packet.to_vec(), keys)
            {
                Some(d) => d,
                None => //INVALID MAC
                {
                    //LOG IF ON SERVER
                    #[cfg(feature = "server")]
                    log::warn!("HMAC verification failed: {}", peer_addr);

                    return None;
                }
            }
        },

        //NO KEYS
        EncryptionMode::OneShot(None) =>
        {
            let key =
            {
                #[cfg(feature = "server")]
                {
                    server::CONNECTIONS.get(&peer_addr)
                        .and_then(|c| c.obfuscation_key().cloned())
                        .unwrap_or_else(|| [0u8; 32])
                }

                #[cfg(feature = "client_base")]
                {
                    options::get_obfuscation_key()
                }
            };

            Zeroizing::new(obfuscate_data(&decoded_packet, key)) //NO ENCRYPTION, REMOVE OBFUSCATION
        },

        //STREAMED ENCRYPTION (VERIFY-THEN-DECRYPT)
        EncryptionMode::Stream(rex_stream) =>
        {
            match rex_stream.open(&decoded_packet)
            {
                Some(d) => d,
                None => //INVALID MAC
                {
                    //LOG IF ON SERVER
                    #[cfg(feature = "server")]
                    log::warn!("Stream HMAC verification failed: {}", peer_addr);

                    return None;
                }
            }
        }
    };

    //RETURN SERIALIZED PACKET
    Some(ReadResult
    {
        #[cfg(feature = "server")] peer_addr,
        data: decoded_packet,
    })
}

//UTILS
/// Sends a [`PacketCode`] on the main connection with the global sequence counter.
///
/// # Parameters
/// - `write_stream`: The connection's write half.
/// - `code`: What to send.
/// - `keys`: The session keys, or `None` to obfuscate only (before the handshake).
pub async fn send //SEND packet TO stream
(
    write_stream: &mut OwnedWriteHalf,
    code: PacketCode,
    keys: Option<&SharedKeys>,
)
{
    let _ = send_tcp
    (
        write_stream,
        Packet
        {
            code,
            seq: 0,
        },
        EncryptionMode::OneShot(keys),
        None,
    ).await;
}

/// Receives and decodes one [`PacketCode`] on the main connection.
///
/// On the server this also enforces the connection's policies: control characters are
/// stripped from typed text, the sequence number is checked, and the message length, message
/// gap and packet rate limits are applied (see the module documentation). A violation that
/// ends the session removes the connection and returns `None`.
///
/// # Parameters
/// - `streams`: The connection's read half and shared write half.
/// - `keys`: The session keys, or `None` before the handshake.
/// - `seq`: A local sequence counter to use instead of the global one.
///
/// # Returns
/// - `Some(code)` for a valid packet.
/// - `None` if the read failed, the packet did not decode, its sequence number did not
///   increase, or the connection was ended for a policy violation.
pub async fn receive
(
    streams: &mut Streams<'_>,
    keys: Option<&SharedKeys>,
    seq: Option<&mut usize> //LOCAL/GLOBAL SEQ
) -> Option<PacketCode>
{
    let read = read_tcp
    (
        streams,
        EncryptionMode::OneShot(keys),
        #[cfg(feature = "server")] false,
    ).await?;

    //DESERIALIZE AND RETURN
    match wincode::config::deserialize::<Packet, _>(&read.data, chat_consts::PACKET_CONFIG)
    {
        #[allow(unused_mut)]
        Ok(mut packet) =>
        {
            //STRIP CONTROL CHARACTERS
            #[cfg(feature = "server")]
            if let PacketCode::MessageRequest { text, .. } | PacketCode::PrivateMessageRequest { text, .. } |
                PacketCode::Re { message: text } | PacketCode::EditRequest { text, .. } = &mut packet.code &&
                text.chars().any(|c| c.is_control() && c != '\n')
            {
                *text = text.chars().filter_map(|c| match c
                {
                    '\n' => Some('\n'),
                    '\t' => Some(' '),
                    c if c.is_control() => None,
                    c => Some(c),
                }).collect();
            }

            //SPAM, SEQ & LENGTH CHECKS (SERVER)
            #[cfg(feature = "server")]
            {
                //LOCAL SEQ COUNTER
                if let Some(seq) = seq
                {
                    if packet.seq > *seq //VALID SEQ
                    {
                        //SET SEQ TO CURRENT
                        *seq = packet.seq;
                    } else { return None; }
                } else if let Some(mut conn) = server::CONNECTIONS.get_mut(&read.peer_addr)
                {
                    //ACTIVITY TIMER
                    let mut spam_warning = false;
                    let mut shared_key = None;
                    let mut disconnect = false;
                    let mut grace = true;
                    let mut reason = ("spam", "SPAM");
                    let mut wait = Duration::ZERO;

                    //SPAM
                    if let PacketCode::MessageRequest { ref text, .. } |
                        PacketCode::PrivateMessageRequest { ref text, .. } |
                        PacketCode::EditRequest { ref text, .. } = packet.code
                    {
                        //MESSAGE SIZE
                        disconnect = text.len() > config::read_config("max_message_length");

                        if !disconnect && config::read_config("spam_protection") && conn.is_authenticated() &&
                            Instant::now().duration_since(*conn.last_activity()) <
                            Duration::from_millis(config::read_config::<u64>("min_message_delay"))
                        {
                            //INCREMENT SPAM VIOLATIONS
                            *conn.spam_violations_mut().unwrap() += 1;

                            //WARN
                            spam_warning = true;
                            shared_key = conn.keys().cloned();

                            //CHECK FOR TOO MANY VIOLATIONS
                            disconnect = *conn.spam_violations().unwrap() > config::read_config::<usize>("max_message_delay_violations");
                        }

                        *conn.last_activity_mut() = Instant::now(); //RESET last_activity
                    }

                    //PACKET RATE
                    if !disconnect && config::read_config("spam_protection") &&
                        !matches!(packet.code, PacketCode::KeepAlive | PacketCode::KeyExchangeOffer { .. } |
                            PacketCode::KeyExchangeReply { .. })
                    {
                        wait = conn.take_credit(config::read_config("max_packet_rate"),
                            config::read_config("max_packet_burst"));

                        //STILL OVERDRAWN AFTER A WAIT IS A FLOOD
                        if !wait.is_zero() &&
                            conn.throttles().copied().unwrap_or(0) > config::read_config::<usize>("max_packet_rate_violations")
                        {
                            disconnect = true;
                            reason = ("packet rate", "RATE");
                        }
                    }

                    //SEQ
                    if packet.seq > *conn.seq() //VALID SEQ
                    {
                        //SET SEQ TO CURRENT
                        *conn.seq_mut() = packet.seq;
                    } else
                    {
                        //INVALID SEQ
                        grace = false;
                        disconnect = true;
                    }
                    drop(conn); //PREVENT DEADLOCK

                    //SEND WARNING CODE
                    if spam_warning
                    {
                        log::warn!("Spam warning: {}", read.peer_addr);

                        send(&mut *streams.1.lock().await, PacketCode::SpamWarning, shared_key.as_ref()).await;
                    }

                    //TOO MANY VIOLATIONS, BYE
                    if disconnect
                    {
                        log::warn!("Disconnecting ({}): {}", if !grace { "invalid sequence number" } else { reason.0 },
                            read.peer_addr);

                        server::remove_connection(&read.peer_addr, grace, Some(if !grace { "SEQ" } else { reason.1 })).await;
                        return None;
                    }

                    //OVERDRAWN
                    if !wait.is_zero()
                    {
                        log::debug!("Packet held for {}ms: {}", wait.as_millis(), read.peer_addr);

                        time::sleep(wait).await;
                    }
                }
            }

            //VERIFY SEQUENCE NUMBER
            #[cfg(feature = "client_base")] //ON CLIENT
            {
                let used_seq = match seq
                {
                    Some(ref s) => **s,
                    None => options::get_server_seq(),
                };

                if packet.seq > used_seq || used_seq == 0 || matches!(packet.code, PacketCode::Disconnect) //VALID
                {
                    //SET SEQ
                    if let Some(seq) = seq
                    {
                        *seq = packet.seq;
                    } else
                    {
                        options::set_server_seq(packet.seq);
                    }
                } else //INVALID, DISCONNECT
                {
                    return None;
                }
            }

            return Some(packet.code);
        },

        Err(_) =>
        {
            //FORCEFULLY DISCONNECT CLIENT ON INVALID PACKET
            #[cfg(feature = "server")]
            {
                log::warn!("Undecodable packet ({} bytes): {}", read.data.len(), read.peer_addr);
                server::remove_connection(&read.peer_addr, false, Some("packet")).await;
            }

            return None;
        }
    }
}
