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

#![cfg(feature = "client")]

//MODULES
pub mod colors;
pub mod ui;

use std::
{
    env,
    thread,
    process,
    fs::File,
    path::Path,
    net::TcpStream,
    time::Duration,
    io::
    {
        self,
        Read,
        Write,
    },
    sync::
    {
        mpsc,
        LazyLock,
        Arc,
        Mutex,
    },
};

use sha2::{ Sha256, Digest };

use socket2::{ Socket, TcpKeepalive };
use socks::Socks5Stream;

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

use why2_chat::
{
    config,
    consts,
    options,
    misc,
    command::
    {
        self,
        Command,
    },
    network::
    {
        self,
        MessagePacket,
        MessageColors,
        FilePayload,
        voice::client::device,
        client::{ self, ClientEvent },
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

fn invalid_usage(subject: Option<&str>) //PRINT 'INVALID' MESSAGE
{
    print!("Invalid {}! Press Ctrl+H for help.\n\n\r>>> ", subject.unwrap_or("usage"))
}

fn mute(parameters: Option<String>) //MUTE LOCAL/PEER CLIENT
{
    ui::clear_lines(2);

    //GET ID PARAMETER
    let id = if let Some(parameters) = parameters
    {
        match parameters.parse::<usize>()
        {
            Ok(i) => Some(i),
            Err(_) => return invalid_usage(None)
        }
    } else { None };

    //INFO LOG
    print!
    (
        "Sucessfully {}muted{}.\n\n\r>>> ",
        if options::toggle_mute(id) { "" } else { "un" },
        if let Some(id) = id
        {
            format!(" ID {id}")
        } else { String::new() }
    );
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

                    //COMMAND SHORTCUTS
                    KeyCode::Char(c) =>
                    {
                        if let Some(command) = command::COMMAND_LIST.iter().find(|i| i.shortcut == Some(c))
                        {
                            match command.command
                            {
                                //CTRL+L (LIST)
                                Command::List =>
                                {
                                    ui::clear_lines(1);
                                },

                                //CTRL+C (EXIT)
                                Command::Exit =>
                                {
                                    ui::clear_lines(2);
                                },

                                _ => {} //NORMAL COMMAND
                            }

                            return command.command.to_string();
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

    //COLLECT AND TRIM (NOT PASSWORDS)
    let read = input.iter().collect::<String>();
    if !options::get_asking_password()
    {
        read.trim().to_string()
    } else
    {
        read
    }
}

fn to_color(color: &str) -> Result<u8, ()> //CONVERT STRING TO COLOR CODE
{
    color.parse::<Color>()
        .map(|c| colors::color_to_u8(&c))
        .map_err(|_| ())
}

fn color_handler(config_key: &str, parameters: Option<String>) //HANDLE COLOR CHANGE
{
    ui::clear_lines(2);

    //CHECK FOR PARAMETERS
    if let Some(parameters) = parameters
    {
        //CHECK FOR COLOR VALIDITY
        print!("{}\n\n\r>>> ", if to_color(&parameters).is_ok()
        {
            //SAVE COLOR TO CONFIG
            config::client_write(config_key, &parameters.to_lowercase());

            "Color set successfully."
        } else
        {
            "Invalid color! See \x1b]8;;https://docs.rs/colored/latest/colored/enum.Color.html\x1b\\colored API\x1b]8;;\x1b\\ for help."
        });
    } else
    {
        invalid_usage(None);
    }
}

fn get_colors() -> MessageColors //READ COLORS FROM CONFIG
{
    MessageColors
    {
        username_color: to_color(&config::read_config::<String>("username_color")).ok(),
        message_color: to_color(&config::read_config::<String>("message_color")).ok(),
    }
}

fn main()
{
    //CREATE CHANNEL
    let (tx, rx) = mpsc::channel::<ClientEvent>();

    //SPAWN PRINTER THREAD
    thread::spawn(move ||
    {
        while let Ok(event) = rx.recv()
        {
            ui::draw_event(event);
        }
    });

    //CONFIGURATION
    misc::check_version(&tx); //CHECK WHY2 VERSION
    config::init_config(); //CREATE client.toml CONFIGURATION

    //CHECK FOR PARAMETERS
    if let Some(arg) = env::args().nth(1)
    {
        if arg == "--verify" && env::args().len() == 4 //SAVE SERVER PUBKEY
        {
            config::server_keys_save(&env::args().nth(2).unwrap(), &env::args().nth(3).unwrap());
            println!("Key saved.");
        } else if arg == "--audio-setup" && env::args().len() == 2
        {
            device::setup_devices();
        } else if arg == "--help" && env::args().len() == 2
        {
            println!
            (
                "WHY2 Chat Client\n\
                ================\n\n\
                Usage: why2 [options]\n\n\
                --verify (HOST) (PUBKEY HASH) - Whitelist server keys\n\
                --audio-setup                 - Select audio devices for voice chat\n\
                --help                        - Display this"
            );
        } else //INVALID CMD
        {
            println!("Invalid usage! Use 'why2 --help'.");
        }

        return;
    }

    println!("Welcome.\n");

    //GET CONNECTING ADDRESS
    let mut connecting_addr = if config::read_config::<bool>("auto_connect") //USER ENABLED AUTOMATIC CONNECTION
    {
        let addr = config::read_config("auto_connect_addr"); //USE CONFIG ADDR

        //PRINT OUT IP
        println!(">>> {addr}");
        io::stdout().flush().unwrap();

        addr
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
    if !connecting_addr.contains(':')
    {
        //APPEND DEFAULT PORT TO connecting_ip
        connecting_addr.push_str(&format!(":{}", config::read_config::<u16>("default_port")));
    } else
    {
        spacer_add_spaces += connecting_addr.len() - connecting_addr.find(":").unwrap();
    }

    //PRINT SPACER
    println!("{}", "=".repeat(connecting_addr.find(":").unwrap() + spacer_add_spaces));

    //SET GLOBAL SERVER ADDR
    options::set_server_address(&connecting_addr);

    //CHECK IF SOCKS5 IS ENABLED
    if config::read_config("socks5_enabled")
    {
        options::enable_socks5();
    }

    //CONNECT TO SERVER
    let mut stream = match if !options::socks5_enabled() //NO SOCKS5
    {
        TcpStream::connect(connecting_addr)
    } else //USE PROXY
    {
        Socks5Stream::connect(config::read_config::<String>("socks5_addr"), connecting_addr.as_str())
            .map(|s| s.into_inner())
    }
    {
        Ok(s) => s,
        Err(e) =>
        {
            eprintln!("\nConnecting failed: {e}");
            process::exit(1);
        }
    };

    //SET KEEP-ALIVE
    let socket = Socket::from(stream);
    socket.set_tcp_keepalive(&TcpKeepalive::new()
        .with_time(Duration::from_secs(60))
        .with_interval(Duration::from_secs(10))).expect("Failed to set KEEPALIVE");
    stream = socket.into();

    //SET TCP_NODELAY
    stream.set_nodelay(true).expect("Failed to set TCP_NODELAY");

    //CREATE STREAM LOCK
    let write_stream = Arc::new(Mutex::new(stream.try_clone().expect("Failed cloning stream")));

    //CLONE SOCKET FOR CLIENT INPUT
    let write_stream_listen = write_stream.clone();

    //LISTEN TO SERVER
    thread::spawn(move || client::listen_server(&mut (&mut stream, write_stream_listen), tx));

    //ENABLE RAW MODE
    let _raw_mode_guard = RawModeGuard::enable().unwrap();

    //LOOP FOR CLIENT-SIDE USER INPUT
    loop
    {
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
                if !command::send_command_code(&mut write_stream.lock().unwrap(), &command, &parameters)
                {
                    match command
                    {
                        //HELP
                        Command::Help =>
                        {
                            ui::clear_lines(2);
                            options::set_extra_space(true); //ADD EXTRA NEWLINE ON NEXT RECEIVED MESSAGE

                            //PRINT COMMAND LIST
                            println!("\nCommands:");

                            for info in command::COMMAND_LIST //ITERATE OVER ALL COMMANDS
                            {
                                //[OPTIONAL], (REQUIRED)
                                let args = info.args.iter().map(|arg|
                                {
                                    if arg.required
                                    {
                                        format!("({})", arg.name)
                                    } else
                                    {
                                        format!("[{}]", arg.name)
                                    }
                                }).collect::<Vec<String>>().join(" ");

                                let separator = if info.args.is_empty() { "" } else { " " };

                                //PRINT COMMAND
                                println!
                                (
                                    "\r{prefix}{name}{separator}{args} - {description}",
                                    prefix = command::COMMAND_PREFIX,
                                    name = info.triggers[0].to_lowercase(),
                                    description = info.description,
                                );
                            }

                            //PRINT PROMPT BAR
                            print!("\n\r>>> ");
                        },

                        Command::Info =>
                        {
                            ui::clear_lines(2);
                            let mut valid = false;

                            //PARAMETERS PASSED
                            if let Some(parameters) = parameters
                            {
                                //CHECK IF COMMAND/ALIAS EXISTS
                                if let Some(command) = command::COMMAND_LIST.iter()
                                    .find(|c| c.triggers.iter()
                                        .find(|t| t.eq_ignore_ascii_case(&parameters)).is_some())
                                {
                                    options::set_extra_space(true); //ADD EXTRA NEWLINE ON NEXT RECEIVED MESSAGE
                                    print!
                                    (
                                        "\n\rCommand: {command}
                                        \rAliases: {aliases}
                                        \rShortcut: {shortcut}
                                        \rParameters: {args}
                                        \rDescription: {description}
                                        \n\r>>> ",

                                        command = command.triggers[0],
                                        aliases = command.triggers[1..].join(", "),
                                        shortcut = command.shortcut.map(|s| format!("Ctrl+{}", s.to_ascii_uppercase())).unwrap_or(String::from("None")),
                                        description = command.description,
                                        args = if !command.args.is_empty()
                                        {
                                            command.args.iter().map(|arg|
                                            {
                                                if arg.required
                                                {
                                                    format!("({})", arg.name)
                                                } else
                                                {
                                                    format!("[{}]", arg.name)
                                                }
                                            }).collect::<Vec<String>>().join(" ")
                                        } else { String::from("None") },
                                    );

                                    valid = true;
                                }
                            }

                            if !valid { invalid_usage(None); }
                        },

                        Command::Upload =>
                        {
                            ui::clear_lines(2);

                            //CHECK PATH
                            if let Some(parameters) = parameters
                            {
                                let path = Path::new(parameters.trim());

                                //TRY TO OPEN FILE
                                if let Ok(metadata) = path.metadata() && path.is_file() &&
                                    let Ok(mut file) = File::open(path) &&
                                    let Some(filename) = path.file_name().and_then(|n| n.to_str())
                                {
                                    //GET SHA256 FILE HASH
                                    let mut hasher = Sha256::new();
                                    let mut buffer = vec![0; consts::UPLOAD_CHUNK_SIZE];

                                    //LOOP READING
                                    let success = loop
                                    {
                                        match file.read(&mut buffer)
                                        {
                                            Ok(0) => break true,
                                            Ok(bytes) => hasher.update(&buffer[..bytes]),
                                            Err(_) => break false,
                                        }
                                    };

                                    //REQUEST FILE UPLOAD
                                    if success
                                    {
                                        //FINALIZE HASH
                                        let hash: [u8; 32] = hasher.finalize().into();

                                        //STORE UPLOAD IN ACTIVE UPLOADS LIST
                                        client::ACTIVE_UPLOADS.lock().unwrap().insert(hash.clone(), path.canonicalize().unwrap());

                                        //SEND UPLOAD REQUEST
                                        network::send(&mut write_stream.lock().unwrap(), MessagePacket
                                        {
                                            code: command.to_code(),
                                            file: Some(FilePayload
                                            {
                                                size: Some(metadata.len()),
                                                filename: Some(filename.to_owned()),
                                                hash: Some(hash),
                                                ..Default::default()
                                            }),
                                            ..Default::default()
                                        }, options::get_keys().as_ref());
                                    } else //HASHING FAILED
                                    {
                                        print!("Error reading file!\n\n\r>>> ");
                                    }
                                } else //NON-EXISTING FILE
                                {
                                    print!("File not found!\n\n\r>>> ");
                                }
                            } else { invalid_usage(None); }
                        },

                        Command::UsernameColor =>
                        {
                            color_handler("username_color", parameters);
                        },

                        Command::MessageColor =>
                        {
                            color_handler("message_color", parameters);
                        },

                        Command::Mute =>
                        {
                            mute(parameters);
                        },

                        //INVALID COMMAND
                        Command::Invalid =>
                        {
                            ui::clear_lines(2);
                            invalid_usage(Some("command"));
                        },

                        //NON IMPLEMENTED COMMAND
                        _ => panic!("Invalid command")
                    }
                }

                io::stdout().flush().unwrap(); //FLUSH COMMAND OUTPUT
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
        network::send(&mut write_stream.lock().unwrap(), MessagePacket
        {
            text: Some(input),
            colors: get_colors(),
            ..Default::default()
        }, options::get_keys().as_ref());
    }
}
