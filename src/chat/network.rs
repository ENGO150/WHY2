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
    collections::HashSet,
    net::TcpStream,

    sync::
    {
        Arc,
        Mutex,
        RwLock,
    },

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
use serde_json::{ json, Value };

use once_cell::sync::Lazy;

use crossterm::terminal;

use crate::
{
    chat::
    {
        config,
        crypto,
        options as chat_options,
    },
    core::
    {
        encrypter,
        decrypter,
        options,
    },
};

//STRUCTS
#[derive(Serialize, Deserialize, PartialEq, Clone)]
pub enum MessageCode //CONTROL CODES
{
    ClientServerKE, //CLIENT -> SERVER | KEY EXCHANGE
    ServerClientKE, //SERVER -> CLIENT | KEY EXCHANGE
    Welcome,        //SERVER -> CLIENT | INFORMATIONS
    Disconnect,     //SERVER <> CLIENT | QUIT COMMUNICATION
    Username,       //SERVER -> CLIENT | PICK USERNAME
    PasswordL,      //SERVER -> CLIENT | LOGIN
    PasswordR,      //SERVER -> CLIENT | REGISTER
    Accept,         //SERVER -> CLIENT | START CHATTING
    Join,           //SERVER -> CLIENT | CLIENT JOIN MESSAGE
    Leave,          //SERVER -> CLIENT | CLIENT LEAVE MESSAGE
    List,           //CLIENT <> SERVER | PRINT CONNECTED USERS
}

#[derive(Serialize, Deserialize)]
pub struct MessagePacket //MESSAGE PACKET (WHAT IS BEING SENT)
{
    pub text: Option<String>,      //MESSAGE
    pub username: Option<String>,  //USERNAME (SENT ONLY BY SERVER, AS SERVER DOESN'T ACCEPT USERNAMES FROM CLIENT)
    pub id: Option<usize>,         //ID OF USER
    pub code: Option<MessageCode>, //CONTROL CODE
}

struct Connection //CLIENT CONNECTION (WHAT IS PUSHED TO connections LIST)
{
    stream: Arc<Mutex<TcpStream>>, //STREAM
    username: String,              //USERNAME
    id: usize,                     //ID OF USER
    shared_key: String,            //SHARED KEY BETWEEN SERVER AND CLIENT (one to one)
}

//LISTS
static CONNECTIONS: Lazy<Arc<RwLock<Vec<Connection>>>> = Lazy::new(|| //LIST FOR EACH CLIENT CONNECTION
{
    Arc::new(RwLock::new(Vec::new()))
});

//PRIVATE
fn send_code(stream: &mut TcpStream, text: Option<String>, code: MessageCode, shared_key: Option<&str>) //SEND CODE TO CLIENT
{
    send(stream, MessagePacket
    {
        text: text,
        username: Some(config::server_config("server_username")),
        id: None,
        code: Some(code),
    }, shared_key);
}

fn key_exchange_client(stream: &mut TcpStream) -> String //KEY EXCHANGE FOR CLIENT-SIDE
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

fn key_exchange_server(stream: &mut TcpStream) -> Option<String> //KEY EXCHANGE FOR SERVER-SIDE
{
    //WAIT FOR ClientServerKE
    let message = loop
    {
        //READ MESSAGE
        let received = match receive(stream, None)
        {
            Some(r) => r,
            None => return None,
        };

        if received.code == Some(MessageCode::ClientServerKE) && !received.text.is_none() { break received; }
    };

    //SEND ECC PUBKEY TO CLIENT
    send(stream, MessagePacket
    {
        text: Some(crypto::get_public_key()),
        username: Some(config::server_config("server_username")),
        id: None,
        code: Some(MessageCode::ServerClientKE),
    }, None);

    //CALCULATE SHARED SECRET
    Some(crypto::get_shared_key(message.text.unwrap()))
}

fn send_welcome_packet(stream: &mut TcpStream, shared_key: Option<&str>) //send welcome packet you idiot
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
        send(&mut *connection.stream.lock().unwrap(), MessagePacket
        {
            text: message.map(str::to_string),
            username: Some(username.to_string()),
            id: id,
            code: code.clone(),
        }, Some(&connection.shared_key));
    }
}

fn remove_connection(stream: &mut TcpStream, disconnect: bool) //REMOVE CONNECTION BY TcpStream
{
    println!("Closed connection: {}", &stream.peer_addr().unwrap());

    //GET TARGET PEER ADDRESS
    let peer_addr = stream.peer_addr().unwrap();

    //USERNAME OF TARGET, FOR DISCONNECT MESSAGE
    let mut username = String::new();

    //REMOVE MATCHING
    {
        let mut connections = CONNECTIONS.write().unwrap(); //WRITE LOCK

        connections.retain(|conn|
        {
            let mut removed_stream = conn.stream.lock().unwrap();
            let should_remove = removed_stream.peer_addr().unwrap() == peer_addr;

            if should_remove
            {
                //SEND DISCONNECT CODE TO REMOVED CLIENT
                if disconnect
                {
                    send_code(&mut removed_stream, None, MessageCode::Disconnect, Some(&conn.shared_key));
                }

                username = conn.username.clone();
            }

            !should_remove //KEEP NON-MATCHING
        });
    }

    send_to_all(Some(&username), &config::server_config("server_username"), None, Some(MessageCode::Leave));
}

fn user_connected(username: &str) -> bool //CHECK IF CLIENT WITH username IS CONNECTED
{
    CONNECTIONS.read().unwrap().iter().any(|conn| conn.username == username)
}

fn get_latest_id() -> usize
{
    //GET HashSet OF IDS
    let ids: HashSet<usize> = CONNECTIONS.read().unwrap().iter().map(|i| i.id).collect();

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

//PUBLIC
pub fn listen_client(stream: &mut TcpStream) //CLIENT -> SERVER COMMUNICATION
{
    //GET SHARED KEY
    let _shared_key_string = match key_exchange_server(stream)
    {
        Some(r) => r,
        None => return
    };
    let shared_key = Some(_shared_key_string.as_str());

    //SEND PACKET WITH REQUIRED SERVER INFO
    send_welcome_packet(stream, shared_key);

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
        send_code(stream, None, MessageCode::Username, shared_key);

        match receive(stream, shared_key)
        {
            //USERNAME CONDITIONS MET, BREAK LOOP
            Some(r) =>
            {
                if let Some(uname) = r.text
                {
                    if uname.len() >= min_len && uname.len() <= max_len && uname.chars().all(char::is_alphanumeric) && !user_connected(&uname)
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
        send_code(stream, None, MessageCode::Disconnect, shared_key);
        return;
    }

    let username = username.unwrap();

    //ASK FOR PASSWORD
    if !config::server_users_contains(&username) //REGISTRATION
    {
        //SEND REGISTER CODE
        send_code(stream, None, MessageCode::PasswordR, shared_key);

        //WAIT FOR ANSWER
        let response = match receive(stream, shared_key)
        {
            Some(r) => r,
            None => return
        };

        //NO PASSWORD, DISCONNECT CLIENT
        if response.text.is_none()
        {
            send_code(stream, None, MessageCode::Disconnect, shared_key);
            return;
        }

        //SAVE PASSWORD
        config::server_users_write(&username, &response.text.unwrap());
    } else //LOGIN
    {
        //SEND LOGIN CODE
        send_code(stream, None, MessageCode::PasswordL, shared_key);

        //WAIT FOR ANSWER
        let response = match receive(stream, shared_key)
        {
            Some(r) => r,
            None => return
        };

        //INVALID PASSWORD, DISCONNECT CLIENT
        if response.text.is_none() || response.text.unwrap() != config::server_users_config(&username)
        {
            send_code(stream, None, MessageCode::Disconnect, shared_key);
            return;
        }
    }

    //GENERATE ID FOR CLIENT
    let id = get_latest_id();

    //ADD CLIENT TO CONNECTIONS
    {
        //CREATE CONNECTION
        let connection = Connection
        {
            stream: Arc::new(Mutex::new(stream.try_clone().expect("Failed to clone client stream"))),
            username: username.clone(),
            id: id,
            shared_key: shared_key.unwrap().to_string(),
        };

        //PUSH
        CONNECTIONS.write().unwrap().push(connection);
    }

    //TELL CLIENT TO START CHATTING
    send_code(stream, None, MessageCode::Accept, shared_key);

    //SEND JOIN MESSAGE
    send_to_all(Some(&username), &config::server_config("server_username"), None, Some(MessageCode::Join));

    //LOOP READING
    loop
    {
        //READ
        let read = match receive(stream, shared_key)
        {
            Some(r) => r,
            None => return
        };

        //CLIENT CODES
        if read.code.is_some()
        {
            match read.code.unwrap()
            {
                MessageCode::Disconnect => //CLIENT QUITS
                {
                    //DISCONNECT CLIENT
                    remove_connection(stream, true);

                    return;
                },

                MessageCode::List => //CLIENT REQUESTED LIST OF ONLINE USERS
                {
                    let connections = CONNECTIONS.read().unwrap(); //READ LOCK
                    let mut user_list = Vec::new();

                    //ITERATE OVER CONNECTIONS, CREATE JSON OF USERS
                    for connection in connections.iter()
                    {
                        user_list.push(json!({ "username": connection.username, "id": connection.id }));
                    }

                    //SEND LIST BACK TO CLIENT
                    send(stream, MessagePacket
                    {
                        text: Some(json!(user_list).to_string()), //BUILD JSON FROM user_list
                        username: Some(config::server_config("server_username")),
                        id: None,
                        code: Some(MessageCode::List),
                    }, shared_key);

                    continue;
                }

                _ => continue
            }
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
    let mut server_uname: Option<String> = None;

    let mut invalid_username = false; //PRINT "Invalid Username!"

    //FORMATTING SHIT
    let mut first_message = true;
    let mut extra_space: bool;

    //LOOP READING
    loop
    {
        let read = receive(stream, chat_options::get_shared_key().as_deref()).unwrap();
        extra_space = false; //RESET EXTRA SPACE

        //EXTRA SPACE
        if chat_options::get_extra_space() { println!(); }

        //CODES
        if let Some(code) = read.code && (server_uname == None || server_uname == read.username)
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

                    //GET SERVER USERNAME
                    server_uname = read.username;

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

                    println!("[{}]: {} connected.\n", server_uname.as_ref().unwrap(), read.text.unwrap());
                }

                //LEAVE MESSAGE (CLIENT DISCONNECTED)
                MessageCode::Leave =>
                {
                    clear_lines(2);

                    println!("[{}]: {} disconnected.\n", server_uname.as_ref().unwrap(), read.text.unwrap());
                },

                //LIST OF ONLINE USERS
                MessageCode::List =>
                {
                    clear_lines(2);

                    if !chat_options::get_extra_space() { println!(); }
                    println!("Online Users:");

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
                }

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

pub fn send(stream: &mut TcpStream, packet: MessagePacket, key: Option<&str>) //SEND packet TO stream
{
    //ENCODE THE PACKET STRUCT TO Vec<u8>
    let encoded_packet = bincode::serde::encode_to_vec(packet, bincode::config::standard()).expect("Encoding packet failed");
    let mut encoded_packet_string = String::from_utf8(base91::slice_encode(&encoded_packet)).expect("Encoding packet failed"); //ENCODE TO BASE91 STRING

    //ENCRYPT
    if let Some(key) = key
    {
        //ENCRYPT
        let encrypted_packet = encrypter::encrypt_text(&encoded_packet_string, Some(&key)).output.expect("Encrypting packet failed");

        //CONVERT ENCRYPTED PACKET (FROM Vec<i64>) TO Vec<u8>
        let mut encrypted_packet_flattened = Vec::with_capacity(encrypted_packet.len() * 8);
        for num in &encrypted_packet
        {
            encrypted_packet_flattened.extend_from_slice(&num.to_le_bytes()); //FLATTEN i64s to u8s
        }

        //OVERWRITE encoded_packet_string
        encoded_packet_string = String::from_utf8(base91::slice_encode(&encrypted_packet_flattened)).expect("Encoding encrypted packet failed");
    }

    //SEND
    stream.write_all((encoded_packet_string + "\n").as_bytes()).expect("Sending packet failed");
    stream.flush().expect("Flushing stream failed");
}

pub fn receive(stream: &mut TcpStream, key: Option<&str>) -> Option<MessagePacket>
{
    //MAX PACKET SIZE FOR SERVER
    let max_packet_size: usize;
    let is_server = //ONLY APPLY SIZE LIMIT ON SERVER-SIDE
    {
        let connections = CONNECTIONS.read().unwrap(); //READ LOCK

        max_packet_size = config::server_config("max_packet_size").parse::<usize>().unwrap(); //GET MAX SIZE
        !connections.is_empty()
    };

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
                remove_connection(stream, false);
                return None;
            },

            Ok(i) => //VALID MESSAGE
            {
                if is_server && i >= max_packet_size //INPUT TOO LONG
                {
                    remove_connection(stream, true);
                    return None;
                }
            }
        }
    }

    //DECODE PACKET (BASE91)
    let mut decoded_packet = base91::slice_decode(packet.trim().as_bytes());

    //DECRYPT
    if let Some(key) = key
    {
        //CONVERT decoded_packet FROM Vec<u8> TO Vec<i64>
        let recovered_encrypted_packet: Vec<i64> = decoded_packet.chunks_exact(8).map(|chunk|
        {
            //CONVERT chunk TO [u8]
            let mut array = [0u8; 8];
            array.copy_from_slice(chunk);

            //RETURN i64
            i64::from_le_bytes(array)
        }).collect();

        //DECRYPT
        let decrypted_packet = decrypter::decrypt_text(options::EncryptedData
        {
            output: Some(recovered_encrypted_packet),
            key: Some(key.to_owned()),
        }).output.expect("Decrypting packet failed");

        //OVERWRITE decoded_packet
        decoded_packet = base91::slice_decode(decrypted_packet.as_bytes());
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

pub fn disconnect_all() //DISCONNECT ALL CLIENTS
{
    send_to_all(None, &config::server_config("server_username"), None, Some(MessageCode::Disconnect));
}
