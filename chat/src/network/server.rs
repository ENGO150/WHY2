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

use std::
{
    env,
    io::Write,
    path::PathBuf,
    fs::{ self, File },
    collections::HashSet,
    time::{ Instant, Duration },
    net::
    {
        TcpStream,
        SocketAddr,
        Shutdown,
    },
    sync::
    {
        LazyLock,
        Arc,
        Mutex,
    },
};

use zeroize::Zeroizing;

use serde_json::{ json, Value };

use dashmap::DashMap;

use crate::
{
    config,
    consts,
    options,
    misc,
    crypto::{ kex, password },
    network::
    {
        self,
        MessageCode,
        MessagePacket,
        FilePayload,
        voice::server as voice_server,
    },
};

//STRUCTS
pub struct ActiveUpload //ACTIVE FILE UPLOAD
{
    pub file: File,             //TARGET FILE (SERVER-SIDE)
    pub size: u64,              //EXPECTED FILE SIZE
    pub current_size: u64,      //CURRENT SIZE
    pub hash: String,           //SHA256 HASH OF FINAL FILE
    pub filename: String,       //FILENAME
    pub username: String,       //SENDER
}

//ENUMS
#[derive(Clone)]
pub enum Connection //CLIENT CONNECTION (WHAT IS PUSHED TO connections LIST)
{
    Authenticated
    {
        stream: Arc<Mutex<TcpStream>>, //STREAM
        peer_addr: SocketAddr,         //ADDRESS & PORT
        username: String,              //USERNAME
        id: usize,                     //ID OF USER
        keys: consts::SharedKeys,     //SHARED KEYS BETWEEN SERVER AND CLIENT (one to one)
        last_activity: Instant,        //TIME OF LAST MESSAGE (USED FOR TIMEOUT)
        last_key_exchange: Instant,    //TIME OF LAST REKEY
        spam_violations: usize,        //SPAM VIOLATIONS (unexpected, huh?)
        channel: Option<String>,       //CHANNEL
        seq: usize,                    //SEQUENCE NUMBER (CLIENT -> SERVER)
        server_seq: usize,             //SEQUENCE NUMBER (SERVER -> CLIENT)
    },

    NonAuthenticated
    {
        stream: Arc<Mutex<TcpStream>>,     //STREAM
        peer_addr: SocketAddr,             //ADDRESS & PORT
        username: Option<String>,          //CHOSEN USERNAME
        keys: Option<consts::SharedKeys>, //SHARED KEYS
        last_activity: Instant,            //TIME OF LAST MESSAGE
        seq: usize,                        //SEQUENCE NUMBER
    },
}

//IMPLEMENTATIONS
impl Connection
{
    //GET STREAM FROM Connection
    pub fn stream(&self) -> &Arc<Mutex<TcpStream>>
    {
        match self
        {
            Self::Authenticated { stream, .. } => stream,
            Self::NonAuthenticated { stream, .. } => stream,
        }
    }

    //GET PEER ADDR FROM Connection#stream
    pub fn peer_addr(&self) -> &SocketAddr
    {
        match self
        {
            Self::Authenticated { peer_addr, .. } => peer_addr,
            Self::NonAuthenticated { peer_addr, .. } => peer_addr,
        }
    }

    //GET USERNAME FROM Connection
    fn username(&self) -> Option<&String>
    {
        match self
        {
            Self::Authenticated { username, .. } => Some(username),
            Self::NonAuthenticated { username, .. } => username.as_ref(),
        }
    }

    //GET USERNAME FROM Connection AS MUTABLE
    fn username_mut(&mut self) -> &mut Option<String>
    {
        match self
        {
            Self::Authenticated { .. } => panic!("Do not use username_mut() on Authenticated client"),
            Self::NonAuthenticated { username, .. } => username,
        }
    }

    //GET ID FROM Connection
    pub fn id(&self) -> Option<&usize>
    {
        match self
        {
            Self::Authenticated { id, .. } => Some(id),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET SHARED KEYS FROM Connection
    pub fn keys(&self) -> Option<&consts::SharedKeys>
    {
        match self
        {
            Self::Authenticated { keys, .. } => Some(keys),
            Self::NonAuthenticated { keys, .. } => keys.as_ref(),
        }
    }

    //GET LAST ACTIVITY FROM Connection
    pub fn last_activity(&self) -> &Instant
    {
        match self
        {
            Self::Authenticated { last_activity, .. } => last_activity,
            Self::NonAuthenticated { last_activity, .. } => last_activity,
        }
    }

    //GET LAST ACTIVITY FROM Connection AS MUTABLE
    pub fn last_activity_mut(&mut self) -> &mut Instant
    {
        match self
        {
            Self::Authenticated { last_activity, .. } => last_activity,
            Self::NonAuthenticated { last_activity, .. } => last_activity,
        }
    }

    //GET LAST KEY EXCHANGE FROM Connection
    pub fn last_key_exchange(&self) -> Option<&Instant>
    {
        match self
        {
            Self::Authenticated { last_key_exchange, .. } => Some(last_key_exchange),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET SPAM VIOLATIONS FROM Connection
    pub fn spam_violations(&self) -> Option<&usize>
    {
        match self
        {
            Self::Authenticated { spam_violations, .. } => Some(spam_violations),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET SPAM VIOLATIONS FROM Connection AS MUTABLE
    pub fn spam_violations_mut(&mut self) -> Option<&mut usize>
    {
        match self
        {
            Self::Authenticated { spam_violations, .. } => Some(spam_violations),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET CHANNEL
    pub fn channel(&self) -> &Option<String>
    {
        match self
        {
            Self::Authenticated { channel, .. } => channel,
            Self::NonAuthenticated { .. }  => &None,
        }
    }

    //GET LAST SEQUENCE NUMBER
    pub fn seq(&self) -> &usize
    {
        match self
        {
            Self::Authenticated { seq, .. } => seq,
            Self::NonAuthenticated { seq, .. } => seq,
        }
    }

    //GET LAST SEQUENCE NUMBER AS MUTABLE
    pub fn seq_mut(&mut self) -> &mut usize
    {
        match self
        {
            Self::Authenticated { seq, .. } => seq,
            Self::NonAuthenticated { seq, .. } => seq,
        }
    }

    //GET LAST SERVER SEQUENCE NUMBER
    pub fn server_seq(&self) -> Option<&usize>
    {
        match self
        {
            Self::Authenticated { server_seq, .. } => Some(server_seq),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET LAST SERVER SEQUENCE NUMBER AS MUTABLE
    pub fn server_seq_mut(&mut self) -> Option<&mut usize>
    {
        match self
        {
            Self::Authenticated { server_seq, .. } => Some(server_seq),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //CHECK IF CONNECTION IS INACTIVE
    fn is_inactive(&self, now: Option<Instant>) -> bool
    {
        now.unwrap_or(Instant::now()).duration_since(*self.last_activity()) > Duration::from_secs(config::read_config::<u64>("communication_time"))
    }

    //CLONE STREAM
    fn cloned_stream(&self) -> Option<TcpStream>
    {
        self.stream().lock().ok()?.try_clone().ok()
    }

    //IS AUTHENTICATED
    pub fn is_authenticated(&self) -> bool
    {
        match self
        {
            Self::Authenticated { .. } => true,
            Self::NonAuthenticated { .. } => false,
        }
    }
}

//LISTS
pub static CONNECTIONS: LazyLock<DashMap<SocketAddr, Connection>> = LazyLock::new(|| DashMap::new()); //LIST FOR EACH CLIENT CONNECTION
pub static ACTIVE_UPLOADS: LazyLock<DashMap<u64, ActiveUpload>> = LazyLock::new(|| DashMap::new());   //LIST FOR ACTIVE FILE UPLOADS

//PRIVATE
fn untrusted_read(stream: &mut TcpStream, code: MessageCode, keys: Option<&consts::SharedKeys>) -> Option<MessagePacket>
{
    //SET READ TIMEOUT FOR ZOMBIE CONNECTIONS
    stream.set_read_timeout(Some(Duration::from_millis(2000))).expect("Failed to set read timeout");

    let mut invalid_packets = 0; //INVALID KEY EXCHANGE PACKETS COUNTER

    //WAIT FOR KeyExchange
    let message = loop
    {
        //READ MESSAGE
        let received = match network::receive(stream, keys)
        {
            Some(r) => r,
            None => return None
        };

        if received.code == Some(code.clone()) && !received.text.is_none() { break received; }

        //CHECK INVALID PACKETS COUNTER
        if invalid_packets == 3 { return None; }
        invalid_packets += 1; //INCREMENT
    };

    //REMOVE READ TIMEOUT
    stream.set_read_timeout(None).expect("Failed to unset read timeout");

    Some(message)
}

fn key_exchange(peer_addr: &SocketAddr, keys: &mut consts::SharedKeys) //KEY EXCHANGE FOR SERVER-SIDE
{
    //GET CONNECTION
    let mut stream = match CONNECTIONS.get(peer_addr)
    {
        Some(conn) => conn.cloned_stream().unwrap(),
        None => return
    };

    //LOAD KEYS
    let (sk, pk) = kex::get_server_keys();          //ECC
    let (pq_sk, pq_pk) = kex::get_server_pq_keys(); //PQ (ML-KEM)

    //PREPARE PAYLOAD
    let payload = serde_json::json!
    ({
        "ecc": pk,
        "pq": pq_pk,
    }).to_string();

    //SEND ECC PUBKEY TO CLIENT
    network::send(&mut stream, MessagePacket
    {
        text: Some(payload),
        code: Some(MessageCode::KeyExchange),
        ..Default::default()
    }, None);

    //READ FROM UNTRUSTED CLIENT
    let message = match untrusted_read(&mut stream, MessageCode::KeyExchange, None)
    {
        Some(r) => r,
        None => return
    };

    //DERIVE SHARED KEYS
    let derived_keys = (||
    {
        //PARSE CLIENT RESPONSE (JSON)
        let client_response: Value = serde_json::from_str(message.text.as_ref().unwrap()).ok()?;
        let client_ecc_pk = client_response["ecc"].as_str()?;
        let client_pq_ciphertext = client_response["pq"].as_str()?;

        //DECAPSULATE PQ
        let pq_secret = kex::decapsulate_pq(&pq_sk, client_pq_ciphertext)?;

        //DERIVE
        kex::derive_shared_secret(sk, client_ecc_pk.to_string(), pq_secret)
    })();

    //UPDATE CLIENT KEYS
    if let Some(new_keys) = derived_keys
    {
        update_client_keys(peer_addr, &new_keys);
        *keys = new_keys;
    }
}

fn send_welcome_packet(stream: &mut TcpStream, keys: &consts::SharedKeys) //send welcome packet you idiot
{
    //CREATE JSON WITH ALL THE INFO
    let welcome_json = json!(
    {
        "min_pass": config::read_config::<usize>("min_password_length"),
        "max_uname": config::read_config::<usize>("max_username_length"),
        "min_uname": config::read_config::<usize>("min_username_length"),
        "server_name": config::read_config::<String>("server_name"),
    }).to_string();

    //SEND
    send_code(stream, Some(welcome_json), MessageCode::Welcome, Some(keys));
}

fn send_to_all(packet: MessagePacket) //SEND PACKET TO ALL CLIENTS
{
    //GET SENDER'S CHANNEL
    let channel = packet.username.as_ref().and_then(|username|
    {
        CONNECTIONS.iter()
            .find(|entry| entry.username() == Some(username))
            .and_then(|entry| entry.channel().clone())
    });

    //COLLECT EACH CLIENT IN SAME CHANNEL
    let entries: Vec<Connection> = CONNECTIONS.iter().filter_map(|entry|
    {
        match entry.value()
        {
            Connection::Authenticated { channel: c, .. } if c == &channel =>
            {
                //FOUND, COLLECT
                Some(entry.value().clone())
            },
            _ => None,
        }
    }).collect();

    for ref entry in entries
    {
        if let Ok(mut stream) = entry.stream().lock()
        {
            network::send(&mut *stream, packet.clone(), entry.keys());
        }
    }
}

fn get_upload_dir(username: &str) -> PathBuf //GET USER'S TEMP DIR FOR UPLOAD
{
    env::temp_dir().join(consts::UPLOADS_DIR).join(username)
}

//PUBLIC
pub fn remove_connection(peer_addr: &SocketAddr, grace: bool) //REMOVE CONNECTION BY TcpStream
{
    //REMOVE CONNECTION
    let connection = match CONNECTIONS.remove(peer_addr)
    {
        Some((_, conn)) => conn,
        None => return
    };

    let stream = connection.stream();

    //DISCONNECT
    if let Ok(mut stream) = stream.lock()
    {
        //SEND DISCONNECT CODE IF GRACEFUL
        if grace
        {
            send_code(&mut stream, None, MessageCode::Disconnect, connection.keys());
        }

        //SHUTDOWN STREAM
        stream.shutdown(Shutdown::Both).ok();
    }

    //AUTHENTICATED ACTIONS
    if connection.is_authenticated()
    {
        //DISCONNECT FROM VOICE CHAT
        if options::voice_chat_enabled()
        {
            voice_server::remove_connection(connection.id().unwrap());
        }

        //SEND LEAVE MESSAGE
        send_to_all(MessagePacket
        {
            text: Some(connection.username().unwrap().to_string()),
            username: Some(config::read_config::<String>("server_username")),
            id: connection.id().copied(),
            code: Some(MessageCode::Leave),

            ..Default::default()
        });
    }

    log::info!("Close connection: {}", peer_addr);
}

fn user_connected(username: &str) -> bool //CHECK IF CLIENT WITH username IS CONNECTED
{
    CONNECTIONS.iter().any(|conn|
    {
        conn.username().map_or(false, |u| u == &username.to_string())
    })
}

fn get_latest_id() -> usize
{
    //GET HashSet OF IDS
    let ids: HashSet<usize> = CONNECTIONS.iter().filter_map(|conn|
    {
        if let Some(id) = conn.id()
        {
            Some(*id)
        } else
        {
            None
        }
    }).collect();

    //GET SMALLEST UNUSED ID
    for i in 0..
    {
        if !ids.contains(&i) //ID FOUND, RETURN
        {
            return i;
        }
    }

    unreachable!("what the fuck");
}

fn update_client_keys(peer_addr: &SocketAddr, keys: &consts::SharedKeys) //ADD KEY TO NonAuthenticated CLIENT AFTER KEY EXCHANGE
{
    //UPDATE CONNECTION
    CONNECTIONS.alter(peer_addr, |_, old_connection|
    {
        match old_connection
        {
            Connection::NonAuthenticated { stream, seq, peer_addr, .. } =>
            {
                Connection::NonAuthenticated
                {
                    stream: stream,
                    peer_addr: peer_addr,
                    username: None,
                    keys: Some(keys.to_owned()),
                    last_activity: Instant::now(),
                    seq: seq,
                }
            },

            Connection::Authenticated { stream, username, id, last_activity, spam_violations, channel, seq, server_seq, peer_addr, .. } =>
            {
                Connection::Authenticated
                {
                    stream: stream,
                    peer_addr: peer_addr,
                    username: username,
                    id: id,
                    keys: keys.to_owned(),
                    last_activity: last_activity,
                    last_key_exchange: Instant::now(),
                    spam_violations: spam_violations,
                    channel: channel,
                    seq: seq,
                    server_seq: server_seq,
                }
            }
        }
    });
}

fn authenticate_client(peer_addr: &SocketAddr, username: &str, id: usize) //MOVE CONNECTION FROM NonAuthenticated TO Authenticated
{
    //UPDATE CONNECTION
    CONNECTIONS.alter(&peer_addr, |_, old_connection|
    {
        Connection::Authenticated
        {
            stream: old_connection.stream().clone(),
            peer_addr: *old_connection.peer_addr(),
            username: username.to_string(),
            id: id,
            keys: old_connection.keys().unwrap().to_owned(),
            last_activity: Instant::now() - Duration::from_millis(config::read_config("min_message_delay")),
            last_key_exchange: *old_connection.last_key_exchange().unwrap_or(&Instant::now()),
            spam_violations: 0,
            channel: None,
            seq: *old_connection.seq(),
            server_seq: 0,
        }
    });

    log::info!("Authenticate connection: {}", peer_addr);
}

fn update_client_channel(peer_addr: &SocketAddr, channel: &Option<String>) //MOVE CLIENT TO CHANNEL
{
    //UPDATE CONNECTION
    CONNECTIONS.alter(&peer_addr, |_, old_connection|
    {
        Connection::Authenticated
        {
            stream: old_connection.stream().clone(),
            peer_addr: *old_connection.peer_addr(),
            username: old_connection.username().unwrap().clone(),
            id: *old_connection.id().unwrap(),
            keys: old_connection.keys().unwrap().to_owned(),
            last_activity: Instant::now(),
            last_key_exchange: *old_connection.last_key_exchange().unwrap(),
            spam_violations: *old_connection.spam_violations().unwrap(),
            channel: channel.clone(),
            seq: *old_connection.seq(),
            server_seq: *old_connection.server_seq().unwrap(),
        }
    });
}

fn ask_version(stream: &mut TcpStream, keys: &consts::SharedKeys) -> Option<String> //ASK CLIENT FOR VERSION
{
    //ASK FOR VERSION
    send_code(stream, Some(misc::get_version().to_string()), MessageCode::Version, Some(keys));

    //READ FROM UNTRUSTED CLIENT
    untrusted_read(stream, MessageCode::Version, Some(keys))?.text
}

fn send_voice_clients(stream: &mut TcpStream, keys: &consts::SharedKeys, id: usize)
{
    //FIND CHANNEL
    let sender_channel = match CONNECTIONS.iter().find(|e| e.value().id() == Some(&id))
    {
        Some(entry) => entry.value().channel().clone(),
        None => return,
    };

    let mut clients: Vec<(usize, String)> = Vec::new();

    //COLLECT VOICE CLIENTS
    for entry in CONNECTIONS.iter()
    {
        let conn = entry.value();

        let uid = match conn.id()
        {
            Some(i) => *i,
            None => continue
        };

        //FILTERS
        if uid == id { continue; } // IGNORE SELF
        if conn.channel() != &sender_channel { continue; } //IGNORE ANOTHER CHANNELS

        //CHECK IF IS IN VOICE
        if voice_server::CONNECTIONS.contains_key(&uid)
        {
            //ADD USERNAMES
            if let Some(username) = conn.username()
            {
                 clients.push((uid, username.clone()));
            }
        }
    }

    //SEND
    network::send(stream, MessagePacket
    {
        text: Some(json!(clients).to_string()),
        code: Some(MessageCode::VoiceClients),
        ..Default::default()
    }, Some(keys));
}

//PUBLIC
pub fn send_code(stream: &mut TcpStream, text: Option<String>, code: MessageCode, keys: Option<&consts::SharedKeys>) //SEND CODE TO CLIENT
{
    network::send(stream, MessagePacket
    {
        text: text,
        code: Some(code),
        ..Default::default()
    }, keys);
}

pub fn listen_client(stream: &mut TcpStream) //CLIENT -> SERVER COMMUNICATION
{
    let peer_addr = match stream.peer_addr()
    {
        Ok(addr) => addr,
        Err(_) => return,
    };

    log::info!("New connection: {}", peer_addr);

    //PUSH NEW CONNECTION
    CONNECTIONS.insert(peer_addr, Connection::NonAuthenticated
    {
        stream: Arc::new(Mutex::new(stream.try_clone().expect("Failed to clone client stream"))),
        peer_addr: peer_addr,
        username: None,
        keys: None,
        last_activity: Instant::now(),
        seq: 0,
    });

    //GET ENCRYPTION & MAC KEYS
    let mut keys = (Zeroizing::new(vec![]), Zeroizing::new(vec![]));
    key_exchange(&peer_addr, &mut keys);

    //CHECK FOR VALID KEYS
    if keys.0.is_empty() || keys.1.is_empty()
    {
        return remove_connection(&peer_addr, false)
    }

    //ASK CLIENT FOR THEIR PACKAGE VERSION
    if config::read_config("check_client_version")
    {
        let version = ask_version(stream, &keys);
        if version.is_none() || version != Some(misc::get_version().to_string())
        {
            return remove_connection(&peer_addr, true);
        }
    }

    //SEND PACKET WITH REQUIRED SERVER INFO
    send_welcome_packet(stream, &keys);

    //GET USERNAME FROM USER
    let mut username: Option<String> = None; //USER ENTERED USERNAME

    //USERNAME CONFIGS
    let max_tries = config::read_config::<usize>("max_auth_tries"); //MAX n
    let min_len = config::read_config::<usize>("min_username_length");
    let max_len = config::read_config::<usize>("max_username_length");

    //TELL USER IF REGISTRATIONS ARE DISABLED
    let disabled_registration = !config::read_config::<bool>("allow_register");
    if disabled_registration
    {
        send_code(stream, None, MessageCode::RegisterDisabled, Some(&keys));
    }

    //ASK n TIMES
    for _ in 0..max_tries
    {
        //SEND PICK_USERNAME CODE
        send_code(stream, None, MessageCode::Username, Some(&keys));

        match network::receive(stream, Some(&keys))
        {
            //USERNAME CONDITIONS MET, BREAK LOOP
            Some(r) =>
            {
                if let Some(uname) = r.text
                {
                    if uname.len() >= min_len && uname.len() <= max_len && uname.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') && !user_connected(&uname)
                    {
                        username = Some(uname);
                        break;
                    }
                }
            },

            None => return remove_connection(&peer_addr, false),
        }
    }

    //NO USERNAME RECEIVED, DISCONNECT CLIENT
    if username.is_none()
    {
        return remove_connection(&peer_addr, true);
    }

    let username = username.unwrap();

    //UPDATE USERNAME IN NonAuthenticated
    if let Some(mut conn) = CONNECTIONS.get_mut(&peer_addr)
    {
        if !conn.is_authenticated()
        {
            //UPDATE
            *conn.username_mut() = Some(username.clone());
        }
    }

    let user_exists = config::server_users_contains(&username);

    //ASK FOR PASSWORD
    if !user_exists && !disabled_registration //REGISTRATION (ENTER "FAKE" LOGIN ON DISABLED REGISTER)
    {
        let max_tries = config::read_config::<usize>("max_auth_tries"); //MAX n
        let mut password: Option<String> = None;

        //KEEP ASKING FOR PASSWORD n TIMES
        for _ in 0..max_tries
        {
            //SEND REGISTER CODE
            send_code(stream, None, MessageCode::PasswordR, Some(&keys));

            //WAIT FOR ANSWER
            match network::receive(stream, Some(&keys))
            {
                Some(r) =>
                {
                    if let Some(pass) = r.text
                    {
                        //CHECK LENGTH
                        if pass.len() > config::read_config("min_password_length")
                        {
                            password = Some(pass);
                            break;
                        }
                    }
                },

                None => return remove_connection(&peer_addr, false)
            };
        }

        if password.is_none()
        {
            return remove_connection(&peer_addr, true);
        }

        //SAVE PASSWORD
        config::server_users_write(&username, &password::hash_password(&password.unwrap()));
    } else //LOGIN
    {
        //SEND LOGIN CODE
        send_code(stream, None, MessageCode::PasswordL, Some(&keys));

        //WAIT FOR ANSWER
        let response = match network::receive(stream, Some(&keys))
        {
            Some(r) => r,
            None => return remove_connection(&peer_addr, false),
        };

        //INVALID PASSWORD (OR FAKE LOGIN), DISCONNECT CLIENT
        if !user_exists || response.text.is_none() || !password::compare_password_hash(&config::server_users_config(&username), &response.text.unwrap())
        {
            return remove_connection(&peer_addr, true);
        }
    }

    //GENERATE ID FOR CLIENT
    let id = get_latest_id();

    //AUTHENTICATE CLIENT
    authenticate_client(&peer_addr, &username, id);

    //TELL CLIENT TO START CHATTING
    send_code(stream, Some(id.to_string()), MessageCode::Accept, Some(&keys));

    //SEND JOIN MESSAGE
    send_to_all(MessagePacket
    {
        text: Some(username.clone()),
        username: Some(config::read_config::<String>("server_username")),
        code: Some(MessageCode::Join),
        ..Default::default()
    });

    //LOOP READING
    loop
    {
        //READ
        let read = match network::receive(stream, Some(&keys))
        {
            Some(r) => r,
            None => return
        };

        //REKEY EVERY 10 MINUTES
        if Instant::now().duration_since(*CONNECTIONS.get(&peer_addr).unwrap().last_key_exchange().unwrap()) >=
            Duration::from_secs(consts::REKEY_INTERVAL)
        {
            //INFORM CLIENT ABOUT REKEYING
            send_code(stream, None, MessageCode::Rekey, Some(&keys));
            key_exchange(&peer_addr, &mut keys); //INIT REKEY
        }

        //CLIENT CODES
        if let Some(code) = read.code
        {
            match code
            {
                //CLIENT QUITS
                MessageCode::Disconnect =>
                {
                    //DISCONNECT CLIENT
                    return remove_connection(&peer_addr, true);
                },

                //VOICE CALL
                MessageCode::Voice =>
                {
                    //CHECK DISABLED FEATURE
                    if !options::voice_chat_enabled()
                    {
                        send_code(stream, None, MessageCode::InvalidFeature, Some(&keys));
                    } else
                    {
                        //ACKNOWLEDGE
                        send_code(stream, None, MessageCode::Voice, Some(&keys));

                        if !voice_server::CONNECTIONS.contains_key(&id) //IS NOT USING VOICE
                        {
                            //ADD CLIENT ID TO VOICE CONNECTIONS MAP
                            voice_server::CONNECTIONS.insert(id, (None, username.clone()));

                            //SEND CODE TO CHANNEL
                            if options::voice_chat_enabled()
                            {
                                send_to_all(MessagePacket
                                {
                                    code: Some(MessageCode::ChannelJoin),
                                    username: Some(username.clone()),
                                    id: Some(id),
                                    ..Default::default()
                                });
                            }

                            //SEND CONNECTED CLIENTS
                            send_voice_clients(stream, &keys, id);
                        } else //IS USING VOICE
                        {
                            //SEND CODE TO LAST CHANNEL
                            if options::voice_chat_enabled()
                            {
                                send_to_all(MessagePacket
                                {
                                    code: Some(MessageCode::ChannelLeave),
                                    username: Some(username.clone()),
                                    id: Some(id),
                                    ..Default::default()
                                });
                            }

                            //REMOVE FROM VOICE
                            voice_server::remove_connection(&id);
                        }
                    }
                },

                //SWITCH CHANNEL
                MessageCode::Channel =>
                {
                    //CHECK PARAMETER VALIDITY
                    if read.text.iter().all(|s| !s.is_empty() && s.len() <= config::read_config("max_channel_length") && s.chars().all(|c| c.is_ascii_alphanumeric() && c != ' '))
                    {
                        //SEND ChannelLeave CODE TO OLD CHANNEL
                        if options::voice_chat_enabled()
                        {
                            send_to_all(MessagePacket
                            {
                                code: Some(MessageCode::ChannelLeave),
                                username: Some(username.clone()),
                                id: Some(id),
                                ..Default::default()
                            });
                        }

                        //UPDATE CHANNEL
                        update_client_channel(&peer_addr, &read.text);
                        send_code(stream, read.text, MessageCode::Channel, Some(&keys));

                        //SEND CODE TO CHANNEL
                        if options::voice_chat_enabled() && voice_server::CONNECTIONS.contains_key(&id)
                        {
                            send_to_all(MessagePacket
                            {
                                code: Some(MessageCode::ChannelJoin),
                                username: Some(username.clone()),
                                id: Some(id),
                                ..Default::default()
                            });
                        }

                        //SEND CONNECTED CLIENTS
                        send_voice_clients(stream, &keys, id);
                    } else //INVALID CHANNEL
                    {
                        //SEND InvalidUsage CODE
                        send_code(stream, None, MessageCode::InvalidUsage, Some(&keys));
                    }
                },

                //CLIENT REQUESTED LIST OF ONLINE USERS
                MessageCode::List =>
                {
                    let mut user_list = Vec::new();

                    //ITERATE OVER CONNECTIONS, CREATE JSON OF USERS
                    for connection_enum in CONNECTIONS.iter()
                    {
                        if let Connection::Authenticated { username: uname, id: user_id, channel, .. } = connection_enum.value()
                        {
                            user_list.push(json!({ "username": uname, "id": user_id, "channel": channel }));
                        }
                    }

                    //SEND LIST BACK TO CLIENT
                    network::send(stream, MessagePacket
                    {
                        text: Some(json!(user_list).to_string()), //BUILD JSON FROM user_list
                        code: Some(MessageCode::List),
                        ..Default::default()
                    }, Some(&keys));
                },

                //FILE UPLOAD
                MessageCode::Upload =>
                {
                    let mut valid = false;

                    if let Some(file) = read.file //CHECK FOR FILE PAYLOAD
                    {
                        //CHECK IF UPLOAD ALREADY STARTED
                        if let Some(mut active) = ACTIVE_UPLOADS.get_mut(&file.uid) &&
                            let Some(chunk_data) = file.data
                        {
                            //WRITE
                            if active.file.write_all(&chunk_data).is_ok()
                            {
                                //UPDATE SIZE
                                active.current_size += chunk_data.len() as u64;
                                if active.current_size <= active.size { valid = true; }

                                //CHECK SIZE
                                if active.current_size == active.size
                                {
                                    //REMOVE ACTIVE UPLOAD
                                    drop(active);
                                    ACTIVE_UPLOADS.remove(&file.uid);

                                    //TODO: Implement file renaming
                                }
                            }
                        } else //NEW UPLOAD, VERIFY SIZE
                        {
                            if let Some(size) = file.size &&
                                let Some(hash) = file.hash &&
                                let Some(filename) = file.filename &&
                                size / 1_000_000 <= config::read_config::<u64>("max_upload_size")
                            {
                                //GENERATE RANDOM UID
                                let uid = rand::random::<u64>();

                                //CREATE TEMP UPLOAD DIRECTORY
                                let temp_dir = get_upload_dir(&username);
                                fs::create_dir_all(&temp_dir).expect("Creating upload temp directory failed");

                                //ADD ACTIVE UPLOAD (ALSO CREATE THE FILE)
                                ACTIVE_UPLOADS.insert(uid, ActiveUpload
                                {
                                    file: File::create_new(temp_dir.join(uid.to_string())).expect("Creating upload file failed"),
                                    size,
                                    current_size: 0,
                                    hash: hash.clone(),
                                    filename,
                                    username: username.clone(),
                                });

                                //SEND APPROVAL TO CLIENT
                                network::send(stream, MessagePacket
                                {
                                    code: Some(MessageCode::Upload),
                                    file: Some(FilePayload
                                    {
                                        uid,
                                        hash: Some(hash),
                                        ..Default::default()
                                    }),
                                    ..Default::default()
                                }, Some(&keys));
                                valid = true;
                            }
                        }
                    }

                    //NO FILE PAYLOAD, HUH?
                    if !valid
                    {
                        return remove_connection(&peer_addr, true);
                    }
                },

                //PRIVATE MESSAGE
                MessageCode::PrivateMessage =>
                {
                    let parse_result = read.text.as_ref().and_then(|text|
                    {
                        let (id_str, message) = text.split_once(' ')?;
                        let recipient_id = id_str.parse::<usize>().ok()?;

                        //FIND RECIPIENT BY ID
                        let recipient_addr = CONNECTIONS.iter()
                            .find(|entry| entry.value().id() == Some(&recipient_id))
                            .map(|entry| *entry.key())?;

                        Some((recipient_addr, recipient_id, message.to_string()))
                    });

                    if let Some((recipient_addr, recipient_id, private_message)) = parse_result
                    {
                        //SEND TO RECIPIENT (IF NOT SELF-MESSAGE)
                        if recipient_id != id
                        {
                            let recipient_data = if let Some(recipient) =
                                CONNECTIONS.get(&recipient_addr)
                            {
                                Some((recipient.cloned_stream().unwrap(), recipient.keys().cloned()))
                            } else
                            {
                                None
                            };

                            //SEND
                            if let Some((mut recipient_stream, recipient_keys)) = recipient_data
                            {
                                network::send(&mut recipient_stream, MessagePacket
                                {
                                    text: Some(private_message.clone()),
                                    username: Some(username.clone()),
                                    id: Some(id),
                                    code: Some(MessageCode::PrivateMessage),

                                    ..Default::default()
                                }, recipient_keys.as_ref());
                            }
                        }

                        //SEND CONFIRMATION BACK TO SENDER
                        network::send(stream, MessagePacket
                        {
                            text: Some(private_message),
                            username: CONNECTIONS.get(&recipient_addr).and_then(|e| e.username().cloned()),
                            id: Some(recipient_id),
                            code: Some(MessageCode::PrivateMessageBack),

                            ..Default::default()
                        }, Some(&keys));
                    } else
                    {
                        //INVALID PM FORMAT
                        send_code(stream, None, MessageCode::InvalidUsage, Some(&keys));
                    }
                },

                _ => {}
            }

            continue;
        }

        if read.text.is_none() { continue; } //NO MESSAGE, CONTINUE
        let message = read.text.unwrap().trim().to_string(); //TRIM MESSAGE

        //SEND MESSAGE TO ALL USERS
        send_to_all(MessagePacket
        {
            text: Some(message),
            username: Some(username.clone()),
            id: Some(id),
            colors: read.colors,
            ..Default::default()
        });
    }
}

pub fn disconnect_all() //DISCONNECT ALL CLIENTS
{
    //ITERATE OVER ALL ADDRESSES, REMOVE CONNECTIONS
    let addrs: Vec<SocketAddr> = CONNECTIONS.iter().map(|conn| *conn.peer_addr()).collect();
    for addr in &addrs
    {
        remove_connection(addr, true); //REMOVE GRACEFULLY
    }
}

pub fn disconnect_inactive() //DISCONNECT ALL INACTIVE CLIENTS
{
    let now = Instant::now();

    //COLLECT ADDRESSES OF INACTIVE CONNECTIONS
    let inactive_addrs: Vec<SocketAddr> = CONNECTIONS.iter()
        .filter(|conn| conn.is_inactive(Some(now)))
        .map(|conn| *conn.peer_addr())
        .collect();

    //DISCONNECT INACTIVE CLIENTS
    for addr in &inactive_addrs
    {
        remove_connection(addr, true);
    }
}
