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
    net::TcpStream,
    time::{ Instant, Duration },
    collections::HashSet,

    sync::
    {
        Arc,
        Mutex,
        RwLock,
    },
};

use serde_json::json;

use once_cell::sync::Lazy;

use crate::
{
    chat::
    {
        config,
        crypto,
        network::
        {
            self,
            MessageCode,
            MessagePacket,
        },
    },
};

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
        shared_key: Vec<i64>,          //SHARED KEY
        last_activity: Instant,        //TIME OF LAST MESSAGE
    },
}

#[derive(PartialEq)]
pub enum DisconnectType //TYPE OF REMOVING CLIENT FROM CONNECTIONS LIST
{
    Gracefully,   //SEND Disconnect CODE TO CLIENT
    Forcefully,   //fuck them, close connection
    Authenticate, //NOT REMOVING, ONLY TRANSFER TO AUTHENTICATED CLIENTS
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

    //GET USERNAME FROM Connection
    fn username(&self) -> Option<&String>
    {
        match self
        {
            Self::Authenticated { username, .. } => Some(username),
            Self::NonAuthenticated { .. } => None,
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
            Self::NonAuthenticated { shared_key, .. } => Some(shared_key),
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
        now.unwrap_or(Instant::now()).duration_since(*self.last_activity()) > Duration::from_secs(config::server_config("communication_time").parse().unwrap())
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
pub static CONNECTIONS: Lazy<Arc<RwLock<Vec<Connection>>>> = Lazy::new(|| //LIST FOR EACH CLIENT CONNECTION
{
    Arc::new(RwLock::new(Vec::new()))
});

//PRIVATE
fn key_exchange(stream: &mut TcpStream) -> Option<Vec<i64>> //KEY EXCHANGE FOR SERVER-SIDE
{
    //WAIT FOR ClientServerKE
    let message = loop
    {
        //READ MESSAGE
        let received = network::receive(stream, None)?;

        if received.code == Some(MessageCode::ClientServerKE) && !received.text.is_none() { break received; }
    };

    //SEND ECC PUBKEY TO CLIENT
    network::send(stream, MessagePacket
    {
        text: Some(crypto::get_public_key()),
        username: None,
        id: None,
        code: Some(MessageCode::ServerClientKE),
    }, None);

    //CALCULATE SHARED SECRET
    Some(crypto::get_shared_key(message.text.unwrap()))
}

fn send_welcome_packet(stream: &mut TcpStream, shared_key: Option<&Vec<i64>>) //send welcome packet you idiot
{
    //CREATE JSON WITH ALL THE INFO
    let welcome_json = json!(
    {
        "max_uname": config::server_config("max_username_length"),
        "min_uname": config::server_config("min_username_length"),
        "server_name": config::server_config("server_name"),
    }).to_string();

    //SEND
    send_code(stream, Some(welcome_json), MessageCode::Welcome, shared_key);
}

fn send_to_all(message: Option<&str>, username: &str, id: Option<usize>, code: Option<MessageCode>) //SEND PACKET TO ALL CLIENTS
{
    let connections = CONNECTIONS.read().unwrap(); //READ LOCK

    //SEND TO EACH CLIENT
    for connection in connections.iter()
    {
        if connection.is_authenticated()
        {
            network::send(&mut *connection.stream().lock().unwrap(), MessagePacket
            {
                text: message.map(str::to_string),
                username: Some(username.to_string()),
                id: id,
                code: code.clone(),
            }, connection.shared_key());
        }
    }
}

pub fn remove_connection(stream: &mut TcpStream, disconnect_type: DisconnectType) //REMOVE CONNECTION BY TcpStream
{
    //GET TARGET PEER ADDRESS
    let peer_addr = stream.peer_addr().unwrap();

    //USERNAME OF TARGET, FOR DISCONNECT MESSAGE
    let mut username: Option<String> = None;
    let mut removed = false;

    //REMOVE MATCHING
    {
        let mut connections = CONNECTIONS.write().unwrap(); //WRITE LOCK

        connections.retain(|conn|
        {
            let mut removed_stream = conn.stream().lock().unwrap();
            let should_remove = removed_stream.peer_addr().unwrap() == peer_addr;

            if should_remove
            {
                //SEND DISCONNECT CODE TO REMOVED CLIENT
                if disconnect_type == DisconnectType::Gracefully
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
        send_to_all(username.as_ref().map(|s| s.as_str()), &config::server_config("server_username"), None, Some(MessageCode::Leave));
    }

    println!("{} connection: {}", if disconnect_type == DisconnectType::Authenticate
    {
        "Authenticate"
    } else
    {
        "Close"
    }, &stream.peer_addr().unwrap());
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

fn get_connection_by_id(id: usize) -> Option<Connection> //RETURN CONNECTION WITH MATCHING ID
{
    CONNECTIONS.read().unwrap().iter().find(|conn|
    {
        conn.id() == Some(&id)
    }).cloned()
}

fn authenticate_client(connection: Connection) //MOVE CONNECTION FROM NonAuthenticated TO Authenticated
{
    remove_connection(&mut connection.stream().lock().unwrap(), DisconnectType::Authenticate); //REMOVE FROM NonAuthenticated
    CONNECTIONS.write().unwrap().push(connection); //ADD Authenticated
}

//PUBLIC
pub fn send_code(stream: &mut TcpStream, text: Option<String>, code: MessageCode, shared_key: Option<&Vec<i64>>) //SEND CODE TO CLIENT
{
    network::send(stream, MessagePacket
    {
        text: text,
        username: None,
        id: None,
        code: Some(code),
    }, shared_key);
}

pub fn listen_client(stream: &mut TcpStream) //CLIENT -> SERVER COMMUNICATION
{
    //GET SHARED KEY
    let shared_key = match key_exchange(stream)
    {
        Some(r) => Some(r),
        None => return
    };

    //ADD CONNECTION TO NonAuthenticated
    {
        //CREATE CONNECTION
        let connection = Connection::NonAuthenticated
        {
            stream: Arc::new(Mutex::new(stream.try_clone().expect("Failed to clone client stream"))),
            shared_key: shared_key.clone().unwrap(),
            last_activity: Instant::now(),
        };

        //PUSH
        CONNECTIONS.write().unwrap().push(connection);
    }

    //SEND PACKET WITH REQUIRED SERVER INFO
    send_welcome_packet(stream, shared_key.as_ref());

    //GET USERNAME FROM USER
    let mut username: Option<String> = None; //USER ENTERED USERNAME

    //USERNAME CONFIGS
    let max_tries: usize = config::server_config("max_username_tries").parse().unwrap(); //MAX n
    let min_len: usize = config::server_config("min_username_length").parse().unwrap();
    let max_len: usize = config::server_config("max_username_length").parse().unwrap();

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

            None => return
        }
    }

    //NO USERNAME RECEIVED, DISCONNECT CLIENT
    if username.is_none()
    {
        send_code(stream, None, MessageCode::Disconnect, shared_key.as_ref());
        return;
    }

    let username = username.unwrap();

    //ASK FOR PASSWORD
    if !config::server_users_contains(&username) //REGISTRATION
    {
        //SEND REGISTER CODE
        send_code(stream, None, MessageCode::PasswordR, shared_key.as_ref());

        //WAIT FOR ANSWER
        let response = match network::receive(stream, shared_key.as_ref())
        {
            Some(r) => r,
            None => return
        };

        //NO PASSWORD, DISCONNECT CLIENT
        if response.text.is_none()
        {
            send_code(stream, None, MessageCode::Disconnect, shared_key.as_ref());
            return;
        }

        //SAVE PASSWORD
        config::server_users_write(&username, &response.text.unwrap());
    } else //LOGIN
    {
        //SEND LOGIN CODE
        send_code(stream, None, MessageCode::PasswordL, shared_key.as_ref());

        //WAIT FOR ANSWER
        let response = match network::receive(stream, shared_key.as_ref())
        {
            Some(r) => r,
            None => return
        };

        //INVALID PASSWORD, DISCONNECT CLIENT
        if response.text.is_none() || response.text.unwrap() != config::server_users_config(&username)
        {
            send_code(stream, None, MessageCode::Disconnect, shared_key.as_ref());
            return;
        }
    }

    //GENERATE ID FOR CLIENT
    let id = get_latest_id();

    //AUTHENTICATE CLIENT
    {
        //CREATE CONNECTION
        let connection = Connection::Authenticated
        {
            stream: Arc::new(Mutex::new(stream.try_clone().expect("Failed to clone client stream"))),
            username: username.clone(),
            id: id,
            shared_key: shared_key.clone().unwrap(),
            last_activity: Instant::now(),
            spam_violations: 0,
        };

        //PUSH
        authenticate_client(connection);
    }

    //TELL CLIENT TO START CHATTING
    send_code(stream, None, MessageCode::Accept, shared_key.as_ref());

    //SEND JOIN MESSAGE
    send_to_all(Some(&username), &config::server_config("server_username"), None, Some(MessageCode::Join));

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
        if read.code.is_some()
        {
            match read.code.unwrap()
            {
                //CLIENT QUITS
                MessageCode::Disconnect =>
                {
                    //DISCONNECT CLIENT
                    remove_connection(stream, DisconnectType::Gracefully);

                    return;
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
                        username: None,
                        id: None,
                        code: Some(MessageCode::List),
                    }, shared_key.as_ref());
                },

                //PRIVATE MESSAGE
                MessageCode::PrivateMessage =>
                {
                    //CHECK PARAMETER VALIDITY
                    if let Some(message) = read.text //CLIENT ACTUALLY SENT SOMETHING
                    {
                        if let Some((sender_id, private_message)) = message.split_once(' ') //CLIENT ACTUALLY PASSED AT LEAST TWO PARAMETERS
                        {
                            if let Ok(num) = sender_id.parse::<usize>() //CLIENT ACTUALLY SENT NUMERIC ID
                            {
                                if let Some(recipient) = get_connection_by_id(num) //yippee!! client sent valid id
                                {
                                    if recipient.is_authenticated()
                                    {
                                        //SEND MESSAGE TO RECEIVER
                                        if num != id //DO NOT SEND ON SELF MESSAGE
                                        {
                                            network::send(&mut recipient.stream().lock().unwrap(), MessagePacket
                                            {
                                                text: Some(private_message.to_string()),
                                                username: Some(username.clone()),
                                                id: Some(id),
                                                code: Some(MessageCode::PrivateMessage),
                                            }, recipient.shared_key());
                                        }

                                        //SEND MESSAGE BACK TO SENDER
                                        network::send(stream, MessagePacket
                                        {
                                            text: Some(private_message.to_string()),
                                            username: recipient.username().cloned(),
                                            id: Some(num),
                                            code: Some(MessageCode::PrivateMessageBack),
                                        }, shared_key.as_ref());

                                        continue; //VALID, DO NOT SEND InvalidUsage CODE
                                    }
                                }
                            }
                        }
                    }

                    //SEND InvalidUsage CODE IF INVALID
                    network::send(stream, MessagePacket
                    {
                        text: None,
                        username: None,
                        id: None,
                        code: Some(MessageCode::InvalidUsage),
                    }, shared_key.as_ref());
                    continue;
                },

                _ => continue
            }

            continue; //DO NOT FORWARD CODES
        }

        if read.text.is_none() { continue; } //NO MESSAGE, CONTINUE
        let message = read.text.unwrap();

        send_to_all(Some(&message), &username, Some(id), None);
    }
}

pub fn disconnect_all() //DISCONNECT ALL CLIENTS
{
    //ITERATE OVER ALL STREAMS, REMOVE CONNECTIONS
    let mut streams: Vec<TcpStream> = CONNECTIONS.read().unwrap().iter().map(|conn| conn.cloned_stream().unwrap()).collect();
    for stream in &mut streams
    {
        remove_connection(stream, DisconnectType::Gracefully); //REMOVE GRACEFULLY
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
        remove_connection(&mut stream, DisconnectType::Gracefully);
    }
}
