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
#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "client_base")]
pub mod client;

pub mod file;
pub mod codes;

#[cfg(any(feature = "server", feature = "client_screen"))]
pub mod screen;

#[cfg(any(feature = "server", feature = "client_voice"))]
pub mod voice;

use std::
{
    net::TcpStream,
    io::{ Read, Write },
};

use wincode::
{
    config::DefaultConfig,
    SchemaWrite,
    SchemaRead,
};

use zeroize::Zeroizing;

use why2::
{
    consts,
    stream::RexStream,
};

use crate::
{
    crypto,
    network::codes::PacketCode,
    consts::
    {
        Streams,
        self as chat_consts,
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
use crate::config;

//TRAITS
pub trait SequencedPacket: SchemaWrite<DefaultConfig, Src = Self>
{
    fn seq(&self) -> usize;
    fn set_seq(&mut self, seq: usize);
}

//ENUMS
pub enum EncryptionMode<'a> //MODE OF ENCRYPTION
{
    OneShot(Option<&'a chat_consts::SharedKeys>), //ONE-SHOT ENCRYPTION
    Stream(&'a mut RexStream),                    //STREAM ENCRYPTION
}

//STRUCTS
#[derive(SchemaWrite, SchemaRead, Clone)]
pub struct Packet //MESSAGE PACKET (WHAT IS BEING SENT)
{
    pub code: PacketCode, //CONTROL CODE
    pub seq: usize,       //SEQUENCE NUMBER
}

pub struct ReadResult //RESULT OF TCP READ
{
    #[cfg(feature = "server")] pub peer_addr: SocketAddr,
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
pub fn send_tcp //SEND packet TO stream
(
    stream: &mut TcpStream,
    mut packet: impl SequencedPacket,
    encryption_mode: EncryptionMode,
    seq: Option<&mut usize>, //LOCAL/GLOBAL SEQ COUNTER
)
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
                let peer_addr = stream.peer_addr().ok();
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
    let packet_bytes = Zeroizing::new(wincode::serialize(&packet).expect("Encoding packet failed"));

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
                    stream.peer_addr().ok()
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

        //STREAMED ENCRYPTION
        EncryptionMode::Stream(rex_stream) =>
        {
            //CONVERT PACKET BYTES TO i64
            let input_i64 = Zeroizing::new(crypto::bytes_to_i64(&packet_bytes));

            //ENCRYPT
            let mut encrypted_i64 = Zeroizing::new(rex_stream.update(&input_i64).expect("Stream encryption failed"));

            //FLUSH
            encrypted_i64.extend(rex_stream.finalize().expect("Stream finalize failed"));

            //CONVERT ENCRYPTED BYTES BACK TO u8
            let mut final_bytes = crypto::i64_to_bytes(&encrypted_i64);

            //TRUNCATE PADDING
            final_bytes.truncate(packet_bytes.len());

            final_bytes
        },
    };

    //CONVERT ENCRYPTED OUTPUT TO BYTES ([LENGTH][DATA])
    let packet_len = final_bytes.len();
    let mut transmission_packet = Vec::with_capacity(4 + packet_len);
    transmission_packet.extend_from_slice(&(packet_len as u32).to_be_bytes());
    transmission_packet.append(&mut final_bytes);

    //SEND
    if stream.write_all(&transmission_packet).is_ok()
    {
        stream.flush().ok();
    }
}

pub fn read_tcp
(
    streams: &mut Streams,
    encryption_mode: EncryptionMode,
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

        //ALLOW BIG MESSAGES WHEN SPAM PROTECTION IS OFF AND CLIENT IS AUTHENTICATED OR WHEN STREAM IS AUXILIARY
        max_packet_size = if (!spam_protection && authenticated) || auxiliary
        {
            usize::MAX
        } else //SET MAX PACKET SIZE IF SPAM PROTECTION IS ENABLED
        {
            config::read_config("max_packet_size")
        };
    }

    //READ MESSAGE LENGTH
    let mut len_buf = [0u8; 4];
    if streams.0.read_exact(&mut len_buf).is_err() //READ LENGTH
    {
        #[cfg(feature = "server")]
        server::remove_connection(&peer_addr, false, Some("length"));
        return None;
    }
    let len = u32::from_be_bytes(len_buf) as usize;

    //CHECK PACKET SIZE
    #[cfg(feature = "server")]
    if len > max_packet_size
    {
        server::remove_connection(&peer_addr, true, Some("length"));
        return None;
    }

    //READ REST OF PACKET
    let mut decoded_packet = Zeroizing::new(vec![0u8; len]);
    if streams.0.read_exact(&mut decoded_packet).is_err() //READ
    {
        #[cfg(feature = "server")]
        server::remove_connection(&peer_addr, false, Some("length"));
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

        //STREAMED ENCRYPTION
        EncryptionMode::Stream(rex_stream) =>
        {
            let original_len = decoded_packet.len();

            //CONVERT u8 TO i64
            let input_i64 = Zeroizing::new(crypto::bytes_to_i64(&decoded_packet));

            //DECRYPTION
            let mut decrypted_i64 = Zeroizing::new(rex_stream.update(&input_i64).expect("Stream decryption failed"));

            //FLUSH
            decrypted_i64.extend(rex_stream.finalize().expect("Stream finalize failed"));

            //CONVERT BACK TO u8
            let mut output_bytes = Zeroizing::new(crypto::i64_to_bytes(&decrypted_i64));

            //TRUNCATE PADDING
            output_bytes.truncate(original_len);

            output_bytes
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
pub fn send //SEND packet TO stream
(
    stream: &mut TcpStream,
    code: PacketCode,
    keys: Option<&chat_consts::SharedKeys>,
)
{
    send_tcp
    (
        stream,
        Packet
        {
            code,
            seq: 0,
        },
        EncryptionMode::OneShot(keys),
        None,
    );
}

pub fn receive
(
    streams: &mut Streams,
    keys: Option<&chat_consts::SharedKeys>,
    seq: Option<&mut usize> //LOCAL/GLOBAL SEQ
) -> Option<PacketCode>
{
    let read = read_tcp
    (
        streams,
        EncryptionMode::OneShot(keys),
        #[cfg(feature = "server")] false,
    )?;

    //DESERIALIZE AND RETURN
    match wincode::deserialize::<Packet>(&read.data)
    {
        Ok(packet) =>
        {
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

                    //SPAM
                    if packet.code != PacketCode::KeepAlive &&
                        !matches!(packet.code, PacketCode::KeyExchange { .. })
                    {
                        //MESSAGE SIZE (ONLY FOR AUTHENTICATED)
                        if let PacketCode::Message { ref text, .. } = packet.code
                        {
                            disconnect = text.len() > config::read_config("max_message_length");
                        }

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
                        send(&mut streams.1.lock().unwrap(), PacketCode::SpamWarning, shared_key.as_ref());
                    }

                    //TOO MANY VIOLATIONS, BYE
                    if disconnect
                    {
                        server::remove_connection(&read.peer_addr, grace, Some(if !grace { "SEQ" } else { "SPAM" }));
                        return None;
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

                if packet.seq > used_seq || used_seq == 0 || packet.code == PacketCode::Disconnect //VALID
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
            server::remove_connection(&read.peer_addr, false, Some("packet"));

            return None;
        }
    }
}
