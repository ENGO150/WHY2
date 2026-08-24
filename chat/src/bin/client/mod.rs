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
pub mod tui;

use std::
{
    process,
    fs::File,
    path::Path,
    sync::Arc,
    io::
    {
        self,
        Read,
        Write,
    },
};

use tokio::
{
    task,
    net::tcp::OwnedWriteHalf,
    sync::
    {
        Mutex as MutexAsync,
        mpsc::{ self, Sender },
    },
};

use sha2::{ Sha256, Digest };

use crossterm::style::Color;

use unicode_width::UnicodeWidthStr;

use ratatui::text::{ Line, Span };

use tui::
{
    theme,
    palette,
    App,
    TerminalGuard,
    settings::Devices,
};

#[cfg(feature = "client_voice")]
use tui::settings::DeviceEntry;

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
use why2_chat::network::voice::client as voice;

#[cfg(feature = "client_screen")]
use winit::event_loop::EventLoop;

#[cfg(feature = "client_screen")]
use why2_chat::network::screen::client::
{
    self as screen,
    UserEvent,
    display::ScreenShareApp,
};

//HANDLER FNS
fn invalid_usage(app: &mut App, subject: Option<&str>) //PUSH 'INVALID' MESSAGE
{
    app.push_styled(format!("Invalid {}! Press Ctrl+H for help.", subject.unwrap_or("usage")), theme::ERROR);
}

#[cfg(feature = "client_voice")]
fn mute(app: &mut App, parameters: Option<String>) //MUTE LOCAL/PEER CLIENT
{
    //GET ID PARAMETER
    let id = if let Some(parameters) = parameters
    {
        match parameters.parse::<usize>()
        {
            Ok(i) => Some(i),
            Err(_) => return invalid_usage(app, None)
        }
    } else { None };

    //INFO LOG
    app.push_styled(format!
    (
        "Sucessfully {}muted{}.",
        if options::toggle_mute(id) { "" } else { "un" },
        if let Some(id) = id
        {
            format!(" ID {id}")
        } else { String::new() }
    ), theme::OK);
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

fn color_handler(app: &mut App, config_key: &str, parameters: Option<String>) //HANDLE COLOR CHANGE
{
    //CHECK FOR PARAMETERS
    let Some(parameters) = parameters else { return invalid_usage(app, None) };

    //CHECK FOR COLOR VALIDITY
    if let Ok((_, formatted_name)) = to_color(&parameters)
    {
        //SAVE COLOR TO CONFIG
        config::client_write(config_key, &formatted_name);
        app.reload_theme();

        app.push_styled("Color set successfully.", theme::OK);
    } else
    {
        app.push_styled("Invalid color! See https://docs.rs/crossterm/latest/crossterm/style/enum.Color.html for help.", theme::ERROR);
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

async fn read_line() -> String //READ ONE LINE FROM STDIN (BEFORE RAW MODE IS ENABLED)
{
    task::spawn_blocking(||
    {
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        input
    }).await.expect("Reading stdin failed")
}

//EVERY DEVICE THE VOICE CLIENT COULD OPEN. THE LIST COMES FROM THE VOICE CLIENT ITSELF, SO IT IS ENUMERATED
//IN THE SAME cpal HOST THAT LATER OPENS THE CHOSEN DEVICE (BLOCKING, HENCE spawn_blocking).
async fn audio_devices() -> Devices
{
    #[cfg(not(feature = "client_voice"))]
    {
        Devices::default()
    }

    #[cfg(feature = "client_voice")]
    {
        task::spawn_blocking(||
        {
            let (input, output) = voice::list_devices();

            Devices
            {
                input: input.into_iter().map(device_entry).collect(),
                output: output.into_iter().map(device_entry).collect(),
            }
        }).await.unwrap_or_default()
    }
}

#[cfg(feature = "client_voice")]
fn device_entry(device: voice::AudioDevice) -> DeviceEntry
{
    DeviceEntry { id: device.id, label: device.label }
}

#[tokio::main]
async fn main()
{
    //RESTORE THE TERMINAL EVEN ON A PANIC - THE RELEASE PROFILE USES panic = "abort", SO Drop NEVER RUNS
    tui::install_panic_hook();

    //CREATE CHANNEL
    let (tx, mut rx) = mpsc::channel::<ClientEvent>(consts::EVENT_CHANNEL_BOUND);

    //CONFIGURATION
    misc::check_version(&tx).await; //CHECK WHY2 VERSION
    config::init_config(); //CREATE client.toml CONFIGURATION

    //DRAIN WHATEVER check_version REPORTED - STILL ON THE NORMAL SCREEN
    let mut pre_tui = App::new();
    while let Ok(event) = rx.try_recv() { pre_tui.apply(event); }
    flush_plain(&mut pre_tui);

    println!("Welcome.\n");

    //RUN REST OF CLIENT IN NEW TASK
    #[cfg(feature = "client_screen")]
    tokio::spawn(run_client(tx, rx));

    #[cfg(not(feature = "client_screen"))]
    run_client(tx, rx).await;

    #[cfg(feature = "client_screen")]
    {
        let event_loop = EventLoop::<UserEvent>::with_user_event()
            .build().expect("Failed to create event loop");

        *screen::SCREEN_SHARE_PROXY.write().unwrap() = Some(event_loop.create_proxy());

        let mut app = ScreenShareApp::new();
        event_loop.run_app(&mut app).expect("Event loop terminated with error");
    }
}

fn flush_plain(app: &mut App) //PRINT PENDING OUTPUT WITHOUT A FRAME (PRE-TUI PHASE)
{
    for line in app.drain_plain() { println!("{line}"); }

    io::stdout().flush().unwrap();
}

async fn run_client(tx: Sender<ClientEvent>, mut rx: mpsc::Receiver<ClientEvent>)
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
        read_line().await.trim().to_owned()
    };

    //REMEMBER WHAT THE USER ACTUALLY TYPED - THE TITLE ONLY SHOWS A PORT WHEN ONE WAS ASKED FOR
    let display_addr = connecting_addr.trim().to_owned();

    //ADD PORT TO IP IF MISSING
    if !connecting_addr.contains(':')
    {
        //APPEND DEFAULT PORT TO connecting_ip
        connecting_addr.push_str(&format!(":{}", config::read_config::<u16>("default_port")));
    }

    //SET GLOBAL SERVER ADDR
    options::set_server_address(&connecting_addr);

    //CHECK IF SOCKS5 IS ENABLED
    if config::read_config("socks5_enabled")
    {
        options::enable_socks5();
    }

    //CONNECT TO SERVER (STILL ON THE NORMAL SCREEN, SO A FAILURE STAYS READABLE)
    let (mut read_stream, write_half) = match client::connect(connecting_addr).await
    {
        Ok(s) => s,
        Err(e) =>
        {
            eprintln!("\nConnecting failed: {e}");
            process::exit(1);
        }
    };

    //CREATE STREAM LOCK
    let write_stream = Arc::new(MutexAsync::new(write_half));

    //CLONE SOCKET FOR CLIENT INPUT
    let write_stream_listen = write_stream.clone();

    //LISTEN TO SERVER
    let client_tx = tx.clone();
    tokio::spawn(async move
    {
        client::listen_server(&mut (&mut read_stream, write_stream_listen), client_tx).await;
    });

    //ENTER THE TUI - EVERY TERMINAL WRITE FROM HERE ON GOES THROUGH tui::run
    let mut app = App::new();
    app.address = display_addr;

    let guard = TerminalGuard::enter().expect("Entering the alternate screen failed");
    let mut terminal = tui::init().expect("Creating the terminal backend failed");

    tui::run(&mut terminal, &mut app, &mut rx, &write_stream).await;

    //LEAVE THE ALTERNATE SCREEN BEFORE SAYING ANYTHING ELSE
    drop(guard);

    if let Some(message) = app.quit_message.take() { println!("{message}"); }

    process::exit(app.exit_code);
}

//HANDLES ONE SUBMITTED LINE: SLASH COMMAND, LOGIN STEP OR CHAT MESSAGE
pub async fn submit(app: &mut App, write_stream: &Arc<MutexAsync<OwnedWriteHalf>>, input: String)
{
    let input = if options::get_asking_password() { input } else { input.trim().to_string() };

    //APPEND MESSAGE TO HISTORY
    if options::get_sending_messages()
    {
        if input.is_empty() { return; } //DO NOT FORWARD EMPTY MESSAGES

        //USER COMMANDS
        let mut command_used = false;
        if let (Some(command), parameters) = command::get_command(&input)
        {
            //SEND CODE ON A SIMPLE COMMAND, CONTINUE OTHERWISE (RELEASE THE STREAM LOCK FIRST)
            let sent = command::send_command_code(&mut *write_stream.lock().await, &command, &parameters).await;

            //A REQUEST/RESPONSE COMMAND THE USER TYPED WANTS ITS ANSWER ECHOED INTO THE PANE
            if sent == Some(true)
            {
                match command
                {
                    Command::List => app.list_requested = true,
                    #[cfg(feature = "client_screen")]
                    Command::Screens => app.screens_requested = true,
                    _ => {},
                }
            }

            match sent
            {
                //COMMAND SENT
                Some(true) => {}

                //INVALID USAGE
                Some(false) => invalid_usage(app, None),

                None =>
                {
                    match command
                    {
                        //HELP
                        Command::Help =>
                        {
                            //COLUMN WIDTHS ARE MEASURED, NOT GUESSED - LONG SIGNATURES MUST NOT PUSH THE REST OUT OF LINE
                            let signature_width = command::COMMAND_LIST.iter()
                                .map(palette::signature_width).max().unwrap_or(0);

                            //ONLY SHORTCUT-CARRYING ROWS NEED A PADDED DESCRIPTION, AND PADDING TO THE
                            //LONGEST DESCRIPTION OF ALL WOULD PUSH THEM OFF THE PANE
                            let description_width = command::COMMAND_LIST.iter()
                                .filter(|info| info.shortcut.is_some())
                                .map(|info| info.description.width()).max().unwrap_or(0);

                            let last = command::COMMAND_LIST.len() - 1;

                            app.push_styled("Commands:", theme::TITLE);

                            for (index, info) in command::COMMAND_LIST.iter().enumerate() //ITERATE OVER ALL COMMANDS
                            {
                                let shortcut = palette::format_shortcut(info);
                                let padding = signature_width - palette::signature_width(info);

                                let mut spans = vec![Span::styled(tui::branch(index == last), theme::BORDER)];

                                spans.extend(palette::signature_spans(info, None));
                                spans.push(Span::raw(" ".repeat(padding + 2)));

                                spans.push(Span::styled(format!
                                (
                                    "{description:<width$}",
                                    description = info.description,
                                    width = if shortcut.is_empty() { 0 } else { description_width },
                                ), theme::DIM));

                                if !shortcut.is_empty() { spans.push(Span::styled(format!("  [{shortcut}]"), theme::ACCENT)); }

                                app.push(Line::from(spans));
                            }
                        },

                        Command::Info =>
                        {
                            let mut valid = false;

                            //PARAMETERS PASSED
                            if let Some(parameters) = parameters
                            {
                                //CHECK IF COMMAND/ALIAS EXISTS
                                if let Some(info) = command::COMMAND_LIST.iter()
                                    .find(|c| c.triggers.iter().any(|t| t.eq_ignore_ascii_case(&parameters)))
                                {
                                    let shortcut = palette::format_shortcut(info);

                                    app.push(Line::from(palette::signature_spans(info, None)));

                                    let fields =
                                    [
                                        ("Aliases", if info.triggers.len() > 1 { info.triggers[1..].join(", ") } else { String::from("None") }),
                                        ("Shortcut", if shortcut.is_empty() { String::from("None") } else { shortcut }),
                                        ("Description", info.description.to_string()),
                                    ];

                                    let last = fields.len() - 1;

                                    for (index, (label, value)) in fields.into_iter().enumerate()
                                    {
                                        app.push(Line::from(vec!
                                        [
                                            Span::styled(tui::branch(index == last), theme::BORDER),
                                            Span::styled(format!("{label:<12}"), theme::DIM),
                                            Span::raw(value),
                                        ]));
                                    }

                                    valid = true;
                                }
                            }

                            if !valid { invalid_usage(app, None); }
                        },

                        Command::Upload =>
                        {
                            //CHECK PATH
                            if let Some(parameters) = parameters
                            {
                                let path = Path::new(parameters.trim());

                                //TRY TO OPEN FILE
                                if let Ok(file) = File::open(path) && path.metadata().is_ok() &&
                                    path.is_file() && path.file_name().and_then(|n| n.to_str()).is_some()
                                {
                                    let path = path.to_owned();
                                    let mut file = file;
                                    let write_stream = write_stream.clone();
                                    let keys = options::get_keys();

                                    tokio::spawn(async move
                                    {
                                        //GET SHA256 FILE HASH (BLOCKING I/O + CPU, KEEP IT OFF THE RUNTIME)
                                        let hash: Option<[u8; 32]> = task::spawn_blocking(move ||
                                        {
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

                                            //FINALIZE HASH
                                            if success { Some(hasher.finalize().into()) } else { None }
                                        }).await.expect("Hashing file failed");

                                        //REQUEST FILE UPLOAD
                                        if let Some(hash) = hash
                                        {
                                            //STORE UPLOAD IN ACTIVE UPLOADS LIST
                                            client::ACTIVE_UPLOADS.lock().unwrap()
                                                .insert(hash, path.canonicalize().unwrap());

                                            //SEND UPLOAD REQUEST
                                            network::send(&mut *write_stream.lock().await,
                                                PacketCode::Upload { hash, token: None, uid: None }, keys.as_ref()).await;
                                        }
                                    });
                                } else //NON-EXISTING FILE
                                {
                                    app.push_styled("File not found!", theme::ERROR);
                                }
                            } else { invalid_usage(app, None); }
                        },

                        //THE DEVICE LIST IS ENUMERATED HERE, ONCE, SO THE DRAW PATH NEVER TALKS TO cpal
                        Command::Settings => app.settings.open(audio_devices().await),

                        Command::UsernameColor => color_handler(app, "username_color", parameters),
                        Command::MessageColor => color_handler(app, "message_color", parameters),

                        #[cfg(feature = "client_voice")]
                        Command::Mute => mute(app, parameters),

                        //INVALID COMMAND
                        Command::Invalid => invalid_usage(app, Some("command")),

                        //NON IMPLEMENTED COMMAND
                        _ => panic!("Invalid command")
                    }
                }
            }

            command_used = true;
        }

        //ADD INPUT
        if !options::get_asking_password()
        {
            app.input.push_history(&input);
        }

        if command_used { return }; //DO NOT SEND COMMAND STRING
    }

    //DISABLE ASKING_PASSWORD
    options::set_asking_password(false);

    //SEND input TO SERVER
    let packet = match options::get_login_state()
    {
        LoginState::Username =>
        {
            app.username = input.clone();
            PacketCode::Username { username: Some(input) }
        },
        LoginState::PasswordLogin => PacketCode::PasswordL { password: Some(input) },
        LoginState::PasswordRegister => PacketCode::PasswordR { password: Some(input) },
        LoginState::None => PacketCode::Message { text: input, colors: get_colors(), username: None, id: None },
    };

    network::send(&mut *write_stream.lock().await, packet, options::get_keys().as_ref()).await;
}
