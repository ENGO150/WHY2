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

use std::
{
    collections::HashSet,
    net::{ TcpStream, SocketAddr },
    time::{ Instant, Duration },
    sync::
    {
        LazyLock,
        Arc,
        Mutex,
        RwLock,
    },
};

use serde_json::json;

use crate::chat::
{
    config,
    crypto,
    options,
    misc,
    network::
    {
        self,
        MessageCode,
        MessagePacket,
    },
};

//CONSTS
const GRID_W: usize = options::GRID_DIMENSIONS.0;
const GRID_H: usize = options::GRID_DIMENSIONS.1;

//ENUMS
#[derive(Clone)]
pub enum Connection //CLIENT CONNECTION (WHAT IS PUSHED TO connections LIST)
{
    Authenticated
    {
        stream: Arc<Mutex<TcpStream>>, //STREAM
        username: String,              //USERNAME
        id: usize,                     //ID OF USER
        shared_key: Vec<i64>,          //SHARED KEY BETWEEN SERVER AND CLIENT (one to one)
        last_activity: Instant,        //TIME OF LAST MESSAGE (USED FOR TIMEOUT)
        spam_violations: usize,        //SPAM VIOLATIONS (unexpexted, huh?)
    },

    NonAuthenticated
    {
        stream: Arc<Mutex<TcpStream>>, //STREAM
        username: Option<String>,      //CHOSEN USERNAME
        shared_key: Option<Vec<i64>>,  //SHARED KEY
        last_activity: Instant,        //TIME OF LAST MESSAGE
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
    pub fn peer_addr(&self) -> Option<SocketAddr>
    {
        self.stream().lock().ok()?.peer_addr().ok()
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
    fn id(&self) -> Option<&usize>
    {
        match self
        {
            Self::Authenticated { id, .. } => Some(id),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET SHARED KEY FROM Connection
    pub fn shared_key(&self) -> Option<&Vec<i64>>
    {
        match self
        {
            Self::Authenticated { shared_key, .. } => Some(shared_key),
            Self::NonAuthenticated { shared_key, .. } => shared_key.as_ref(),
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

    //CHECK IF CONNECTION IS INACTIVE
    fn is_inactive(&self, now: Option<Instant>) -> bool
    {
        now.unwrap_or(Instant::now()).duration_since(*self.last_activity()) > Duration::from_secs(config::server_config::<u64>("communication_time"))
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
pub static CONNECTIONS: LazyLock<Arc<RwLock<Vec<Connection>>>> = LazyLock::new(|| //LIST FOR EACH CLIENT CONNECTION
{
    Arc::new(RwLock::new(Vec::new()))
});

//PRIVATE
fn key_exchange(stream: &mut TcpStream) -> Option<Vec<i64>> //KEY EXCHANGE FOR SERVER-SIDE
{
    //GENERATE EPHEMERAL KEYS
    let (sk, pk) = crypto::get_server_keys();

    //SEND ECC PUBKEY TO CLIENT
    network::send(stream, MessagePacket
    {
        text: Some(pk),
        code: Some(MessageCode::KeyExchange),
        ..Default::default()
    }, None);

    //SET READ TIMEOUT FOR ZOMBIE CONNECTIONS
    stream.set_read_timeout(Some(Duration::from_millis(2000))).expect("Failed to set read timeout");

    let mut invalid_packets = 0; //INVALID KEY EXCHANGE PACKETS COUNTER

    //WAIT FOR KeyExchange
    let message = loop
    {
        //READ MESSAGE
        let received = network::receive(stream, None)?;

        if received.code == Some(MessageCode::KeyExchange) && !received.text.is_none() { break received; }

        //CHECK INVALID PACKETS COUNTER
        if invalid_packets == 3 { return None; }
        invalid_packets += 1; //INCREMENT
    };

    //REMOVE READ TIMEOUT
    stream.set_read_timeout(None).expect("Failed to unset read timeout");

    //CALCULATE SHARED SECRET
    crypto::derive_shared_secret::<GRID_W, GRID_H>(sk, message.text.unwrap())
}

fn send_welcome_packet(stream: &mut TcpStream, shared_key: Option<&Vec<i64>>) //send welcome packet you idiot
{
    //CREATE JSON WITH ALL THE INFO
    let welcome_json = json!(
    {
        "min_pass": config::server_config::<usize>("min_password_length"),
        "max_uname": config::server_config::<usize>("max_username_length"),
        "min_uname": config::server_config::<usize>("min_username_length"),
        "server_name": config::server_config::<String>("server_name"),
    }).to_string();

    //SEND
    send_code(stream, Some(welcome_json), MessageCode::Welcome, shared_key);
}

fn send_to_all(packet: MessagePacket) //SEND PACKET TO ALL CLIENTS
{
    let connections = CONNECTIONS.read().unwrap(); //READ LOCK

    //SEND TO EACH CLIENT
    for connection in connections.iter()
    {
        if connection.is_authenticated()
        {
            network::send(&mut *connection.stream().lock().unwrap(), packet.clone(), connection.shared_key());
        }
    }
}

pub fn remove_connection(stream: &mut TcpStream, grace: bool) //REMOVE CONNECTION BY TcpStream
{
    //GET TARGET PEER ADDRESS
    let peer_addr = stream.peer_addr().ok();

    //USERNAME OF TARGET, FOR DISCONNECT MESSAGE
    let mut username: Option<String> = None;
    let mut removed = false;

    //REMOVE MATCHING
    {
        let mut connections = CONNECTIONS.write().unwrap(); //WRITE LOCK

        connections.retain(|conn|
        {
            let should_remove = conn.peer_addr() == peer_addr;
            let mut removed_stream = conn.stream().lock().unwrap();

            if should_remove
            {
                //SEND DISCONNECT CODE TO REMOVED CLIENT
                if grace
                {
                    send_code(&mut removed_stream, None, MessageCode::Disconnect, conn.shared_key());
                }

                if conn.is_authenticated()
                {
                    username = conn.username().cloned();
                }

                removed = true;
            }

            !should_remove //KEEP NON-MATCHING
        });
    }

    //RETURN IF NO CLIENT WAS REMOVED
    if !removed { return; }

    if username.is_some()
    {
        send_to_all(MessagePacket
        {
            text: username.as_ref().map(|s| s.to_string()),
            username: Some(config::server_config::<String>("server_username")),
            code: Some(MessageCode::Leave),
            ..Default::default()
        });
    }

    println!("Close connection: {}", peer_addr.unwrap());
}

fn user_connected(username: &str) -> bool //CHECK IF CLIENT WITH username IS CONNECTED
{
    CONNECTIONS.read().unwrap().iter().any(|conn|
    {
        conn.username() == Some(&username.to_string())
    })
}

fn get_latest_id() -> usize
{
    //GET HashSet OF IDS
    let ids: HashSet<usize> = CONNECTIONS.read().unwrap().iter().filter_map(|conn|
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

fn find_connection_index<F>(connections: &Vec<Connection>, condition: F) -> Option<usize> //RETURN INDEX OF CONNECTION WITH MATCHING PREDICATE
where
    F: Fn(&Connection) -> bool
{
    connections.iter().position(condition)
}

fn update_client_shared_key(stream: &mut TcpStream, shared_key: &Vec<i64>) //ADD KEY TO NonAuthenticated CLIENT AFTER KEY EXCHANGE
{
    let peer_addr = stream.peer_addr().ok();
    let mut connections = CONNECTIONS.write().unwrap();

    //FIND CONNECTION
    if let Some(index) = find_connection_index(&connections, |c| c.peer_addr() == peer_addr)
    {
        //CLONE STREAM
        let stream = connections[index].stream().clone();

        //OVERWRITE OLD CONNECTION
        connections[index] = Connection::NonAuthenticated
        {
            stream: stream,
            username: None,
            shared_key: Some(shared_key.to_owned()),
            last_activity: Instant::now(),
        };
    }
}

fn authenticate_client(stream: &mut TcpStream, username: &str, id: usize) //MOVE CONNECTION FROM NonAuthenticated TO Authenticated
{
    let peer_addr = stream.peer_addr().ok();
    let mut connections = CONNECTIONS.write().unwrap();

    //FIND CONNECTION
    if let Some(index) = find_connection_index(&connections, |c| c.peer_addr() == peer_addr)
    {
        //CLONE OLD PROPERTIES
        let stream = connections[index].stream().clone();
        let shared_key = connections[index].shared_key().unwrap().to_owned();

        //OVERWRITE OLD CONNECTION
        connections[index] = Connection::Authenticated
        {
            stream: stream,
            username: username.to_string(),
            id: id,
            shared_key: shared_key,
            last_activity: Instant::now() - Duration::from_millis(config::server_config("min_message_delay")),
            spam_violations: 0,
        };

        println!("Authenticate connection: {}", peer_addr.unwrap());
    }
}

fn ask_version(stream: &mut TcpStream, shared_key: Option<&Vec<i64>>) -> Option<String> //ASK CLIENT FOR VERSION
{
    //ASK FOR VERSION
    send_code(stream, Some(misc::get_version().to_string()), MessageCode::Version, shared_key);

    //WAIT FOR RESPONSE FROM CLIENT
    let version = loop
    {
        //READ MESSAGE
        let received = network::receive(stream, shared_key)?;

        if received.code == Some(MessageCode::Version) && !received.text.is_none() { break received; }
    };

    version.text
}

//PUBLIC
pub fn send_code(stream: &mut TcpStream, text: Option<String>, code: MessageCode, shared_key: Option<&Vec<i64>>) //SEND CODE TO CLIENT
{
    network::send(stream, MessagePacket
    {
        text: text,
        code: Some(code),
        ..Default::default()
    }, shared_key);
}

pub fn listen_client(stream: &mut TcpStream) //CLIENT -> SERVER COMMUNICATION
{
    println!("New connection: {}", match stream.peer_addr()
    {
        Ok(addr) => addr,
        Err(_) => return //idk
    });

    //ADD CONNECTION TO NonAuthenticated
    {
        //CREATE CONNECTION
        let connection = Connection::NonAuthenticated
        {
            stream: Arc::new(Mutex::new(stream.try_clone().expect("Failed to clone client stream"))),
            username: None,
            shared_key: None,
            last_activity: Instant::now(),
        };

        //PUSH
        CONNECTIONS.write().unwrap().push(connection);
    }

    //GET SHARED KEY
    let shared_key = match key_exchange(stream)
    {
        Some(r) => Some(r),
        None => return remove_connection(stream, false)
    };

    //UPDATE CONNECTION
    update_client_shared_key(stream, shared_key.as_ref().unwrap());

    //ASK CLIENT FOR THEIR PACKAGE VERSION
    if config::server_config("check_client_version")
    {
        let version = ask_version(stream, shared_key.as_ref());
        if version.is_none() || version != Some(misc::get_version().to_string())
        {
            return remove_connection(stream, true);
        }
    }

    //SEND PACKET WITH REQUIRED SERVER INFO
    send_welcome_packet(stream, shared_key.as_ref());

    //GET USERNAME FROM USER
    let mut username: Option<String> = None; //USER ENTERED USERNAME

    //USERNAME CONFIGS
    let max_tries = config::server_config::<usize>("max_auth_tries"); //MAX n
    let min_len = config::server_config::<usize>("min_username_length");
    let max_len = config::server_config::<usize>("max_username_length");

    //TELL USER IF REGISTRATIONS ARE DISABLED
    let disabled_registration = !config::server_config::<bool>("allow_register");
    if disabled_registration
    {
        send_code(stream, None, MessageCode::RegisterDisabled, shared_key.as_ref());
    }

    //ASK n TIMES
    for _ in 0..max_tries
    {
        //SEND PICK_USERNAME CODE
        send_code(stream, None, MessageCode::Username, shared_key.as_ref());

        match network::receive(stream, shared_key.as_ref())
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

            None => return remove_connection(stream, false),
        }
    }

    //NO USERNAME RECEIVED, DISCONNECT CLIENT
    if username.is_none()
    {
        return remove_connection(stream, true);
    }

    let username = username.unwrap();

    //UPDATE USERNAME IN NonAuthenticated
    {
        let mut connections = CONNECTIONS.write().unwrap(); //WRITE LOCK
        let peer_addr = stream.peer_addr().ok(); //GET PEER ADDRESS

        for conn in connections.iter_mut()
        {
            if conn.peer_addr() == peer_addr && !conn.is_authenticated() //CONNECTION FOUND
            {
                //UPDATE
                *conn.username_mut() = Some(username.clone());

                break;
            }
        }
    }

    let user_exists = config::server_users_contains(&username);

    //ASK FOR PASSWORD
    if !user_exists && !disabled_registration //REGISTRATION (ENTER "FAKE" LOGIN ON DISABLED REGISTER)
    {
        let max_tries = config::server_config::<usize>("max_auth_tries"); //MAX n
        let mut password: Option<String> = None;

        //KEEP ASKING FOR PASSWORD n TIMES
        for _ in 0..max_tries
        {
            //SEND REGISTER CODE
            send_code(stream, None, MessageCode::PasswordR, shared_key.as_ref());

            //WAIT FOR ANSWER
            match network::receive(stream, shared_key.as_ref())
            {
                Some(r) =>
                {
                    if let Some(pass) = r.text
                    {
                        //CHECK LENGTH
                        if pass.len() > config::server_config("min_password_length")
                        {
                            password = Some(pass);
                            break;
                        }
                    }
                },

                None => return remove_connection(stream, false)
            };
        }

        if password.is_none()
        {
            return remove_connection(stream, true);
        }

        //SAVE PASSWORD
        config::server_users_write(&username, &crypto::hash_password(&password.unwrap()));
    } else //LOGIN
    {
        //SEND LOGIN CODE
        send_code(stream, None, MessageCode::PasswordL, shared_key.as_ref());

        //WAIT FOR ANSWER
        let response = match network::receive(stream, shared_key.as_ref())
        {
            Some(r) => r,
            None => return remove_connection(stream, false),
        };

        //INVALID PASSWORD (OR FAKE LOGIN), DISCONNECT CLIENT
        if !user_exists || response.text.is_none() || !crypto::compare_password_hash(&config::server_users_config(&username), &response.text.unwrap())
        {
            return remove_connection(stream, true);
        }
    }

    //GENERATE ID FOR CLIENT
    let id = get_latest_id();

    //AUTHENTICATE CLIENT
    authenticate_client(stream, &username, id);

    //TELL CLIENT TO START CHATTING
    send_code(stream, None, MessageCode::Accept, shared_key.as_ref());

    //SEND JOIN MESSAGE
    send_to_all(MessagePacket
    {
        text: Some(username.clone()),
        username: Some(config::server_config::<String>("server_username")),
        code: Some(MessageCode::Join),
        ..Default::default()
    });

    //LOOP READING
    loop
    {
        //READ
        let read = match network::receive(stream, shared_key.as_ref())
        {
            Some(r) => r,
            None => return
        };

        //CLIENT CODES
        if let Some(code) = read.code
        {
            match code
            {
                //CLIENT QUITS
                MessageCode::Disconnect =>
                {
                    //DISCONNECT CLIENT
                    return remove_connection(stream, true);
                },

                //CLIENT REQUESTED LIST OF ONLINE USERS
                MessageCode::List =>
                {
                    let connections = CONNECTIONS.read().unwrap(); //READ LOCK
                    let mut user_list = Vec::new();

                    //ITERATE OVER CONNECTIONS, CREATE JSON OF USERS
                    for connection_enum in connections.iter()
                    {
                        if let Connection::Authenticated { username: uname, id: user_id, .. } = connection_enum
                        {
                            user_list.push(json!({ "username": uname, "id": user_id }));
                        }
                    }

                    //SEND LIST BACK TO CLIENT
                    network::send(stream, MessagePacket
                    {
                        text: Some(json!(user_list).to_string()), //BUILD JSON FROM user_list
                        code: Some(MessageCode::List),
                        ..Default::default()
                    }, shared_key.as_ref());
                },

                //PRIVATE MESSAGE
                MessageCode::PrivateMessage =>
                {
                    //CHECK CLOSURE
                    let parse_pm_data = |connections: &Vec<Connection>, message: Option<&String>| -> Option<(usize, usize, String)>
                    {
                        let text = message?; //EXISTING PARAMETERS
                        let (sender_id, private_message) = text.split_once(' ')?; //VALID PARAMETERS
                        let recipient_id = sender_id.parse::<usize>().ok()?; //VALID ID
                        let recipient_index = find_connection_index(&connections,
                        |c| c.id() == Some(&recipient_id))?; //EXISTING ID

                        Some((recipient_index, recipient_id, private_message.to_string()))
                    };

                    let connections = CONNECTIONS.read().unwrap();
                    if let Some((recipient_index, recipient_id, private_message)) = parse_pm_data(&connections, read.text.as_ref())
                    {
                        //SEND MESSAGE TO RECEIVER
                        if recipient_id != id //DO NOT SEND ON SELF MESSAGE
                        {
                            network::send(&mut connections[recipient_index].stream().lock().unwrap(), MessagePacket
                            {
                                text: Some(private_message.to_string()),
                                username: Some(username.clone()),
                                id: Some(id),
                                code: Some(MessageCode::PrivateMessage),
                                ..Default::default()
                            }, connections[recipient_index].shared_key());
                        }

                        //SEND MESSAGE BACK TO SENDER
                        network::send(stream, MessagePacket
                        {
                            text: Some(private_message.to_string()),
                            username: connections[recipient_index].username().cloned(),
                            id: Some(recipient_id),
                            code: Some(MessageCode::PrivateMessageBack),
                            ..Default::default()
                        }, shared_key.as_ref());
                    } else
                    {
                        //SEND InvalidUsage CODE IF INVALID
                        send_code(stream, None, MessageCode::InvalidUsage, shared_key.as_ref());
                    }

                    continue;
                },

                _ => continue
            }
        }

        if read.text.is_none() { continue; } //NO MESSAGE, CONTINUE
        let message = read.text.unwrap();

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
    //ITERATE OVER ALL STREAMS, REMOVE CONNECTIONS
    let mut streams: Vec<TcpStream> = CONNECTIONS.read().unwrap().iter().map(|conn| conn.cloned_stream().unwrap()).collect();
    for stream in &mut streams
    {
        remove_connection(stream, true); //REMOVE GRACEFULLY
    }
}

pub fn disconnect_inactive() //DISCONNECT ALL INACTIVE CLIENTS
{
    let now = Instant::now();

    let connections = CONNECTIONS.read().unwrap(); //READ LOCK

    //COLLECT STREAMS OF INACTIVE CONNECTIONS
    let inactive_streams: Vec<TcpStream> = connections.iter()
        .filter(|conn| conn.is_inactive(Some(now)))
        .filter_map(|conn| conn.cloned_stream())
        .collect();

    drop(connections); //RELEASE READ LOCK

    //DISCONNECT INACTIVE STREAMS
    for mut stream in inactive_streams
    {
        remove_connection(&mut stream, true);
    }
}
