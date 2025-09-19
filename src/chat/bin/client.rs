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
    thread,
    process,
    net::TcpStream,
    io::{ self, Write },
};

use crossterm::
{
    terminal,

    event::
    {
        read,
        Event,
        KeyCode
    },
};

use why2::
{
    core::misc,
    chat::
    {
        command,
        config,
        crypto,
        options,

        network::
        {
            self,
            MessagePacket,
            clear_lines,
        },
    },
};

fn main()
{
    misc::check_version(); //CHECK FOR UPDATES
    config::init_client_config(); //CREATE client.toml CONFIGURATION
    crypto::init_keys(); //GENERATE ECC KEYS
    options::set_core_options(); //SET ENCRYPTION OPTIONS

    println!("Welcome.\n");

    //GET CONNECTING IP
    let mut connecting_ip = if config::client_config("auto_connect") == "true" //USER ENABLED AUTOMATIC CONNECTION
    {
        let ip = config::client_config("auto_connect_ip"); //USE CONFIG IP

        //PRINT OUT IP
        println!(">>> {ip}");
        io::stdout().flush().unwrap();

        ip
    } else //NO AUTO CONNECT
    {
        print!("Enter IP Address:\n>>> ");
        io::stdout().flush().unwrap();

        //GET IP FROM USER INPUT
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        input.trim().to_owned()
    };

    //SPACER ("=") TO BE ADDED
    let mut spacer_add_spaces = 4;

    //ADD PORT TO IP IF MISSING
    if !connecting_ip.contains(':')
    {
        //APPEND DEFAULT PORT TO connecting_ip
        connecting_ip.push_str(&format!(":{}", config::client_config("default_port")));
    } else
    {
        spacer_add_spaces += connecting_ip.len() - connecting_ip.find(":").unwrap();
    }

    //PRINT SPACER
    println!("{}", "=".repeat(connecting_ip.find(":").unwrap() + spacer_add_spaces));

    //CONNECT TO SERVER
    let mut stream = TcpStream::connect(connecting_ip).unwrap_or_else(|_|
    {
        eprintln!("\nConnecting failed.");
        process::exit(1);
    });

    //CLONE SOCKET FOR CLIENT INPUT
    let mut client_stream = stream.try_clone().expect("Failed cloning stream");

    //LISTEN TO SERVER
    thread::spawn(move || network::listen_server(&mut stream));

    //ENABLE RAW MODE
    terminal::enable_raw_mode().unwrap();

    //LOOP FOR CLIENT-SIDE USER INPUT
    loop
    {
        //CREATE/RESET PARTIAL INPUT VARIABLES
        *options::INPUT_READ.lock().unwrap() = String::new(); //RESET INPUT_READ
        let mut input = String::new();
        let mut cursor_position = 0;

        //READ STDIN
        loop
        {
            if let Event::Key(key_event) = read().unwrap()
            {
                match key_event.code
                {
                    //CHAR INPUT, APPEND
                    KeyCode::Char(c) =>
                    {
                        input.insert(cursor_position, c); //LOCAL VARIABLE
                        options::INPUT_READ.lock().unwrap().insert(cursor_position, c); //GLOBAL VARIABLE
                        cursor_position += 1; //CURSOR

                        print!("\x1B[0K");

                        //PRINT ENTERED CHAR
                        if !options::get_asking_password() //DO NOT PRINT PASSWORD AS TEXT
                        {
                            print!("{}", &input[(cursor_position - 1)..]);
                        } else //PRINT PASSWORD AS ASTERISKS
                        {
                            print!("{}", "*".repeat((input.len() - cursor_position) + 1));
                        }

                        //MOVE CURSOR BACK WHERE IS SHOULD BE
                        let tail_len = input.len() - cursor_position;
                        if tail_len > 0
                        {
                            print!("\x1B[{}D", tail_len);
                        }
                    },

                    //CONTROL CHARACTERS
                    KeyCode::Backspace => //BACKSPACE - REMOVE LAST CHAR
                    {
                        if cursor_position > 0
                        {
                            cursor_position -= 1; //CURSOR
                            input.remove(cursor_position); //LOCAL VARIABLE
                            options::INPUT_READ.lock().unwrap().remove(cursor_position); //GLOBAL VARIABLE

                            //MOVE CURSOR TO LEFT AND DELETE REST OF THE LINE
                            print!("\x1B[1D\x1B[0K");

                            //PRINT REMAINING CHARS
                            if !options::get_asking_password()
                            {
                                print!("{}", &input[cursor_position..]);
                            } else
                            {
                                print!("{}", "*".repeat(input.len() - cursor_position));
                            }

                            //MOVE CURSOR BACK WHERE IS SHOULD BE
                            let tail_len = input.len() - cursor_position;
                            if tail_len > 0
                            {
                                print!("\x1B[{}D", tail_len);
                            }
                        }
                    },

                    KeyCode::Enter => break, //ENTER PRESSED, FINALIZE
                    _ => {} //idk
                }

                io::stdout().flush().unwrap();
            }
        }

        //USER COMMANDS
        if let (Some(command), parameters) = command::get_command(&input.to_uppercase())
        {
            network::send(&mut client_stream, MessagePacket //SEND COMMAND
            {
                text: parameters,
                username: None,
                code: Some(command),
            }, options::get_shared_key().as_deref());
        }

        //USER ENTERED USERNAME - STORE
        if options::get_asking_username()
        {
            options::set_username(input.clone()); //STORE USERNAME
            options::set_asking_username(false); //DISABLE ASKING_USERNAME
        }

        //USER ENTERED PASSWORD - HASH
        if options::get_asking_password()
        {
            //CHECK LENGTH
            if input.len() <= options::MIN_PASSWORD_LEN
            {
                print!("\x1B[2FPassword too short! Enter at least {} characters.\x1B[3E", options::MIN_PASSWORD_LEN);
                clear_lines(1);
                print!(">>> ");

                io::stdout().flush().unwrap();
                continue;
            }

            //HASH
            input = crypto::sha256(&input);
            options::set_asking_password(false); //ENABLE ECHO
        }

        //SEND input TO SERVER
        network::send(&mut client_stream, MessagePacket
        {
            text: Some(input),
            username: None,
            code: None,
        }, options::get_shared_key().as_deref());
    }
}
