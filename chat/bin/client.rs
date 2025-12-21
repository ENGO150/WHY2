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
    env,
    thread,
    process,
    io::{ self, Write },
    net::TcpStream,
    sync::
    {
        LazyLock,
        Arc,
        Mutex,
    },
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

use colored::Color;

use why2::chat::
{
    config,
    options,
    misc,
    command::{ self, Command },
    network::
    {
        self,
        MessagePacket,
        MessageColors,
        SerColor,
        client,
    },
};

//STRUCTS
struct RawModeGuard;

//GLOBAL VARIABLES
static INPUT_HISTORY: LazyLock<Arc<Mutex<(Vec<String>, usize)>>> = LazyLock::new(|| //INPUTS READ FROM CLIENT
{
    Arc::new(Mutex::new((Vec::new(), 0)))
});

//IMPLEMENTATIONS
impl RawModeGuard
{
    fn enable() -> Result<Self, std::io::Error> //ENABLES RAW MODE AND RETURNS A GUARD THAT WILL DISABLE IT ON DROP
    {
        terminal::enable_raw_mode()?;
        Ok(RawModeGuard)
    }
}

impl Drop for RawModeGuard
{
    fn drop(&mut self)
    {
        let _ = terminal::disable_raw_mode(); //IGNORE ERRORS
    }
}

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
            //INGORE KEY RELEASE
            if key_event.is_release() { continue; }

            //CTRL SHORTCUTS
            if key_event.modifiers.contains(KeyModifiers::CONTROL)
            {
                match key_event.code
                {
                    //CTRL+L (LIST)
                    KeyCode::Char('l') =>
                    {
                        misc::clear_lines(1);
                        return Command::List.to_string();
                    },

                    //CTRL+C (EXIT)
                    KeyCode::Char('c') =>
                    {
                        misc::clear_lines(2);
                        return Command::Exit.to_string();
                    },

                    //CTRL+H (HELP)
                    KeyCode::Char('h') =>
                    {
                        return Command::Help.to_string();
                    },

                    //CTRL+A (MOVE TO LINE START)
                    KeyCode::Char('a') =>
                    {
                        if cursor_position > 0
                        {
                            print!("\x1B[{}D", cursor_position);
                            cursor_position = 0;
                        }
                    },

                    //CTRL+E (MOVE TO LINE END)
                    KeyCode::Char('e') =>
                    {
                        let input_len = input.len();

                        if cursor_position < input_len
                        {
                            print!("\x1B[{}C", input_len - cursor_position);
                            cursor_position = input_len;
                        }
                    },

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
                        cursor_position = input.len(); //CURSOR

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
                        cursor_position = input.len(); //CURSOR

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

fn send_command_code(stream: &mut TcpStream, command: &Command, parameters: &Option<String>) -> bool //SEND CODE FROM COMMAND IF POSSIBLE
{
    //CODE COMMAND
    if let Some(code) = command.to_code()
    {
        network::send(stream, MessagePacket
        {
            text: parameters.clone(),
            code: Some(code),
            ..Default::default()
        }, options::get_keys().as_ref());
        return true;
    }

    return false;
}

fn to_color(color: &str) -> Result<SerColor, ()>
{
    color.parse::<Color>().map(SerColor)
}

fn color_handler(config_key: &str, parameters: Option<String>) //HANDLE COLOR CHANGE
{
    let message: &str;

    //CHECK FOR PARAMETERS
    if let Some(parameters) = parameters
    {
        //CHECK FOR COLOR VALIDITY
        if to_color(&parameters).is_ok()
        {
            //SAVE COLOR TO CONFIG
            config::client_write(config_key, &parameters.to_lowercase());

            message = "Color set successfully.";
        } else
        {
            message = "Invalid color! See \x1b]8;;https://docs.rs/colored/latest/colored/enum.Color.html\x1b\\colored API\x1b]8;;\x1b\\ for help.";
        }
    } else
    {
        message = "Invalid usage! Press Ctrl+H for help.";
    }

    //PRINTOUT RESULT
    misc::clear_lines(2);
    print!("{message}\n\n\r>>> ");
}

fn get_colors() -> MessageColors //READ COLORS FROM CONFIG
{
    MessageColors
    {
        username_color: to_color(&config::client_config::<String>("username_color")).ok(),
        message_color: to_color(&config::client_config::<String>("message_color")).ok(),
    }
}

fn main()
{
    //CONFIGURATION
    misc::check_version(); //CHECK WHY2 VERSION
    config::init_client_config(); //CREATE client.toml CONFIGURATION

    //CHECK FOR PARAMETERS
    if let Some(arg) = env::args().nth(1)
    {
        if arg == "--verify" && env::args().len() == 4 //SAVE SERVER PUBKEY
        {
            config::server_keys_save(&env::args().nth(2).unwrap(), &env::args().nth(3).unwrap());
            println!("Key saved.");
        } else //INVALID CMD
        {
            println!("Invalid command! Aborting...");
        }

        return;
    }

    println!("Welcome.\n");

    //GET CONNECTING IP
    let mut connecting_ip = if config::client_config::<bool>("auto_connect") //USER ENABLED AUTOMATIC CONNECTION
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
        connecting_ip.push_str(&format!(":{}", config::client_config::<u16>("default_port")));
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

    //SET TCP_NODELAY
    stream.set_nodelay(true).expect("Failed to set TCP_NODELAY");

    //CLONE SOCKET FOR CLIENT INPUT
    let mut client_stream = stream.try_clone().expect("Failed cloning stream");

    //LISTEN TO SERVER
    thread::spawn(move || client::listen_server(&mut stream));

    //ENABLE RAW MODE
    let _raw_mode_guard = RawModeGuard::enable().unwrap();

    //LOOP FOR CLIENT-SIDE USER INPUT
    loop
    {
        //FLUSH STDOUT
        io::stdout().flush().unwrap();

        //READ STDIN
        let input = read_input();

        //APPEND MESSAGE TO HISTORY
        if options::get_sending_messages()
        {
            //USER COMMANDS
            let mut command_used = false;
            if let (Some(command), parameters) = command::get_command(&input)
            {
                //SEND CODE ON A SIMPLE COMMAND, CONTINUE OTHERWISE
                if !send_command_code(&mut client_stream, &command, &parameters)
                {
                    match command
                    {
                        //HELP
                        Command::Help =>
                        {
                            misc::clear_lines(2);
                            options::set_extra_space(true); //ADD EXTRA NEWLINE ON NEXT RECEIVED MESSAGE

                            print!
                            (
                                "\nCommands:
                                \r/help - Prints this
                                \r/channel [NAME] - Switches to channel/lobby if NAME is omitted
                                \r/list - Show connected users and their IDs
                                \r/pm (ID) (MESSAGE) - Sends private message
                                \r/ucolor (COLOR) - Sets color of username
                                \r/color (COLOR) - Sets color of message
                                \r/exit - Disconnects from server
                                \n\r>>> "
                            );
                        },

                        Command::UsernameColor =>
                        {
                            color_handler("username_color", parameters);
                        },

                        Command::MessageColor =>
                        {
                            color_handler("message_color", parameters);
                        },

                        //INVALID COMMAND
                        Command::Invalid =>
                        {
                            misc::clear_lines(2);
                            print!("Invalid command! Press Ctrl+H for help.\n\n\r>>> ");
                        },

                        //NON IMPLEMENTED COMMAND
                        _ => panic!("Invalid command")
                    }
                }

                command_used = true;
            }

            let mut history = INPUT_HISTORY.lock().unwrap();

            //ADD INPUT
            if history.0.last() != Some(&input)
            {
                history.0.push(input.clone());
            }

            //RESET HISTORY POSITION
            history.1 = history.0.len();

            if command_used { continue }; //DO NOT SEND COMMAND STRING
        }

        //DISABLE ASKING_PASSWORD
        options::set_asking_password(false);

        //SEND input TO SERVER
        network::send(&mut client_stream, MessagePacket
        {
            text: Some(input),
            colors: get_colors(),
            ..Default::default()
        }, options::get_keys().as_ref());
    }
}
