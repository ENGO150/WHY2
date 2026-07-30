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

#![cfg(feature = "client_base")]

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
    io::
    {
        self,
        Read,
        Write,
    },
    sync::
    {
        Arc,
        Mutex,
        mpsc::{ self, Sender },
    },
};

use sha2::{ Sha256, Digest };

use crossterm::
{
    terminal,
    style::Color,
    event::
    {
        self,
        KeyCode,
        KeyModifiers,
        Event,
    },
};

use why2_chat::
{
    config,
    consts,
    misc,
    command::{ self, Command },
    options::{ self, LoginState },
    network::
    {
        self,
        client::{ self, ClientEvent },
        codes::{ PacketCode, MessageColors },
    },
};

#[cfg(feature = "client_voice")]
use cpal::
{
    Device,
    traits::{ DeviceTrait, HostTrait },
};

#[cfg(feature = "client_screen")]
use winit::event_loop::EventLoop;

#[cfg(feature = "client_screen")]
use why2_chat::network::screen::client::
{
    self as screen,
    UserEvent,
    display::ScreenShareApp,
};

//STRUCTS
struct RawModeGuard;

//GLOBAL VARIABLES
pub static INPUT_READ: Mutex<Vec<char>> = Mutex::new(Vec::new()); //PARTIAL INPUT READ FROM CLIENT
static INPUT_HISTORY: Mutex<(Vec<String>, usize)> = Mutex::new((Vec::new(), 0)); //INPUTS READ FROM CLIENT

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
    println!("Invalid {}! Press Ctrl+H for help.\n", subject.unwrap_or("usage"))
}

#[cfg(feature = "client_voice")]
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
    println!
    (
        "Sucessfully {}muted{}.\n",
        if options::toggle_mute(id) { "" } else { "un" },
        if let Some(id) = id
        {
            format!(" ID {id}")
        } else { String::new() }
    );
}

fn read_input(input: &mut Vec<char>, cursor_position: &mut usize) -> String
{
    //CREATE/RESET PARTIAL INPUT VARIABLES
    *INPUT_READ.lock().unwrap() = input.clone();

    loop
    {
        if let Event::Key(key_event) = event::read().unwrap()
        {
            //INGORE KEY RELEASE
            if key_event.is_release() { continue; }

            //CTRL SHORTCUTS
            if key_event.modifiers.contains(KeyModifiers::CONTROL) &&
                !key_event.modifiers.contains(KeyModifiers::ALT) //EXCLUDE ALTGR
            {
                match key_event.code
                {
                    //CTRL+A (MOVE TO LINE START)
                    KeyCode::Char('a') =>
                    {
                        if *cursor_position > 0
                        {
                            print!("\x1B[{}D", cursor_position);
                            *cursor_position = 0;
                        }
                    },

                    //CTRL+E (MOVE TO LINE END)
                    KeyCode::Char('e') =>
                    {
                        let input_len = input.len();

                        if *cursor_position < input_len
                        {
                            print!("\x1B[{}C", input_len - *cursor_position);
                            *cursor_position = input_len;
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
                        input.insert(*cursor_position, c); //LOCAL VARIABLE
                        INPUT_READ.lock().unwrap().insert(*cursor_position, c); //GLOBAL VARIABLE
                        *cursor_position += 1; //CURSOR

                        print!("\x1B[0K");

                        //PRINT ENTERED CHAR
                        if !options::get_asking_password() //DO NOT PRINT PASSWORD AS TEXT
                        {
                            print!("{}", input[(*cursor_position - 1)..].iter().collect::<String>());
                        } else //PRINT PASSWORD AS ASTERISKS
                        {
                            print!("{}", "*".repeat((input.len() - *cursor_position) + 1));
                        }

                        //MOVE CURSOR BACK WHERE IS SHOULD BE
                        let tail_len = input.len() - *cursor_position;
                        if tail_len > 0
                        {
                            print!("\x1B[{}D", tail_len);
                        }
                    },

                    //CONTROL CHARACTERS
                    KeyCode::Backspace => //BACKSPACE - REMOVE LAST CHAR
                    {
                        if *cursor_position > 0
                        {
                            *cursor_position -= 1; //CURSOR
                            input.remove(*cursor_position); //LOCAL VARIABLE
                            INPUT_READ.lock().unwrap().remove(*cursor_position); //GLOBAL VARIABLE

                            //MOVE CURSOR TO LEFT AND DELETE REST OF THE LINE
                            print!("\x1B[1D\x1B[0K");

                            //PRINT REMAINING CHARS
                            redraw_removed(&input, *cursor_position);
                        }
                    },

                    KeyCode::Delete => //DELETE - REMOVE NEXT CHAR
                    {
                        if *cursor_position < input.len()
                        {
                            input.remove(*cursor_position); //LOCAL VARIABLE
                            INPUT_READ.lock().unwrap().remove(*cursor_position); //GLOBAL VARIABLE

                            //MOVE CURSOR TO LEFT AND DELETE REST OF THE LINE
                            print!("\x1B[0K");

                            //PRINT REMAINING CHARS
                            redraw_removed(&input, *cursor_position);
                        }
                    },

                    KeyCode::Left => //ARROW LEFT - MOVE CURSOR
                    {
                        if *cursor_position > 0
                        {
                            *cursor_position -= 1;
                            print!("\x1B[1D");
                        }
                    },

                    KeyCode::Right => //ARROW RIGHT - MOVE CURSOR
                    {
                        if *cursor_position < input.len()
                        {
                            *cursor_position += 1;
                            print!("\x1B[1C");
                        }
                    },

                    KeyCode::Up => //ARROW UP - PAGE HISTORY
                    {
                        let mut history = INPUT_HISTORY.lock().unwrap();

                        //SKIP IF ON TOP OF HISTORY
                        if history.0.is_empty() || history.1 == 0 { continue; }

                        //CLEAR CURRENT INPUT
                        if *cursor_position > 0
                        {
                            print!("\x1B[{}D\x1B[0K", cursor_position);
                        }

                        //MOVE IN HISTORY
                        history.1 -= 1;

                        let new_input = &history.0[history.1]; //SELECTED INPUT IN HISTORY

                        //REPLACE CURRENT INPUT
                        *input = new_input.chars().collect(); //LOCAL VARIABLE
                        *INPUT_READ.lock().unwrap() = input.clone(); //GLOBAL VARIABLE
                        *cursor_position = input.len(); //CURSOR

                        print!("{}", new_input); //PRINT
                    },

                    KeyCode::Down => //ARROW DOWN - PAGE HISTORY
                    { //TODO: Remove duplicity
                        let mut history = INPUT_HISTORY.lock().unwrap();

                        //SKIP IF ON TOP OF HISTORY
                        if history.1 == history.0.len() { continue; }

                        //CLEAR CURRENT INPUT
                        if *cursor_position > 0
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
                        *input = new_input.chars().collect(); //LOCAL VARIABLE
                        *INPUT_READ.lock().unwrap() = input.clone(); //GLOBAL VARIABLE
                        *cursor_position = input.len(); //CURSOR

                        print!("{}", new_input); //PRINT
                    },

                    KeyCode::Enter => break, //ENTER PRESSED, FINALIZE
                    _ => {} //idk
                }
            }

            io::stdout().flush().unwrap();
        }
    }

    //COLLECT
    let read = input.iter().collect::<String>();

    //CLEAR INPUT BUFFERS
    input.clear();
    *cursor_position = 0;
    *INPUT_READ.lock().unwrap() = Vec::new();

    //TRIM
    if !options::get_asking_password()
    {
        read.trim().to_string()
    } else
    {
        read
    }
}

fn to_color(color: &str) -> Result<(u8, String), ()> //CONVERT STRING TO COLOR CODE
{
    //FORMAT COLOR STRING
    let mut formatted_color = color.replace(" ", "_").to_lowercase();
    if formatted_color.starts_with("dark") && !formatted_color.starts_with("dark_")
    {
        formatted_color = formatted_color.replacen("dark", "dark_", 1);
    }

    Color::try_from(formatted_color.as_str())
        .map(|c| (colors::color_to_u8(&c), formatted_color))
        .map_err(|_| ())
}

fn color_handler(config_key: &str, parameters: Option<String>) //HANDLE COLOR CHANGE
{
    ui::clear_lines(2);

    //CHECK FOR PARAMETERS
    if let Some(parameters) = parameters
    {
        //CHECK FOR COLOR VALIDITY
        println!("{}\n", if let Ok((_, formatted_name)) = to_color(&parameters)
        {
            //SAVE COLOR TO CONFIG
            config::client_write(config_key, &formatted_name);

            "Color set successfully."
        } else
        {
            "Invalid color! See \x1b]8;;https://docs.rs/crossterm/latest/crossterm/style/enum.Color.html\x1b\\crossterm API\x1b]8;;\x1b\\ for help."
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
        username_color: to_color(&config::read_config::<String>("username_color")).ok().map(|(c, _)| c),
        message_color: to_color(&config::read_config::<String>("message_color")).ok().map(|(c, _)| c),
    }
}

#[cfg(feature = "client_voice")]
fn prompt_selection(devices: &[String], config: &str) -> usize
{
    //PROMPT
    let mut input: String;
    loop
    {
        print!("\nSelect device: ");
        io::stdout().flush().unwrap();
        input = String::new(); //CLEAR BUFFER
        io::stdin().read_line(&mut input).unwrap(); //READ INPUT

        if let Ok(idx) = input.trim().parse::<usize>() && idx != 0
        {
            if let Some(device) = devices.get(idx - 1)
            {
                config::client_write(config, device);
                println!("Set to: {}", device);
                return idx - 1;
            }
        }

        println!("Invalid selection!");
    }
}

#[cfg(feature = "client_voice")]
fn load_devices<T>(all_devices: T) -> Vec<String>
where
    T: Iterator<Item = Device>
{
    //COLLECT
    let mut devices = Vec::new();
    for device in all_devices
    {
        //DO NOT PUSH DUPLICATES
        if let Ok(name) = device.description().map(|d| d.to_string()) && !devices.contains(&name)
        {
            devices.push(name);
        }
    }

    //SORT & RETURN
    devices.sort();
    devices
}

#[cfg(feature = "client_voice")]
fn list_devices(devices: &[String])
{
    for (i, device) in devices.iter().enumerate()
    {
        println!("[{}]: {device}", i + 1);
    }
}

#[cfg(feature = "client_voice")]
fn setup_devices() //SELECT AUDIO DEVICES AND STORE IN CLIENT CONFIG
{
    let host = cpal::default_host();

    //COLLECT INPUT DEVICES
    println!("Available input devices\n=======================");
    let input_devices = load_devices(host.input_devices().unwrap());

    //GET INPUT DEVICE
    list_devices(&input_devices); //LIST
    prompt_selection(&input_devices, "input_device"); //PROMPT

    //COLLECT OUTPUT DEVICES
    println!("\nAvailable output devices\n========================");
    let output_devices = load_devices(host.output_devices().unwrap());

    //GET OUTPUT DEVICE
    list_devices(&output_devices); //LIST
    prompt_selection(&output_devices, "output_device"); //PROMPT
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
            #[cfg(feature = "client_voice")]
            setup_devices();
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


    //RUN REST OF CLIENT IN NEW THREAD
    #[cfg(feature = "client_screen")]
    thread::spawn(move || run_client(tx));

    #[cfg(not(feature = "client_screen"))]
    run_client(tx);

    #[cfg(feature = "client_screen")]
    {
        let event_loop = EventLoop::<UserEvent>::with_user_event()
            .build().expect("Failed to create event loop");

        *screen::SCREEN_SHARE_PROXY.write().unwrap() = Some(event_loop.create_proxy());

        let mut app = ScreenShareApp::new();
        event_loop.run_app(&mut app).expect("Event loop terminated with error");
    }
}

fn run_client(tx: Sender<ClientEvent>)
{
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
    let mut stream = match client::connect(connecting_addr)
    {
        Ok(s) => s,
        Err(e) =>
        {
            eprintln!("\nConnecting failed: {e}");
            process::exit(1);
        }
    };

    //CREATE STREAM LOCK
    let write_stream = Arc::new(Mutex::new(stream.try_clone().expect("Failed cloning stream")));

    //CLONE SOCKET FOR CLIENT INPUT
    let write_stream_listen = write_stream.clone();

    //LISTEN TO SERVER
    let client_tx = tx.clone();
    thread::spawn(move || client::listen_server(&mut (&mut stream, write_stream_listen), client_tx));

    //ENABLE RAW MODE
    let _raw_mode_guard = RawModeGuard::enable().unwrap();

    //INPUT STATE
    let mut current_input: Vec<char> = Vec::new();
    let mut cursor_position = 0usize;

    //LOOP FOR CLIENT-SIDE USER INPUT
    loop
    {
        //READ STDIN
        let input = read_input(&mut current_input, &mut cursor_position);

        //APPEND MESSAGE TO HISTORY
        if options::get_sending_messages()
        {
            if input.is_empty() { continue; } //DO NOT FORWARD EMPTY MESSAGES

            //USER COMMANDS
            let mut command_used = false;
            if let (Some(command), parameters) = command::get_command(&input)
            {
                //SEND CODE ON A SIMPLE COMMAND, CONTINUE OTHERWISE
                match command::send_command_code(&mut write_stream.lock().unwrap(), &command, &parameters)
                {
                    //COMMAND SENT
                    Some(true) => {}

                    //INVALID USAGE
                    Some(false) =>
                    {
                        ui::clear_lines(2);
                        invalid_usage(None);
                    }

                    None =>
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

                                println!();
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
                                        println!
                                        (
                                            "\n\rCommand: {command}
                                            \rAliases: {aliases}
                                            \rShortcut: {shortcut}
                                            \rParameters: {args}
                                            \rDescription: {description}\n",

                                            command = command.triggers[0],
                                            aliases = command.triggers[1..].join(", "),
                                            shortcut = command.shortcut.map(|s| format!("Ctrl+{}", s.to_ascii_uppercase()))
                                                .unwrap_or_else(|| String::from("None")),
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
                                    if let Ok(mut file) = File::open(path) && path.metadata().is_ok() &&
                                        path.is_file() && path.file_name().and_then(|n| n.to_str()).is_some()
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
                                            network::send(&mut write_stream.lock().unwrap(),
                                                PacketCode::Upload { hash, token: None, uid: None }, options::get_keys().as_ref());
                                        } else //HASHING FAILED
                                        {
                                            println!("Error reading file!\n");
                                        }
                                    } else //NON-EXISTING FILE
                                    {
                                        println!("File not found!\n");
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

                            #[cfg(feature = "client_voice")]
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
                }

                tx.send(ClientEvent::Prompt).unwrap();

                command_used = true;
            }

            let mut history = INPUT_HISTORY.lock().unwrap();

            //ADD INPUT
            if !options::get_asking_password() && history.0.last() != Some(&input)
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
        let packet = match options::get_login_state()
        {
            LoginState::Username => PacketCode::Username { username: Some(input) },
            LoginState::PasswordLogin => PacketCode::PasswordL { password: Some(input) },
            LoginState::PasswordRegister => PacketCode::PasswordR { password: Some(input) },
            LoginState::None => PacketCode::Message { text: input, colors: get_colors(), username: None, id: None },
        };

        network::send(&mut write_stream.lock().unwrap(), packet, options::get_keys().as_ref());
    }
}
