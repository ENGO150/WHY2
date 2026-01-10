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
    thread,
    process,
    net::TcpStream,
    sync::mpsc::Sender,
};

use zeroize::Zeroizing;

use serde_json::Value;

use crossterm::terminal;

use crate::chat::
{
    crypto,
    options,
    misc,
    config::{ self, TofuCode },
    network::
    {
        self,
        MessageCode,
        MessagePacket,
        voice::
        {
            client as voice_client,
            options as voice_options,
        }
    },
};

//CONSTS
const GRID_W: usize = options::GRID_DIMENSIONS.0;
const GRID_H: usize = options::GRID_DIMENSIONS.1;

//ENUMS
pub enum ClientEvent
{
    Message(MessagePacket), //RECEIVED MESSAGE
    Prompt(String, String), //">>>" PROMPT, WITH CHANNEL AND WRITTEN MESSAGE
    TofuError(TofuCode),    //TOFU VERIFICATION FAILED
}

//FUNCTIONS
//PRIVATE
fn key_exchange(stream: &mut TcpStream, buffer: &mut Vec<u8>, keys: &mut options::SharedKeys, tx: &Sender<ClientEvent>) -> bool //KEY EXCHANGE FOR CLIENT-SIDE
{
    //WAIT FOR KeyExchange
    let message = loop
    {
        //READ MESSAGE
        let received = network::receive(stream, buffer, None).unwrap();

        if received.code == Some(MessageCode::KeyExchange) { break received; }
    };

    //VERIFY PUBKEY VALIDITY (TOFU)
    match config::server_keys_check(&stream.peer_addr().unwrap().ip().to_string(), message.text.as_ref().unwrap())
    {
        TofuCode::Valid => {},

        status @ (TofuCode::Mismatch | TofuCode::Unknown(_, _)) =>
        {
            //GRACEFULLY DISCONNECT FROM SERVER
            network::send(stream, MessagePacket
            {
                code: Some(MessageCode::Disconnect),
                ..Default::default()
            }, None);

            //DISABLE RAW MODE
            terminal::disable_raw_mode().unwrap();

            //PRINT SECURITY MESSAGE
            tx.send(ClientEvent::TofuError(status)).unwrap();

            //EXIT
            return false;
        },
    }

    //GENERATE EPHEMERAL KEYS
    let (sk, pk) = crypto::generate_ephemeral_keys();

    //SEND ECC PUBKEY TO SERVER
    network::send(stream, MessagePacket
    {
        text: Some(pk),
        code: Some(MessageCode::KeyExchange),
        ..Default::default()
    }, None);

    //CALCULATE SHARED SECRET
    *keys = crypto::derive_shared_secret::<GRID_W, GRID_H>(sk, message.text.unwrap()).expect("Shared secret derivation failed");

    //SET GLOBAL KEYS VARIABLE
    options::set_keys(keys.clone());

    true
}

//PUBLIC
pub fn listen_server(stream: &mut TcpStream, tx: Sender<ClientEvent>) //SERVER -> CLIENT COMMUNICATION
{
    //CREATE PERSISTENT BUFFER
    let mut buffer = Vec::new();

    //SET GLOBAL CLIENT ENCRYPTION & MAC KEY
    let mut keys = (Zeroizing::new(vec![]), Zeroizing::new(vec![]));
    if !key_exchange(stream, &mut buffer, &mut keys, &tx) { return; }

    //SERVER INFO VARIABLES
    let mut min_pass: Option<u64> = None;
    let mut max_uname: Option<u64> = None;
    let mut min_uname: Option<u64> = None;
    let mut server_name: &str;

    let mut invalid_username = false; //PRINT "Invalid Username!"
    let mut invalid_password = false;

    let mut disabled_registration = false; //PRINT "Registration disabled!"

    //FORMATTING SHIT
    let mut first_message = true;
    let mut extra_space: bool;

    let mut channel = String::new();

    //CONNECTION PROPERTIES
    let mut id = 0usize; //ID SET BY SERVER
    let mut username: Option<String> = None;

    //LOOP READING
    loop
    {
        let read = match network::receive(stream, &mut buffer, Some(&keys))
        {
            Some(packet) => packet,
            None => continue
        };

        extra_space = false; //RESET EXTRA SPACE

        //EXTRA SPACE
        if options::get_extra_space() { println!(); }

        //CODES
        if let Some(code) = read.code
        {
            match code
            {
                //VERSION CHECK
                MessageCode::Version =>
                {
                    let version = misc::get_version().to_string();
                    let server_version = read.text.unwrap();

                    //NON MATCHING VERSION (WILL GET DISCONNECTED)
                    if server_version != version
                    {
                        misc::clear_lines(1);
                        println!("Incompatible version! ({version}/{server_version})");
                    }

                    //RESPOND
                    network::send(stream, MessagePacket
                    {
                        text: Some(version),
                        code: Some(MessageCode::Version),
                        ..Default::default()
                    }, Some(&keys));

                    continue;
                }

                //WELCOME CODE - SERVER INFORMATIONS
                MessageCode::Welcome =>
                {
                    //PARSE JSON
                    let welcome_json: Value = serde_json::from_str(&read.text.unwrap()).expect("Parsing welcome json failed"); //PARSE WELCOME JSON

                    //GET INFO FROM JSON
                    min_pass = Some(welcome_json["min_pass"].as_u64().expect("Invalid welcome json"));
                    max_uname = Some(welcome_json["max_uname"].as_u64().expect("Invalid welcome json"));
                    min_uname = Some(welcome_json["min_uname"].as_u64().expect("Invalid welcome json"));
                    server_name = welcome_json["server_name"].as_str().expect("Invalid welcome json");

                    println!("Successfully connected to {server_name}.\n");
                },

                //REKEY - CHANGE KEYS
                MessageCode::Rekey =>
                {
                    //WAIT FOR SERVER TO INIT KEY EXCHANGE
                    key_exchange(stream, &mut buffer, &mut keys, &tx);
                }

                //PICK_USERNAME CODE - guess what
                MessageCode::Username =>
                {
                    misc::clear_lines(2);

                    //INVALID UNAME
                    if invalid_username
                    {
                        misc::clear_lines(2);
                        print!("Username rejected!");
                    } else //VALID
                    {
                        //SET INVALID USERNAME FOR POSSIBLE NEXT CODE
                        invalid_username = true;
                    }

                    println! //TODO: Fix flushing
                    (
                        "\n\rEnter username ({}):",

                        if disabled_registration
                        {
                            String::from("Registration disabled!")
                        } else
                        {
                            format!("a-Z, 0-9; {}-{} characters", min_uname.unwrap(), max_uname.unwrap())
                        }
                    );
                },

                //REGISTER
                MessageCode::PasswordR =>
                {
                    misc::clear_lines(3);
                    options::set_asking_password(true);

                    //INVALID PASS
                    if invalid_password
                    {
                        print!("Password rejected! Enter at least {} characters.", min_pass.unwrap());
                    } else
                    {
                        invalid_password = true;
                    }

                    println!("\n\rEnter password: (REGISTER)");
                },

                //LOGIN
                MessageCode::PasswordL =>
                {
                    misc::clear_lines(3);
                    options::set_asking_password(true);
                    println!("\nEnter password: (LOGIN)");
                },

                //START CHATTING
                MessageCode::Accept =>
                {
                    misc::clear_lines(3);
                    println!("Login successful. Press Ctrl+H for help.\n");

                    //SET SERVER-SIDE ID
                    id = read.text.unwrap_or("0".to_string()).parse().unwrap();

                    //ALLOW MESSAGE HISTORY & COMMANDS
                    options::set_sending_messages(true);
                },

                //JOIN MESSAGE (CLIENT CONNECTED)
                MessageCode::Join =>
                {
                    misc::clear_lines(2);

                    let user = read.text.unwrap();

                    if first_message
                    {
                        println!();
                        username = Some(user.clone());
                        first_message = false;
                    }

                    println!("[{}]: {} connected.\n", read.username.unwrap(), user);
                }

                //LEAVE MESSAGE (CLIENT DISCONNECTED)
                MessageCode::Leave =>
                {
                    misc::clear_lines(2);
                    println!("[{}]: {} disconnected.\n", read.username.unwrap(), read.text.unwrap());
                    voice_client::remove_consumer(&read.id.unwrap());
                },

                //CHANNEL CHANGE
                MessageCode::Channel =>
                {
                    //REMOVE ALL STORED VOICE CLIENTS
                    voice_client::remove_all_consumers();

                    channel = if let Some(c) = read.text
                    {
                        format!("#{c} | ")
                    } else
                    {
                        String::new()
                    };

                    misc::clear_lines(1);
                },

                //SERVER ALLOWED VOICE
                MessageCode::Voice =>
                {
                    if options::socks5_enabled()
                    {
                        misc::clear_lines(2);
                        println!("Voice chat cannot be enabled while using SOCKS5.\n");
                        continue;
                    }

                    //TOGGLE VOICE
                    let status = if voice_options::swap_use_voice()
                    {
                        let username = username.clone();
                        thread::spawn(move || voice_client::listen_server_voice(id, username.unwrap()));
                        "en"
                    } else
                    {
                        "dis"
                    };

                    //PRINT STATUS
                    misc::clear_lines(2);
                    println!("Voice {}abled.\n", status);
                },

                //VOICE CLIENTS
                MessageCode::VoiceClients =>
                {
                    //PARSE JSON
                    let clients: Vec<(usize, String)> = serde_json::from_str(&read.text.unwrap()).expect("Parsing welcome json failed");

                    //ADD CLIENTS
                    for (id, username) in clients
                    {
                        voice_client::add_consumer(id, username);
                    }
                }

                //CLIENT JOINED VOICE CHANNEL
                MessageCode::ChannelJoin =>
                {
                    let joined_id = read.id.unwrap();
                    if voice_options::get_use_voice() && id != joined_id
                    {
                        voice_client::add_consumer(read.id.unwrap(), read.username.unwrap());
                    }
                },

                //CLIENT LEFT VOICE CHANNEL
                MessageCode::ChannelLeave =>
                {
                    voice_client::remove_consumer(&read.id.unwrap());
                },

                //LIST OF ONLINE USERS
                MessageCode::List =>
                {
                    misc::clear_lines(2);

                    if !options::get_extra_space() { println!(); }
                    println!("Online users:");

                    //PARSE JSON
                    let users_json: Value = serde_json::from_str(&read.text.unwrap()).unwrap();

                    //PRINT USERS
                    for user in users_json.as_array().unwrap()
                    {
                        //GET CHANNEL
                        let c = if let Some(c) = user["channel"].as_str().map(String::from)
                        {
                            format!(" | #{c}")
                        } else
                        {
                            String::new()
                        };

                        println!("\r{} ({}){}", user["username"].as_str().unwrap(), user["id"], c);
                    }

                    println!();

                    extra_space = true;
                    options::set_extra_space(true);
                },

                //PRIVATE MESSAGE INCOMING
                MessageCode::PrivateMessage =>
                {
                    misc::clear_lines(2);
                    println!("[PM FROM] {} ({}): {}\n", read.username.unwrap(), read.id.unwrap(), read.text.unwrap());
                },

                //PRIVATE MESSAGE INCOMING
                MessageCode::PrivateMessageBack =>
                {
                    misc::clear_lines(2);
                    println!("[PM TO] {} ({}): {}\n", read.username.unwrap(), read.id.unwrap(), read.text.unwrap());
                },

                //SPAM WARNING
                MessageCode::SpamWarning =>
                {
                    misc::clear_lines(2);
                    println!("Slow down! You're sending messages too quickly.\n");
                },

                //REGISTRATION DISABLED
                MessageCode::RegisterDisabled =>
                {
                    disabled_registration = true;
                },

                //CLIENT MESSED SOME COMMAND UP
                MessageCode::InvalidUsage =>
                {
                    misc::clear_lines(2);
                    println!("Invalid usage! Press Ctrl+H for help.\n");
                },

                //CLIENTED REQUESTED DISABLED FEATURE
                MessageCode::InvalidFeature =>
                {
                    misc::clear_lines(2);
                    println!("Server has disabled the feature you requested.\n");
                },

                //SERVER DOESN'T LIKE YA ANYMORE - EXIT
                MessageCode::Disconnect =>
                {
                    terminal::disable_raw_mode().unwrap();
                    println!("\nServer quit communication.");
                    process::exit(0);
                },

                _ => continue //EITHER INVALID CODE OR A KEY EXCHANGE CODE
            }
        } else //NO CODE, PRINT MESSAGE
        {
            tx.send(ClientEvent::Message(read)).unwrap();
        }

        //PRINT INPUT PROMPT
        tx.send(ClientEvent::Prompt(channel.clone(), options::INPUT_READ.lock().unwrap().iter().collect::<String>())).unwrap();
        if !extra_space { options::set_extra_space(false); } //DISABLE EXTRA SPACE
    }
}
