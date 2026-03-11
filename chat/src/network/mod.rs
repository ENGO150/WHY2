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

#[cfg(feature = "client")]
pub mod client;

pub mod voice;

use std::
{
    fs::File,
    path::PathBuf,
    net::TcpStream,
    io::{ Read, Write },
    sync::
    {
        Arc,
        Mutex,
        LazyLock,
    },
};

use wincode::{ SchemaWrite, SchemaRead };

use sha2::Sha256;

use dashmap::DashMap;

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

#[cfg(feature = "client")]
use crate::options;

#[cfg(feature = "server")]
use std::time::{ Instant, Duration };

#[cfg(feature = "server")]
use crate::config;

//STRUCTS
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
    InvalidUsage,       //SERVER -> CLIENT | INVALID PARAMETERS TO A COMMAND
    InvalidFeature,     //SERVER -> CLIENT | CLIENT REQUESTED DISABLED FEATURE
    KeepAlive,          //SERVER <> CLIENT | A BIT LESS STUPID KEEP-ALIVE
}

#[derive(SchemaWrite, SchemaRead, Clone)]
pub struct MessageColors //COLORS OF MESSAGE
{
    pub username_color: Option<u8>, //COLOR OF USERNAME
    pub message_color: Option<u8>,  //COLOR OF MESSAGE
}

#[derive(SchemaWrite, SchemaRead, Clone)]
pub struct FilePayload //FILE CHUNK
{
    pub uid: u64,                 //UPLOAD UID
    pub data: Option<Vec<u8>>,    //BINARY DATA
    pub size: Option<u64>,        //FILE SIZE
    pub filename: Option<String>, //FILE NAME
    pub hash: Option<[u8; 32]>,   //FILE HASH
}

#[derive(SchemaWrite, SchemaRead, Clone)]
pub struct MessagePacket //MESSAGE PACKET (WHAT IS BEING SENT)
{
    pub text: Option<String>,      //MESSAGE
    pub username: Option<String>,  //USERNAME (SENT ONLY BY SERVER, AS SERVER DOESN'T ACCEPT USERNAMES FROM CLIENT)
    pub id: Option<usize>,         //ID OF USER
    pub code: Option<MessageCode>, //CONTROL CODE
    pub colors: MessageColors,     //MESSAGE COLORS
    pub seq: usize,                //SEQUENCE NUMBER
    pub file: Option<FilePayload>, //FILE UPLOADED BY CLIENT
}

pub struct ActiveFileshare //ACTIVE FILE UPLOAD
{
    pub file: File,                                  //TARGET FILE (SERVER-SIDE)
    pub size: u64,                                   //EXPECTED FILE SIZE
    pub current_size: u64,                           //CURRENT SIZE
    pub hash: [u8; 32],                              //SHA256 HASH OF FINAL FILE
    pub hasher: Sha256,                              //HASHER
    pub filename: String,                            //FILENAME
    #[cfg(feature = "server")] pub client_id: usize, //ID OF SENDER
}

//LISTS
pub static ACTIVE_FILESHARES: LazyLock<DashMap<u64, ActiveFileshare>> = LazyLock::new(|| DashMap::new()); //LIST FOR ACTIVE FILE UPLOADS

//IMPLEMENTATIONS
impl Default for MessagePacket //DEFAULT
{
    fn default() -> Self
    {
        Self
        {
            text: None,
            username: None,
            id: None,
            code: None,
            colors: MessageColors
            {
                username_color: None,
                message_color: None,
            },
            seq: 0,
            file: None,
        }
    }
}

impl Default for FilePayload
{
    fn default() -> Self
    {
        Self
        {
            uid: 0,
            data: None,
            size: None,
            filename: None,
            hash: None,
        }
    }
}

//FUNCTIONS
//PRIVATE
fn obfuscate_data(mut data: Vec<u8>) -> Vec<u8> //XOR BYTES (USED FOR OBFUSCATION)
{
    for (i, byte) in data.iter_mut().enumerate()
    {
        //XOR EACH BYTE WITH OBFUSCATION KEY
        *byte ^= chat_consts::OBFUSCATION_KEY[i % chat_consts::OBFUSCATION_KEY.len()];
    }

    data
}

//PUBLIC
pub fn send(stream: &mut TcpStream, mut packet: MessagePacket, keys: Option<&chat_consts::SharedKeys>) //SEND packet TO stream
{
    //ADD SEQUENCE NUMBER TO packet (FROM CLIENT)
    #[cfg(feature = "client")]
    {
        packet.seq = options::get_seq() + 1;
        options::set_seq(packet.seq);
    }

    //ADD SEQUENCE NUMBER TO packet (FROM SERVER)
    #[cfg(feature = "server")]
    {
        let peer_addr = stream.peer_addr().ok();
        if peer_addr.is_some() && let Some(mut conn) = server::CONNECTIONS.get_mut(&peer_addr.unwrap())
        {
            if conn.is_authenticated()
            {
                packet.seq = conn.server_seq().unwrap() + 1;
                *conn.server_seq_mut().unwrap() = packet.seq;
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
        obfuscate_data(packet_bytes) //NO ENCRYPTION, OBFUSCATE
    };

    //CONVERT ENCRYPTED OUTPUT TO BYTES ([LENGTH][DATA])
    let packet_len = final_bytes.len();
    let mut transmission_packet = Vec::with_capacity(4 + packet_len);
    transmission_packet.extend_from_slice(&(packet_len as u32).to_be_bytes());
    transmission_packet.append(&mut final_bytes);

    //SEND
    let _ = stream.write_all(&transmission_packet);
    stream.flush().expect("Flushing stream failed");
}

pub fn receive(streams: &mut Streams, keys: Option<&chat_consts::SharedKeys>) -> Option<MessagePacket>
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

        //ALLOW BIG MESSAGES WHEN SPAM PROTECTION IS OFF AND CLIENT IS AUTHENTICATED
        max_packet_size = if !spam_protection && authenticated
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
        decoded_packet = obfuscate_data(decoded_packet); //NO ENCRYPTION, REMOVE OBFUSCATION
    }

    //DESERIALIZE AND RETURN
    match wincode::deserialize::<MessagePacket>(&decoded_packet)
    {
        Ok(packet) =>
        {
            //SPAM, SEQ & LENGTH CHECKS (SERVER)
            #[cfg(feature = "server")]
            {
                //ACTIVITY TIMER
                let mut spam_warning = false;
                let mut shared_key = None;
                let mut disconnect = false;
                let mut grace = true;

                if let Some(mut conn) = server::CONNECTIONS.get_mut(&peer_addr)
                {
                    //MESSAGE SIZE (ONLY FOR AUTHENTICATED)
                    if let Some(text) = &packet.text && conn.is_authenticated()
                    {
                        disconnect = text.len() > config::read_config("max_message_length");
                    }

                    //SPAM
                    if packet.code != Some(MessageCode::KeepAlive) &&
                        packet.code != Some(MessageCode::KeyExchange) &&
                        packet.file.is_none()
                    {
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
                        server::remove_connection(&peer_addr, grace, Some(if !grace { "SEQ" } else { "SPAM" }));
                        return None;
                    }
                }
            }

            //VERIFY SEQUENCE NUMBER
            #[cfg(feature = "client")] //ON CLIENT
            {
                if packet.seq > options::get_server_seq() || options::get_server_seq() == 0 || packet.code == Some(MessageCode::Disconnect) //VALID
                {
                    //SET SEQ
                    options::set_server_seq(packet.seq);
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
            server::remove_connection(&peer_addr, false, Some("packet"));

            return None;
        }
    }
}

pub fn send_file //CHUNK FILE AND SEND TO STREAM
(
    path: PathBuf,
    write_stream: Arc<Mutex<TcpStream>>,
    uid: u64,
    code: MessageCode,
    keys: Option<&chat_consts::SharedKeys>
)
{
    let mut file = File::open(path).expect("Cannot open file for upload");
    let mut buffer = vec![0; chat_consts::UPLOAD_CHUNK_SIZE];

    //LOOP READING
    loop
    {
        match file.read(&mut buffer)
        {
            Ok(0) => break, //EOF
            Ok(bytes) =>
            {
                //SEND FILE CHUNK
                send(&mut write_stream.lock().unwrap(), MessagePacket
                {
                    code: Some(code.clone()),
                    file: Some(FilePayload
                    {
                        uid,
                        data: Some(buffer[..bytes].to_vec()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }, keys);
            },
            Err(_) => {}, //TODO: Implement
        }
    }
}
