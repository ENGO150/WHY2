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

#![cfg(feature = "client")]

use std::
{
    thread,
    process,
    io::{ self, Write },
    sync::{ Arc, Mutex },
    net::TcpStream,
};

use crossterm::
{
    terminal,
    event::
    {
        self,
        KeyCode,
        KeyModifiers,
        Event,
    },
};

use once_cell::sync::Lazy;

use why2::
{
    core::misc,
    chat::
    {
        config,
        crypto,
        options,
        misc as chat_misc,
        command::{ self, Command },
        network::
        {
            self,
            MessageCode,
            MessagePacket,
            client,
        },
    },
};

//GLOBAL VARIABLES
static INPUT_HISTORY: Lazy<Arc<Mutex<(Vec<String>, usize)>>> = Lazy::new(|| //INPUTS READ FROM CLIENT
{
    Arc::new(Mutex::new((Vec::new(), 0)))
});

//HANDLER FNS
fn redraw_removed(input: &Vec<char>, cursor_position: usize) //REDRAW TEXT AFTER CURSOR
{
    //REDRAW INPUT
    if !options::get_asking_password()
    {
        print!("{}", input[cursor_position..].iter().collect::<String>());
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

fn read_input() -> String
{
    //CREATE/RESET PARTIAL INPUT VARIABLES
    *options::INPUT_READ.lock().unwrap() = Vec::new(); //RESET INPUT_READ
    let mut input: Vec<char> = Vec::new();
    let mut cursor_position = 0;

    loop
    {
        if let Event::Key(key_event) = event::read().unwrap()
        {
            //CTRL SHORTCUTS
            if key_event.modifiers.contains(KeyModifiers::CONTROL)
            {
                match key_event.code
                {
                    //CTRL+C (EXIT)
                    KeyCode::Char('c') =>
                    {
                        chat_misc::clear_lines(2);
                        return Command::Exit.to_string();
                    },

                    KeyCode::Char('h') =>
                    {
                        return Command::Help.to_string();
                    }

                    _ => {} //some random shortcut
                };
            } else
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
                            print!("{}", input[(cursor_position - 1)..].iter().collect::<String>());
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
                            redraw_removed(&input, cursor_position);
                        }
                    },

                    KeyCode::Delete => //DELETE - REMOVE NEXT CHAR
                    {
                        if cursor_position < input.len()
                        {
                            input.remove(cursor_position); //LOCAL VARIABLE
                            options::INPUT_READ.lock().unwrap().remove(cursor_position); //GLOBAL VARIABLE

                            //MOVE CURSOR TO LEFT AND DELETE REST OF THE LINE
                            print!("\x1B[0K");

                            //PRINT REMAINING CHARS
                            redraw_removed(&input, cursor_position);
                        }
                    },

                    KeyCode::Left => //ARROW LEFT - MOVE CURSOR
                    {
                        if cursor_position > 0
                        {
                            cursor_position -= 1;
                            print!("\x1B[1D");
                        }
                    },

                    KeyCode::Right => //ARROW RIGHT - MOVE CURSOR
                    {
                        if cursor_position < input.len()
                        {
                            cursor_position += 1;
                            print!("\x1B[1C");
                        }
                    },

                    KeyCode::Up => //ARROW UP - PAGE HISTORY
                    {
                        let mut history = INPUT_HISTORY.lock().unwrap();

                        //SKIP IF ON TOP OF HISTORY
                        if history.0.is_empty() || history.1 == 0 { continue; }

                        //CLEAR CURRENT INPUT
                        if cursor_position > 0
                        {
                            print!("\x1B[{}D\x1B[0K", cursor_position);
                        }

                        //MOVE IN HISTORY
                        history.1 -= 1;

                        let new_input = &history.0[history.1]; //SELECTED INPUT IN HISTORY

                        //REPLACE CURRENT INPUT
                        input = new_input.chars().collect(); //LOCAL VARIABLE
                        *options::INPUT_READ.lock().unwrap() = input.clone(); //GLOBAL VARIABLE
                        cursor_position = new_input.len(); //CURSOR

                        print!("{}", new_input); //PRINT
                    },

                    KeyCode::Down => //ARROW DOWN - PAGE HISTORY
                    { //TODO: Remove duplicity
                        let mut history = INPUT_HISTORY.lock().unwrap();

                        //SKIP IF ON TOP OF HISTORY
                        if history.1 == history.0.len() { continue; }

                        //CLEAR CURRENT INPUT
                        if cursor_position > 0
                        {
                            print!("\x1B[{}D\x1B[0K", cursor_position);
                        }

                        //MOVE IN HISTORY
                        history.1 += 1;

                        //SELECTED INPUT IN HISTORY
                        let new_input = if history.1 < history.0.len()
                        {
                            &history.0[history.1]
                        } else
                        {
                            ""
                        };

                        //REPLACE CURRENT INPUT
                        input = new_input.chars().collect(); //LOCAL VARIABLE
                        *options::INPUT_READ.lock().unwrap() = input.clone(); //GLOBAL VARIABLE
                        cursor_position = new_input.len(); //CURSOR

                        print!("{}", new_input); //PRINT
                    },

                    KeyCode::Enter => break, //ENTER PRESSED, FINALIZE
                    _ => {} //idk
                }
            }

            io::stdout().flush().unwrap();
        }
    }

    input.iter().collect::<String>()
}

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
    thread::spawn(move || client::listen_server(&mut stream));

    //ENABLE RAW MODE
    terminal::enable_raw_mode().unwrap();

    //SENDING MESSAGES BOOL (CONDITION FOR ADDING MESSAGES TO HISTORY)
    let mut sending_messages = false;

    //LOOP FOR CLIENT-SIDE USER INPUT
    loop
    {
        //FLUSH STDOUT
        io::stdout().flush().unwrap();

        //READ STDIN
        let mut input = read_input();

        //USER COMMANDS
        let mut command_used = false;
        if let (Some(command), parameters) = command::get_command(&input)
        {
            match command
            {
                //EXIT
                Command::Exit =>
                {
                    network::send(&mut client_stream, MessagePacket
                    {
                        text: None,
                        username: None,
                        id: None,
                        code: Some(MessageCode::Disconnect),
                    }, options::get_shared_key().as_ref());
                },

                //HELP
                Command::Help =>
                {
                    chat_misc::clear_lines(2);
                    options::set_extra_space(true); //ADD EXTRA NEWLINE ON NEXT RECEIVED MESSAGE

                    print!
                    (
                        "\nCommands:
                        \r/help - Prints this
                        \r/list - Show connected users
                        \r/pm (ID) (MESSAGE) - Sends private message
                        \r/exit - Disconnects from server
                        \n\r>>> "
                    );
                },

                //LIST USERS
                Command::List =>
                {
                    network::send(&mut client_stream, MessagePacket
                    {
                        text: None,
                        username: None,
                        id: None,
                        code: Some(MessageCode::List),
                    }, options::get_shared_key().as_ref());
                },

                //PRIVATE MESSAGE
                Command::PrivateMessage =>
                {
                    network::send(&mut client_stream, MessagePacket
                    {
                        text: parameters,
                        username: None,
                        id: None,
                        code: Some(MessageCode::PrivateMessage),
                    }, options::get_shared_key().as_ref());
                },

                //INVALID COMMAND
                Command::Invalid =>
                {
                    chat_misc::clear_lines(2);
                    print!("Invalid command! Press Ctrl+H for help.\n\n\r>>> ");
                }
            }

            command_used = true;
        }

        //APPEND MESSAGE TO HISTORY
        if sending_messages
        {
            let mut history = INPUT_HISTORY.lock().unwrap();

            //ADD INPUT
            if history.0.last() != Some(&input)
            {
                history.0.push(input.clone());
            }

            //RESET HISTORY POSITION
            history.1 = history.0.len();
        }

        if command_used { continue }; //DO NOT SEND COMMAND STRING

        //USER ENTERED PASSWORD - HASH
        if options::get_asking_password()
        {
            //CHECK LENGTH
            if input.len() <= options::MIN_PASSWORD_LEN
            {
                print!("\x1B[2FPassword too short! Enter at least {} characters.\x1B[3E", options::MIN_PASSWORD_LEN);
                chat_misc::clear_lines(1);
                print!(">>> ");

                continue;
            }

            //HASH
            input = crypto::sha256(&input);
            options::set_asking_password(false); //DISABLE ASKING_PASSWORD
            sending_messages = true; //APPEND NEW MESSAGES TO HISTORY
        }

        //SEND input TO SERVER
        network::send(&mut client_stream, MessagePacket
        {
            text: Some(input),
            username: None,
            id: None,
            code: None,
        }, options::get_shared_key().as_ref());
    }
}
