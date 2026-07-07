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

use why2::consts;

use crate::
{
    crypto,
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
#[derive(SchemaWrite, SchemaRead, PartialEq, Clone)]
pub enum MessageCode //CONTROL CODES
{
    KeyExchange,        //SERVER <> CLIENT | KEY EXCHANGE
    Rekey,              //SERVER -> CLIENT | TRIGGER KEY EXCHANGE (USED FOR RE-KEYING)
    Welcome,            //SERVER -> CLIENT | INFORMATIONS
    Disconnect,         //SERVER <> CLIENT | QUIT COMMUNICATION
    Username,           //SERVER -> CLIENT | PICK USERNAME
    PasswordL,          //SERVER -> CLIENT | LOGIN
    PasswordR,          //SERVER -> CLIENT | REGISTER
    Accept,             //SERVER -> CLIENT | START CHATTING
    Join,               //SERVER -> CLIENT | CLIENT JOIN MESSAGE
    Leave,              //SERVER -> CLIENT | CLIENT LEAVE MESSAGE
    List,               //CLIENT <> SERVER | PRINT CONNECTED USERS
    PrivateMessage,     //CLIENT <> SERVER | SEND MESSAGE ONLY TO ONE CLIENT
    PrivateMessageBack, //SERVER -> CLIENT | SEND MESSAGE BACK TO SENDER
    SpamWarning,        //SERVER -> CLIENT | TELL CLIENT TO CALM TF DOWN
    RegisterDisabled,   //SERVER -> CLIENT | REGISTRATION IS DISABLED
    Version,            //SERVER <> CLIENT | ASK CLIENT FOR THEIR PKG VERSION
    Channel,            //SERVER <> CLIENT | CHANNEL CHANGE
    Voice,              //CLIENT <> SERVER | ESTABLISH VOICE CONNECTION
    ChannelJoin,        //SERVER -> CLIENT | CLIENT JOINED VOICE CHANNEL
    ChannelLeave,       //SERVER -> CLIENT | CLIENT LEFT VOICE CHANNEL
    VoiceClients,       //SERVER -> CLIENT | TELL CLIENT ALL CONNECTED VOICE CLIENTS
    Upload,             //CLIENT <> SERVER | REQUEST FILE UPLOAD (OR APPROVAL FROM SERVER)
    Download,           //CLIENT <> SERVER | DOWNLOAD FILE FROM SERVER
    Uploaded,           //SERVER -> CLIENT | ANNOUNCE NEW UPLOADED FILE
    Files,              //CLIENT <> SERVER | LIST UPLOADED FILES
    Screens,            //CLIENT <> SERVER | LIST SCREENSHARES
    Attach,             //CLIENT <> SERVER | ATTACH CLIENT SCREENSHARE
    Deattach,           //CLIENT <> SERVER | DEATTACH CLIENT SCREENSHARE
    UploadLimit,        //SERVER -> CLIENT | MAX CONCURRENT UPLOADS REACHED
    Screen,             //CLIENT <> SERVER | TOGGLE SCREENSHARE
    InvalidUsage,       //SERVER -> CLIENT | INVALID PARAMETERS TO A COMMAND
    InvalidFeature,     //SERVER -> CLIENT | CLIENT REQUESTED DISABLED FEATURE
    KeepAlive,          //SERVER <> CLIENT | A BIT LESS STUPID KEEP-ALIVE
}

//STRUCTS
#[derive(SchemaWrite, SchemaRead, Clone, Default)]
pub struct MessageColors //COLORS OF MESSAGE
{
    pub username_color: Option<u8>, //COLOR OF USERNAME
    pub message_color: Option<u8>,  //COLOR OF MESSAGE
}

#[derive(SchemaWrite, SchemaRead, Clone, Default)]
pub struct MessagePacket //MESSAGE PACKET (WHAT IS BEING SENT)
{
    pub text: Option<String>,           //MESSAGE
    pub username: Option<String>,       //USERNAME (SENT ONLY BY SERVER, AS SERVER DOESN'T ACCEPT USERNAMES FROM CLIENT)
    pub id: Option<usize>,              //ID OF USER
    pub code: Option<MessageCode>,      //CONTROL CODE
    pub colors: MessageColors,          //MESSAGE COLORS
    pub seq: usize,                     //SEQUENCE NUMBER
    pub token: Option<[u8; 32]>,        //CONNECTION TOKEN
}

pub struct ReadResult //RESULT OF TCP READ
{
    #[cfg(feature = "server")] pub peer_addr: SocketAddr,
    pub data: Vec<u8>,
}

//IMPLEMENTATIONS
impl SequencedPacket for MessagePacket
{
    fn seq(&self) -> usize { self.seq }
    fn set_seq(&mut self, seq: usize) { self.seq = seq; }
}

//FUNCTIONS
//PRIVATE
fn obfuscate_data(mut data: Vec<u8>, obfuscation_key: [u8; 32]) -> Vec<u8> //XOR BYTES (USED FOR OBFUSCATION)
{
    for (i, byte) in data.iter_mut().enumerate()
    {
        //XOR EACH BYTE WITH OBFUSCATION KEY
        *byte ^= obfuscation_key[i % obfuscation_key.len()];
    }

    data
}

//PUBLIC
//HANDLERS
pub fn send_tcp //SEND packet TO stream
(
    stream: &mut TcpStream,
    mut packet: impl SequencedPacket,
    keys: Option<&chat_consts::SharedKeys>,
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
    let packet_bytes = wincode::serialize(&packet).expect("Encoding packet failed");

    let mut final_bytes = if let Some(keys) = keys
    {
        crypto::encrypt_packet::<{ consts::DEFAULT_GRID_WIDTH }, { consts::DEFAULT_GRID_HEIGHT }>(packet_bytes, keys)
    } else
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

        obfuscate_data(packet_bytes, key) //NO ENCRYPTION, OBFUSCATE
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
    keys: Option<&chat_consts::SharedKeys>,
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
    let mut decoded_packet = vec![0u8; len];
    if streams.0.read_exact(&mut decoded_packet).is_err() //READ
    {
        #[cfg(feature = "server")]
        server::remove_connection(&peer_addr, false, Some("length"));
        return None;
    }

    //DECRYPT
    if let Some(keys) = keys
    {
        decoded_packet = match crypto::decrypt_packet::<{ consts::DEFAULT_GRID_WIDTH }, { consts::DEFAULT_GRID_HEIGHT }>(decoded_packet, keys)
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
    } else
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

        decoded_packet = obfuscate_data(decoded_packet, key); //NO ENCRYPTION, REMOVE OBFUSCATION
    }

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
    packet: MessagePacket,
    keys: Option<&chat_consts::SharedKeys>,
)
{
    send_tcp
    (
        stream,
        packet,
        keys,
        None,
    );
}

pub fn receive
(
    streams: &mut Streams,
    keys: Option<&chat_consts::SharedKeys>,
    seq: Option<&mut usize> //LOCAL/GLOBAL SEQ
) -> Option<MessagePacket>
{
    let read = read_tcp
    (
        streams,
        keys,
        #[cfg(feature = "server")] false,
    )?;

    //DESERIALIZE AND RETURN
    match wincode::deserialize::<MessagePacket>(&read.data)
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
                    if packet.code != Some(MessageCode::KeepAlive) &&
                        packet.code != Some(MessageCode::KeyExchange)
                    {
                        //MESSAGE SIZE (ONLY FOR AUTHENTICATED)
                        if let Some(text) = &packet.text && conn.is_authenticated()
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
                        server::send_code(&mut streams.1.lock().unwrap(), None, MessageCode::SpamWarning, shared_key.as_ref());
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

                if packet.seq > used_seq || used_seq == 0 || packet.code == Some(MessageCode::Disconnect) //VALID
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

            return Some(packet);
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
