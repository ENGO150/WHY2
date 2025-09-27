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
    process,
    net::TcpStream,

    io::
    {
        self,
        Read,
        Write,
        BufReader,
        BufRead,
    },
};

use serde::{ Serialize, Deserialize };
use serde_json::Value;

use crossterm::terminal;

use crate::
{
    core::rex::
    {
        encrypter,
        decrypter,
        misc,
        options::{ self, Grid },
    },

    chat::
    {
        config,
        crypto,
        options as chat_options,
    },
};

#[cfg(feature = "server")]
use serde_json::json;

#[cfg(feature = "server")]
use once_cell::sync::Lazy;

#[cfg(feature = "server")]
use std::
{
    time::{ Instant, Duration },
    collections::HashSet,

    sync::
    {
        Arc,
        Mutex,
        RwLock,
    },
};

//STRUCTS
#[derive(Serialize, Deserialize, PartialEq, Clone)]
pub enum MessageCode //CONTROL CODES
{
    ClientServerKE,     //CLIENT -> SERVER | KEY EXCHANGE
    ServerClientKE,     //SERVER -> CLIENT | KEY EXCHANGE
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
    InvalidUsage,       //SERVER -> CLIENT | INVALID PARAMETERS TO A COMMAND
}

#[derive(Serialize, Deserialize)]
pub struct MessagePacket //MESSAGE PACKET (WHAT IS BEING SENT)
{
    pub text: Option<String>,      //MESSAGE
    pub username: Option<String>,  //USERNAME (SENT ONLY BY SERVER, AS SERVER DOESN'T ACCEPT USERNAMES FROM CLIENT)
    pub id: Option<usize>,         //ID OF USER
    pub code: Option<MessageCode>, //CONTROL CODE
}

//ENUMS
#[cfg(feature = "server")]
#[derive(Clone)]
enum Connection //CLIENT CONNECTION (WHAT IS PUSHED TO connections LIST)
{
    Authenticated
    {
        stream: Arc<Mutex<TcpStream>>, //STREAM
        username: String,              //USERNAME
        id: usize,                     //ID OF USER
        shared_key: Vec<i64>,          //SHARED KEY BETWEEN SERVER AND CLIENT (one to one)
        last_activity: Instant,        //TIME OF LAST MESSAGE (USED FOR TIMEOUT)
    },

    NonAuthenticated
    {
        stream: Arc<Mutex<TcpStream>>, //STREAM
        shared_key: Vec<i64>,          //SHARED KEY
        last_activity: Instant,        //TIME OF LAST MESSAGE
    },
}

#[cfg(feature = "server")]
#[derive(PartialEq)]
enum DisconnectType //TYPE OF REMOVING CLIENT FROM CONNECTIONS LIST
{
    Gracefully,   //SEND Disconnect CODE TO CLIENT
    Forcefully,   //fuck them, close connection
    Authenticate, //NOT REMOVING, ONLY TRANSFER TO AUTHENTICATED CLIENTS
}

//IMPLEMENTATIONS
#[cfg(feature = "server")]
impl Connection
{
    //GET STREAM FROM Connection
    fn stream(&self) -> &Arc<Mutex<TcpStream>>
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
    fn shared_key(&self) -> Option<&Vec<i64>>
    {
        match self
        {
            Self::Authenticated { shared_key, .. } => Some(shared_key),
            Self::NonAuthenticated { shared_key, .. } => Some(shared_key),
        }
    }

    //GET LAST ACTIVITY FROM Connection
    fn last_activity(&self) -> &Instant
    {
        match self
        {
            Self::Authenticated { last_activity, .. } => last_activity,
            Self::NonAuthenticated { last_activity, .. } => last_activity,
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
    fn is_authenticated(&self) -> bool
    {
        match self
        {
            Self::Authenticated { .. } => true,
            Self::NonAuthenticated { .. } => false,
        }
    }
}

//LISTS
#[cfg(feature = "server")]
static CONNECTIONS: Lazy<Arc<RwLock<Vec<Connection>>>> = Lazy::new(|| //LIST FOR EACH CLIENT CONNECTION
{
    Arc::new(RwLock::new(Vec::new()))
});

//PRIVATE
#[cfg(feature = "server")]
fn send_code(stream: &mut TcpStream, text: Option<String>, code: MessageCode, shared_key: Option<&Vec<i64>>) //SEND CODE TO CLIENT
{
    send(stream, MessagePacket
    {
        text: text,
        username: None,
        id: None,
        code: Some(code),
    }, shared_key);
}

fn key_exchange_client(stream: &mut TcpStream) -> Vec<i64> //KEY EXCHANGE FOR CLIENT-SIDE
{
    //SEND ECC PUBKEY TO SERVER
    send(stream, MessagePacket
    {
        text: Some(crypto::get_public_key()),
        username: None,
        id: None,
        code: Some(MessageCode::ClientServerKE),
    }, None);

    //WAIT FOR ServerClientKE
    let message = loop
    {
        //READ MESSAGE
        let received = receive(stream, None).unwrap();

        if received.code == Some(MessageCode::ServerClientKE) { break received; }
    };

    //CALCULATE SHARED SECRET
    crypto::get_shared_key(message.text.unwrap())
}

#[cfg(feature = "server")]
fn key_exchange_server(stream: &mut TcpStream) -> Option<Vec<i64>> //KEY EXCHANGE FOR SERVER-SIDE
{
    //WAIT FOR ClientServerKE
    let message = loop
    {
        //READ MESSAGE
        let received = receive(stream, None)?;

        if received.code == Some(MessageCode::ClientServerKE) && !received.text.is_none() { break received; }
    };

    //SEND ECC PUBKEY TO CLIENT
    send(stream, MessagePacket
    {
        text: Some(crypto::get_public_key()),
        username: None,
        id: None,
        code: Some(MessageCode::ServerClientKE),
    }, None);

    //CALCULATE SHARED SECRET
    Some(crypto::get_shared_key(message.text.unwrap()))
}

#[cfg(feature = "server")]
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

#[cfg(feature = "server")]
fn send_to_all(message: Option<&str>, username: &str, id: Option<usize>, code: Option<MessageCode>) //SEND PACKET TO ALL CLIENTS
{
    let connections = CONNECTIONS.read().unwrap(); //READ LOCK

    //SEND TO EACH CLIENT
    for connection in connections.iter()
    {
        if connection.is_authenticated()
        {
            send(&mut *connection.stream().lock().unwrap(), MessagePacket
            {
                text: message.map(str::to_string),
                username: Some(username.to_string()),
                id: id,
                code: code.clone(),
            }, connection.shared_key());
        }
    }
}

#[cfg(feature = "server")]
fn remove_connection(stream: &mut TcpStream, disconnect_type: DisconnectType) //REMOVE CONNECTION BY TcpStream
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

#[cfg(feature = "server")]
fn user_connected(username: &str) -> bool //CHECK IF CLIENT WITH username IS CONNECTED
{
    CONNECTIONS.read().unwrap().iter().any(|conn|
    {
        conn.username() == Some(&username.to_string())
    })
}

#[cfg(feature = "server")]
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

#[cfg(feature = "server")]
fn get_connection_by_id(id: usize) -> Option<Connection> //RETURN CONNECTION WITH MATCHING ID
{
    CONNECTIONS.read().unwrap().iter().find(|conn|
    {
        conn.id() == Some(&id)
    }).cloned()
}

fn str_to_grids(bytes: Vec<u8>) -> Option<Vec<Grid>> //CONVERT STRING SLICE TO VECTOR OF GRIDS
{
    let matrix_size = options::GRID_DIMENSIONS.0 * options::GRID_DIMENSIONS.1 * 8; //EACH i64 IS 8 BYTES

    //CHECK FOR VALID GRID
    if bytes.len() % matrix_size != 0 { return None; }

    Some(bytes.chunks(matrix_size).map(|chunk|
    {
        let mut grid = misc::empty_grid();
        for i in 0..(options::GRID_DIMENSIONS.0)
        {
            for j in 0..(options::GRID_DIMENSIONS.1)
            {
                let start = (i * options::GRID_DIMENSIONS.1 + j) * 8;
                let slice = &chunk[start..start + 8];
                grid[i][j] = i64::from_be_bytes(slice.try_into().unwrap());
            }
        }

        grid
    }).collect())
}

#[cfg(feature = "server")]
fn authenticate_client(connection: Connection) //MOVE CONNECTION FROM NonAuthenticated TO Authenticated
{
    remove_connection(&mut connection.stream().lock().unwrap(), DisconnectType::Authenticate); //REMOVE FROM NonAuthenticated
    CONNECTIONS.write().unwrap().push(connection); //ADD Authenticated
}

//PUBLIC
#[cfg(feature = "server")]
pub fn listen_client(stream: &mut TcpStream) //CLIENT -> SERVER COMMUNICATION
{
    //GET SHARED KEY
    let shared_key = match key_exchange_server(stream)
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

        match receive(stream, shared_key.as_ref())
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
        let response = match receive(stream, shared_key.as_ref())
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
        let response = match receive(stream, shared_key.as_ref())
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
        let read = match receive(stream, shared_key.as_ref())
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
                    send(stream, MessagePacket
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
                                            send(&mut recipient.stream().lock().unwrap(), MessagePacket
                                            {
                                                text: Some(private_message.to_string()),
                                                username: Some(username.clone()),
                                                id: Some(id),
                                                code: Some(MessageCode::PrivateMessage),
                                            }, recipient.shared_key());
                                        }

                                        //SEND MESSAGE BACK TO SENDER
                                        send(stream, MessagePacket
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
                    send(stream, MessagePacket
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

pub fn listen_server(stream: &mut TcpStream) //SERVER -> CLIENT COMMUNICATION
{
    //SET GLOBAL CLIENT SHARED KEY
    chat_options::set_shared_key(key_exchange_client(stream));

    //SERVER INFO VARIABLES
    let mut max_uname: Option<u8> = None;
    let mut min_uname: Option<u8> = None;
    let mut server_name: &str;

    let mut invalid_username = false; //PRINT "Invalid Username!"

    //FORMATTING SHIT
    let mut first_message = true;
    let mut extra_space: bool;

    //LOOP READING
    loop
    {
        let read = receive(stream, chat_options::get_shared_key().as_ref()).unwrap();
        extra_space = false; //RESET EXTRA SPACE

        //EXTRA SPACE
        if chat_options::get_extra_space() { println!(); }

        //CODES
        if let Some(code) = read.code
        {
            match code
            {
                //WELCOME CODE - SERVER INFORMATIONS
                MessageCode::Welcome =>
                {
                    //PARSE JSON
                    let welcome_json: Value = serde_json::from_str(&read.text.unwrap()).expect("Parsing welcome json failed"); //PARSE WELCOME JSON

                    //GET INFO FROM JSON
                    max_uname = Some(welcome_json["max_uname"].as_str().expect("Invalid welcome json").parse().expect("Parsing info to int failed"));
                    min_uname = Some(welcome_json["min_uname"].as_str().expect("Invalid welcome json").parse().expect("Parsing info to int failed"));
                    server_name = welcome_json["server_name"].as_str().expect("Invalid welcome json");

                    println!("Successfully connected to {server_name}.\n");
                },

                //PICK_USERNAME CODE - guess what
                MessageCode::Username =>
                {
                    clear_lines(2);

                    //INVALID UNAME
                    if invalid_username
                    {
                        clear_lines(2);
                        print!("Username rejected!");
                    } else //VALID
                    {
                        //SET INVALID USERNAME FOR POSSIBLE NEXT CODE
                        invalid_username = true;
                    }

                    println!("\n\rEnter username (a-Z, 0-9; {}-{} characters):", min_uname.unwrap(), max_uname.unwrap());
                },

                //REGISTER
                MessageCode::PasswordR =>
                {
                    clear_lines(3);
                    chat_options::set_asking_password(true);
                    println!("\nEnter password: (REGISTER)");
                },

                //LOGIN
                MessageCode::PasswordL =>
                {
                    clear_lines(3);
                    chat_options::set_asking_password(true);
                    println!("\nEnter password: (LOGIN)");
                },

                //START CHATTING
                MessageCode::Accept =>
                {
                    clear_lines(3);
                    println!("Login successful. Press Ctrl+H for help.\n");
                },

                //JOIN MESSAGE (CLIENT CONNECTED)
                MessageCode::Join =>
                {
                    clear_lines(2);

                    if first_message
                    {
                        println!();
                        first_message = false;
                    }

                    println!("[{}]: {} connected.\n", read.username.unwrap(), read.text.unwrap());
                }

                //LEAVE MESSAGE (CLIENT DISCONNECTED)
                MessageCode::Leave =>
                {
                    clear_lines(2);

                    println!("[{}]: {} disconnected.\n", read.username.unwrap(), read.text.unwrap());
                },

                //LIST OF ONLINE USERS
                MessageCode::List =>
                {
                    clear_lines(2);

                    if !chat_options::get_extra_space() { println!(); }
                    println!("Online users:");

                    //PARSE JSON
                    let users_json: Value = serde_json::from_str(&read.text.unwrap()).unwrap();

                    //PRINT USERS
                    for user in users_json.as_array().unwrap()
                    {
                        println!("\r{} ({})", user["username"].as_str().unwrap(), user["id"]);
                    }

                    println!();

                    extra_space = true;
                    chat_options::set_extra_space(true);
                },

                //PRIVATE MESSAGE INCOMING
                MessageCode::PrivateMessage =>
                {
                    clear_lines(2);
                    println!("[PM FROM] {} ({}): {}\n", read.username.unwrap(), read.id.unwrap(), read.text.unwrap());
                },

                //PRIVATE MESSAGE INCOMING
                MessageCode::PrivateMessageBack =>
                {
                    clear_lines(2);
                    println!("[PM TO] {} ({}): {}\n", read.username.unwrap(), read.id.unwrap(), read.text.unwrap());
                },

                //CLIENT MESSED SOME COMMAND UP
                MessageCode::InvalidUsage =>
                {
                    clear_lines(2);
                    println!("Invalid usage! Press Ctrl+H for help.\n");
                },

                //SERVER DOESN'T LIKE YA ANYMORE - EXIT
                MessageCode::Disconnect =>
                {
                    terminal::disable_raw_mode().unwrap();
                    println!("\nServer quit communication.");
                    process::exit(0);
                }

                _ => continue //EITHER INVALID CODE OR A KEY EXCHANGE CODE
            }
        } else //NO CODE, PRINT MESSAGE
        {
            clear_lines(2);

            println!("{} ({}): {}\n", read.username.unwrap(), read.id.unwrap(), read.text.unwrap());
        }

        //PRINT INPUT PROMPT
        print!("\r>>> {}", chat_options::INPUT_READ.lock().unwrap().iter().collect::<String>());
        io::stdout().flush().unwrap();
        if !extra_space { chat_options::set_extra_space(false); } //DISABLE EXTRA SPACE
    }
}

pub fn send(stream: &mut TcpStream, packet: MessagePacket, key: Option<&Vec<i64>>) //SEND packet TO stream
{
    //ENCODE THE PACKET STRUCT TO Vec<u8>
    let encoded_packet = bincode::serde::encode_to_vec(packet, bincode::config::standard()).expect("Encoding packet failed");
    let mut encoded_packet_string = String::from_utf8(base91::slice_encode(&encoded_packet)).expect("Encoding packet failed"); //ENCODE TO BASE91 STRING

    //ENCRYPT
    if let Some(key) = key
    {
        //ENCRYPT
        let encrypted_packet = encrypter::encrypt_string(&encoded_packet_string, Some(key.to_vec())).expect("Encrypting packet failed").output;

        //CONVERT ENCRYPTED PACKET (FROM Vec<Grid>) TO Vec<u8>
        let encrypted_packet_flattened: Vec<u8> = encrypted_packet.iter()
            .flat_map(|grid| grid.iter()
                .flat_map(|row| row.iter()
                    .flat_map(|&val| val.to_be_bytes()))).collect();

        //OVERWRITE encoded_packet_string
        encoded_packet_string = String::from_utf8(base91::slice_encode(&encrypted_packet_flattened)).expect("Encoding encrypted packet failed");
    }

    //SEND
    stream.write_all((encoded_packet_string + "\n").as_bytes()).expect("Sending packet failed");
    stream.flush().expect("Flushing stream failed");
}

pub fn receive(stream: &mut TcpStream, key: Option<&Vec<i64>>) -> Option<MessagePacket>
{
    //GET MAX PACKET SIZE
    let max_packet_size: usize;

    #[cfg(feature = "server")]
    {
        max_packet_size = config::server_config("max_packet_size").parse::<usize>().unwrap();
    }

    #[cfg(not(feature = "server"))]
    {
        max_packet_size = config::client_config("max_packet_size").parse::<usize>().unwrap();
    }

    //READ
    let mut reader = BufReader::new(&mut *stream).take(max_packet_size as u64 + 16);
    let mut packet = String::new();

    //LOOP UNTIL MESSAGE ARRIVES
    while packet.is_empty()
    {
        match reader.read_line(&mut packet)
        {
            Ok(0) | Err(_) => //CLIENT DISCONNECTED
            {
                #[cfg(feature = "server")]
                {
                    remove_connection(stream, DisconnectType::Forcefully);
                }

                return None;
            },

            Ok(_i) => //VALID MESSAGE
            {
                #[cfg(feature = "server")]
                {
                    if _i >= max_packet_size //INPUT TOO LONG
                    {
                        remove_connection(stream, DisconnectType::Gracefully);
                        return None;
                    }
                }
            }
        }
    }

    //DECODE PACKET (BASE91)
    let mut decoded_packet = base91::slice_decode(packet.trim().as_bytes());

    //DECRYPT
    if let Some(key) = key
    {
        //DECRYPT
        let decrypted_packet = decrypter::decrypt_string(options::EncryptedData
        {
            output: str_to_grids(decoded_packet)?, //CONVERT decoded_packet FROM Vec<u8> TO Vec<Grid>
            key: misc::shape_key(key.clone()),
        });

        //OVERWRITE decoded_packet
        decoded_packet = base91::slice_decode(decrypted_packet.as_bytes());
    }

    //ACTIVITY TIMER ON SERVER
    #[cfg(feature = "server")]
    {
        let mut connections = CONNECTIONS.write().unwrap(); //WRITE LOCK
        let peer_addr = stream.peer_addr().unwrap(); //GET CURRENT PEER ADDRESS

        //FIND CONNECTION AND SET last_activity
        for conn in connections.iter_mut()
        {
            if conn.stream().lock().unwrap().peer_addr().unwrap() == peer_addr //CONNECTION FOUND
            {
                match conn
                {
                    Connection::Authenticated { last_activity, .. } | Connection::NonAuthenticated { last_activity, .. } =>
                    {
                        *last_activity = Instant::now(); //RESET last_activity
                    },
                }

                break;
            }
        }
    }

    //DECODE AND RETURN
    Some(bincode::serde::decode_from_slice::<MessagePacket, _>(&decoded_packet, bincode::config::standard()).expect("Decoding packet failed").0)
}

pub fn clear_lines(n: usize) //CLEARS n LINES (ALSO MOVES THE CURSOR n LINES UP)
{
    for i in 0..n
    {
        //CLEAR CURRENT LINE
        print!("\x1B[2K\r");

        //MOVE UP
        if i < n - 1
        {
            print!("\x1B[1A");
        }
    }
}

#[cfg(feature = "server")]
pub fn disconnect_all() //DISCONNECT ALL CLIENTS
{
    //ITERATE OVER ALL STREAMS, REMOVE CONNECTIONS
    let mut streams: Vec<TcpStream> = CONNECTIONS.read().unwrap().iter().map(|conn| conn.cloned_stream().unwrap()).collect();
    for stream in &mut streams
    {
        remove_connection(stream, DisconnectType::Gracefully); //REMOVE GRACEFULLY
    }
}

#[cfg(feature = "server")]
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
