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
    env,
    process,
    net::TcpStream,
    io::{ self, Write },
};

use serde_json::Value;

use crossterm::terminal;

use colored::Colorize;

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
        SerColor,
    },
};

//CONSTS
const GRID_W: usize = options::GRID_DIMENSIONS.0;
const GRID_H: usize = options::GRID_DIMENSIONS.1;

//PRIVATE
fn key_exchange(stream: &mut TcpStream) -> (Vec<i64>, Vec<u8>) //KEY EXCHANGE FOR CLIENT-SIDE
{
    //WAIT FOR KeyExchange
    let message = loop
    {
        //READ MESSAGE
        let received = network::receive(stream, None).unwrap();

        if received.code == Some(MessageCode::KeyExchange) { break received; }
    };

    //VERIFY PUBKEY VALIDITY (TOFU)
    match config::server_keys_check(&stream.peer_addr().unwrap().ip().to_string(), message.text.as_ref().unwrap())
    {
        TofuCode::Valid => {},

        status @ (TofuCode::Mismatch | TofuCode::Unknown(_)) =>
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
            match status
            {
                TofuCode::Mismatch => //SOMETHING FUNNY HAPPENING
                {
                    println!
                    (
                        "\n\rSECURITY WARNING: SERVER IDENTITY MISMATCH
                        \n\rThe server's identity key is different from the
                        \rkey stored in local configuration. This could
                        \rmean that someone is intercepting your connection
                        \r(Man-in-the-Middle attack) or that the server
                        \rkey has been changed.
                        \n\rConnection aborted to protect your privacy."
                    );
                },

                TofuCode::Unknown(hash) => //NEW ONE
                {
                    println!
                    (
                        "\n\rSECURITY WARNING: UNKNOWN SERVER IDENTITY
                        \n\rThe server's identity key is not stored in local
                        \rconfiguration. If you are sure that the key below
                        \ris valid, enter following command and connect again.
                        \n\r{} --verify {} {hash}",

                        env::args().nth(0).unwrap(), stream.peer_addr().unwrap().ip()
                    );
                },

                _ => panic!("what") //what
            }

            //EXIT
            process::exit(0);
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
    crypto::derive_shared_secret::<GRID_W, GRID_H>(sk, message.text.unwrap()).expect("Shared secret derivation failed")
}

fn colorize(text: String, color: Option<SerColor>) -> String //COLORIZE text IF PASSED COLOR
{
    match color
    {
        Some(c) if !config::client_config::<bool>("disable_colors") => text.color(c.0).to_string(),
        _ => text
    }
}

//PUBLIC
pub fn listen_server(stream: &mut TcpStream) //SERVER -> CLIENT COMMUNICATION
{
    //SET GLOBAL CLIENT ENCRYPTION & MAC KEY
    let keys = key_exchange(stream);
    options::set_keys(keys.clone());

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

    //LOOP READING
    loop
    {
        let read = match network::receive(stream, Some(&keys))
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

                    //ALLOW MESSAGE HISTORY & COMMANDS
                    options::set_sending_messages(true);
                },

                //JOIN MESSAGE (CLIENT CONNECTED)
                MessageCode::Join =>
                {
                    misc::clear_lines(2);

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
                    misc::clear_lines(2);

                    println!("[{}]: {} disconnected.\n", read.username.unwrap(), read.text.unwrap());
                },

                //CHANNEL CHANGE
                MessageCode::Channel =>
                {
                    channel = if let Some(c) = read.text
                    {
                        format!("#{c} | ")
                    } else
                    {
                        String::new()
                    };

                    misc::clear_lines(1);
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
            misc::clear_lines(2);

            println!
            (
                "{}{}: {}\n",

                colorize(read.username.unwrap(), read.colors.username_color),                                      //USERNAME
                if config::client_config("show_id") { format!(" ({})", read.id.unwrap()) } else { String::new() }, //ID
                colorize(read.text.unwrap(), read.colors.message_color)                                            //MESSAGE
            );
        }

        //PRINT INPUT PROMPT
        print!("\r{}>>> {}", channel, options::INPUT_READ.lock().unwrap().iter().collect::<String>());
        io::stdout().flush().unwrap();
        if !extra_space { options::set_extra_space(false); } //DISABLE EXTRA SPACE
    }
}
