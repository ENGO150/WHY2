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
    io::{ self, Write },
};

use serde_json::Value;

use crossterm::terminal;

use crate::chat::
{
    crypto,
    options,
    network::
    {
        self,
        MessageCode,
        MessagePacket,
    },
};

//PRIVATE
fn key_exchange(stream: &mut TcpStream) -> Vec<i64> //KEY EXCHANGE FOR CLIENT-SIDE
{
    //SEND ECC PUBKEY TO SERVER
    network::send(stream, MessagePacket
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
        let received = network::receive(stream, None).unwrap();

        if received.code == Some(MessageCode::ServerClientKE) { break received; }
    };

    //CALCULATE SHARED SECRET
    crypto::get_shared_key(message.text.unwrap())
}

//PUBLIC
pub fn listen_server(stream: &mut TcpStream) //SERVER -> CLIENT COMMUNICATION
{
    //SET GLOBAL CLIENT SHARED KEY
    options::set_shared_key(key_exchange(stream));

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
        let read = network::receive(stream, options::get_shared_key().as_ref()).unwrap();
        extra_space = false; //RESET EXTRA SPACE

        //EXTRA SPACE
        if options::get_extra_space() { println!(); }

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
                    network::clear_lines(2);

                    //INVALID UNAME
                    if invalid_username
                    {
                        network::clear_lines(2);
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
                    network::clear_lines(3);
                    options::set_asking_password(true);
                    println!("\nEnter password: (REGISTER)");
                },

                //LOGIN
                MessageCode::PasswordL =>
                {
                    network::clear_lines(3);
                    options::set_asking_password(true);
                    println!("\nEnter password: (LOGIN)");
                },

                //START CHATTING
                MessageCode::Accept =>
                {
                    network::clear_lines(3);
                    println!("Login successful. Press Ctrl+H for help.\n");
                },

                //JOIN MESSAGE (CLIENT CONNECTED)
                MessageCode::Join =>
                {
                    network::clear_lines(2);

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
                    network::clear_lines(2);

                    println!("[{}]: {} disconnected.\n", read.username.unwrap(), read.text.unwrap());
                },

                //LIST OF ONLINE USERS
                MessageCode::List =>
                {
                    network::clear_lines(2);

                    if !options::get_extra_space() { println!(); }
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
                    options::set_extra_space(true);
                },

                //PRIVATE MESSAGE INCOMING
                MessageCode::PrivateMessage =>
                {
                    network::clear_lines(2);
                    println!("[PM FROM] {} ({}): {}\n", read.username.unwrap(), read.id.unwrap(), read.text.unwrap());
                },

                //PRIVATE MESSAGE INCOMING
                MessageCode::PrivateMessageBack =>
                {
                    network::clear_lines(2);
                    println!("[PM TO] {} ({}): {}\n", read.username.unwrap(), read.id.unwrap(), read.text.unwrap());
                },

                //SPAM WARNING
                MessageCode::SpamWarning =>
                {
                    network::clear_lines(2);
                    println!("Slow down! You're sending messages too quickly.\n");
                }

                //CLIENT MESSED SOME COMMAND UP
                MessageCode::InvalidUsage =>
                {
                    network::clear_lines(2);
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
            network::clear_lines(2);

            println!("{} ({}): {}\n", read.username.unwrap(), read.id.unwrap(), read.text.unwrap());
        }

        //PRINT INPUT PROMPT
        print!("\r>>> {}", options::INPUT_READ.lock().unwrap().iter().collect::<String>());
        io::stdout().flush().unwrap();
        if !extra_space { options::set_extra_space(false); } //DISABLE EXTRA SPACE
    }
}
