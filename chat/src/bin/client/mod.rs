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
    iter,
    process,
    fs::File,
    path::Path,
    sync::Arc,
    io::{ Read, Seek },
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
    role::{ self, Role },
    options::{ self, LoginState },
    command::
    {
        self,
        Command,
        Subcommand,
    },
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

//MODERATION ACTIONS - /server <action> [id]
async fn server_command(app: &mut App, write_stream: &Arc<MutexAsync<OwnedWriteHalf>>, parameters: Option<String>)
{
    let Some(info) = command::COMMAND_LIST.iter().find(|info| info.command == Command::Server) else { return };

    //HIDING THE COMMAND DOES NOT STOP ANYBODY TYPING IT OUT, AND REFUSING IT WOULD CONFIRM IT EXISTS -
    //TO A ROLE THAT MAY NOT RUN IT, IT IS SIMPLY NOT A COMMAND
    if !info.available(app.role) { return invalid_usage(app, Some("command")); }

    let Some(parameters) = parameters else { return invalid_usage(app, None) };

    //THE ACTION IS THE FIRST WORD, WHATEVER IT TAKES FOLLOWS IT
    let (action, tail) = match parameters.split_once(char::is_whitespace)
    {
        Some((action, tail)) => (action, tail.trim()),
        None => (parameters.as_str(), ""),
    };

    let Some(sub) = info.action(action) else { return invalid_usage(app, Some("action")) };

    //AN ACTION ABOVE OUR ROLE IS UNKNOWN FOR THE SAME REASON THE COMMAND IS
    if !sub.available(app.role) { return invalid_usage(app, Some("action")); }

    //AN ACTION THAT TAKES A PARAMETER NEEDS ONE, WHATEVER IT IS
    if !sub.args.is_empty() && tail.is_empty() { return invalid_usage(app, None); }

    //MOST ACTIONS ARE AIMED AT A USER AND TAKE AN ID - THE REST READ `tail` AS TEXT
    let id = match sub.takes_id()
    {
        true => match tail.parse::<usize>()
        {
            Ok(id) => Some(id),
            Err(_) => return invalid_usage(app, None),
        },

        false => None,
    };

    match sub.subcommand
    {
        Subcommand::Mute =>
        {
            network::send(&mut *write_stream.lock().await, PacketCode::ServerMute
            {
                id: id.unwrap(),
            }, options::get_keys().as_ref()).await;
        },

        Subcommand::Kick =>
        {
            network::send(&mut *write_stream.lock().await, PacketCode::ServerKick
            {
                id: id.unwrap(),
            }, options::get_keys().as_ref()).await;
        },

        Subcommand::Ban =>
        {
            network::send(&mut *write_stream.lock().await, PacketCode::ServerBan
            {
                id: id.unwrap(),
            }, options::get_keys().as_ref()).await;
        },

        Subcommand::BanIp =>
        {
            network::send(&mut *write_stream.lock().await, PacketCode::ServerBanIp
            {
                id: id.unwrap(),
            }, options::get_keys().as_ref()).await;
        },

        Subcommand::Bans =>
        {
            network::send(&mut *write_stream.lock().await, PacketCode::ServerBans
            {
                users: None,
                ips: None,
            }, options::get_keys().as_ref()).await;
        },

        Subcommand::Pardon =>
        {
            network::send(&mut *write_stream.lock().await, PacketCode::ServerPardon
            {
                id: id.unwrap(),
            }, options::get_keys().as_ref()).await;
        },

        Subcommand::PardonIp =>
        {
            network::send(&mut *write_stream.lock().await, PacketCode::ServerPardonIp
            {
                id: id.unwrap(),
            }, options::get_keys().as_ref()).await;
        },

        Subcommand::Say =>
        {
            network::send(&mut *write_stream.lock().await, PacketCode::ServerSay
            {
                message: tail.to_owned(),
            }, options::get_keys().as_ref()).await;
        },

        //THE ONE ACTION THAT AIMS AT A USER AND STILL TAKES SOMETHING ELSE - THE ROLE IS RESOLVED HERE,
        //SO A NAME NOBODY KNOWS IS INVALID USAGE ON THE SPOT RATHER THAN A PACKET THE SERVER REFUSES
        Subcommand::Role =>
        {
            let Some((target, role)) = tail.split_once(char::is_whitespace) else { return invalid_usage(app, None) };

            let Ok(target) = target.parse::<usize>() else { return invalid_usage(app, None) };
            let Ok(role) = role.trim().parse::<Role>() else { return invalid_usage(app, Some("role")) };

            network::send(&mut *write_stream.lock().await, PacketCode::ServerRole
            {
                id: target,
                role,
                username: None,
            }, options::get_keys().as_ref()).await;
        },

        Subcommand::Settings =>
        {
            network::send(&mut *write_stream.lock().await, PacketCode::ServerSettings
            {
                settings: None,
                save: false,
            }, options::get_keys().as_ref()).await;
        },
    }
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

    //A COLOR THAT PARSES IS NOT NECESSARILY ONE WE CAN SEND: THE WIRE CARRIES A CODE, SO ansi_(n) AND rgb_(r,g,b)
    //HAVE NOWHERE TO GO AND USED TO BE ACCEPTED, WRITTEN TO THE CONFIG AND THEN SILENTLY IGNORED ON EVERY MESSAGE
    let color = Color::try_from(formatted_color.as_str()).map_err(|_| ())?;
    let code = colors::color_to_u8(&color);

    if code == 255 { return Err(()); }

    Ok((code, formatted_color))
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
        app.push_styled("Invalid color! Type the command again and pick one of the offered colors.", theme::ERROR);
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
    let (tx, rx) = mpsc::channel::<ClientEvent>(consts::EVENT_CHANNEL_BOUND);

    //CONFIGURATION
    config::init_config(); //CREATE client.toml CONFIGURATION

    //CHECK WHY2 VERSION - IT REPORTS THROUGH tx LIKE ANYTHING ELSE, SO IT MUST NOT HOLD UP THE FIRST FRAME
    let version_tx = tx.clone();
    tokio::spawn(async move { misc::check_version(&version_tx).await; });

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

async fn run_client(tx: Sender<ClientEvent>, mut rx: mpsc::Receiver<ClientEvent>)
{
    //CHECK IF SOCKS5 IS ENABLED (EVERY DIAL THE CONNECT PROMPT MAKES GOES THROUGH IT)
    if config::read_config("socks5_enabled")
    {
        options::enable_socks5();
    }

    //ENTER THE TUI RIGHT AWAY - THE ADDRESS IS ASKED FOR INSIDE IT, AND SO IS EVERYTHING AFTER IT
    let mut app = App::new();

    let guard = TerminalGuard::enter().expect("Entering the alternate screen failed");
    let mut terminal = tui::init().expect("Creating the terminal backend failed");

    //ASK THE TERMINAL WHAT IT CAN DRAW AND HOW BIG ITS CELLS ARE - THE QUERY WRITES AND READS STDIO, SO
    //IT GOES HERE: THE ALTERNATE SCREEN IS UP AND NOTHING IS READING EVENTS YET
    app.init_picker();

    tui::run(&mut terminal, &mut app, &mut rx, &tx).await;

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

                    //THE DISCONNECT THAT COMES BACK IS ONE THE USER ASKED FOR, SO IT ENDS THE CLIENT
                    //INSTEAD OF DROPPING BACK INTO THE CONNECT BOX
                    Command::Exit => app.leaving = true,

                    //AND THE ONE /logout ASKED FOR IS NOT AN ERROR EITHER - IT GOES BACK TO THE CONNECT BOX
                    Command::Logout => app.logging_out = true,
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
                            //WHAT OUR ROLE MAY RUN - THE WIDTHS AND THE TRUNK ARE MEASURED OVER THIS, NOT OVER THE WHOLE LIST.
                            //A COMMAND THAT TAKES AN ACTION IS LISTED AS ITS ACTIONS: /server ALONE RUNS NOTHING,
                            //AND ITS ACTIONS ARE NOT ALL OF THE SAME RANK
                            let commands = command::COMMAND_LIST.iter()
                                .filter(|info| info.available(app.role))
                                .flat_map(|info| -> Box<dyn Iterator<Item = palette::Entry>>
                                {
                                    match info.subcommands.is_empty()
                                    {
                                        true => Box::new(iter::once(palette::Entry::command(info))),
                                        false => Box::new(info.actions(app.role).map(|sub| palette::Entry::action(info, sub))),
                                    }
                                }).collect::<Vec<palette::Entry>>();

                            //COLUMN WIDTHS ARE MEASURED, NOT GUESSED - LONG SIGNATURES MUST NOT PUSH THE REST OUT OF LINE
                            let signature_width = commands.iter().map(palette::Entry::width).max().unwrap_or(0);

                            //ONLY SHORTCUT-CARRYING ROWS NEED A PADDED DESCRIPTION, AND PADDING TO THE
                            //LONGEST DESCRIPTION OF ALL WOULD PUSH THEM OFF THE PANE
                            let description_width = commands.iter()
                                .filter(|entry| !entry.shortcut().is_empty())
                                .map(|entry| entry.description().width()).max().unwrap_or(0);

                            let last = commands.len().saturating_sub(1);

                            app.push_styled("Commands:", theme::TITLE);

                            for (index, entry) in commands.into_iter().enumerate() //ITERATE OVER ALL COMMANDS WE MAY RUN
                            {
                                let shortcut = entry.shortcut();
                                let padding = signature_width - entry.width();

                                let mut spans = vec![Span::styled(tui::branch(index == last), theme::BORDER)];

                                spans.extend(entry.spans(None));
                                spans.push(Span::raw(" ".repeat(padding + 2)));

                                spans.push(Span::styled(format!
                                (
                                    "{description:<width$}",
                                    description = entry.description(),
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
                                //AN ACTION IS ASKED ABOUT THE WAY IT IS RUN: /info server mute
                                let (word, action) = match parameters.split_once(char::is_whitespace)
                                {
                                    Some((word, action)) => (word, Some(action.trim())),
                                    None => (parameters.as_str(), None),
                                };

                                if let Some(info) = command::COMMAND_LIST.iter()
                                    .find(|c| c.available(app.role) && c.triggers.iter().any(|t| t.eq_ignore_ascii_case(word)))
                                    //AN ACTION THAT WAS NAMED HAS TO EXIST AND BE OURS TO RUN, OTHERWISE THIS IS NOT A COMMAND WE KNOW
                                    && let Some(entry) = match action
                                    {
                                        Some(action) => info.action(action).filter(|sub| sub.available(app.role))
                                            .map(|sub| palette::Entry::action(info, sub)),

                                        None => Some(palette::Entry::command(info)),
                                    }
                                {
                                    let shortcut = entry.shortcut();
                                    let triggers = entry.sub.map_or(info.triggers, |sub| sub.triggers);

                                    app.push(Line::from(entry.spans(None)));

                                    let fields =
                                    [
                                        ("Aliases", if triggers.len() > 1 { triggers[1..].join(", ") } else { String::from("None") }),
                                        ("Shortcut", if shortcut.is_empty() { String::from("None") } else { shortcut }),
                                        ("Description", entry.description().to_string()),
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

                        //ONE REQUEST, TWO CODES: A PERSISTENT IMAGE IS ASKED FOR EXACTLY THE WAY A
                        //FILESHARE IS - THE PATH IS CHECKED AND THE FILE HASHED IDENTICALLY, AND ONLY THE
                        //CODE THE SERVER IS ASKED WITH DECIDES WHICH OF THE TWO IT BECOMES
                        Command::Upload | Command::Image =>
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
                                    let image = command == Command::Image;

                                    //THE HEADER IS READ BEFORE THE SERVER IS ASKED FOR ANYTHING, SO A FILE
                                    //THAT IS NOT AN IMAGE COSTS NO CONNECTION AND NO HASH. THE SERVER STILL
                                    //CHECKS THE BYTES IT RECEIVES - NOTHING MAKES A CLIENT RUN THIS
                                    let mut header = Vec::new();

                                    if image
                                    {
                                        file.by_ref().take(consts::IMAGE_HEADER_SIZE as u64)
                                            .read_to_end(&mut header).ok();

                                        //THE HASH IS TAKEN FROM THE SAME HANDLE, SO GIVE BACK WHAT WAS READ
                                        file.rewind().ok();
                                    }

                                    //THE SERVER TURNS AN OVERSIZED IMAGE DOWN AS INVALID USAGE, WHICH SAYS
                                    //NOTHING ABOUT WHY - THE SIZE IS KNOWN HERE, SO IT IS SAID HERE
                                    if image && path.metadata().map(|m| m.len()).unwrap_or(0) >
                                        consts::MAX_IMAGE_SIZE as u64
                                    {
                                        app.push_styled(format!("Image is too large! (limit is {}MB)",
                                            consts::MAX_IMAGE_SIZE / consts::MEGABYTE), theme::ERROR);
                                    } else if image && !misc::is_image(&header)
                                    {
                                        app.push_styled("Not an image!", theme::ERROR);
                                    } else
                                    {
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
                                                let request = match image
                                                {
                                                    true => PacketCode::Image
                                                    {
                                                        hash,
                                                        filename: path.file_name().and_then(|n| n.to_str())
                                                            .unwrap_or("unnamed_file").to_string(),
                                                        token: None,
                                                        uid: None,
                                                    },

                                                    false => PacketCode::Upload { hash, token: None, uid: None },
                                                };

                                                network::send(&mut *write_stream.lock().await, request, keys.as_ref()).await;
                                            }
                                        });
                                    }
                                } else //NON-EXISTING FILE
                                {
                                    app.push_styled("File not found!", theme::ERROR);
                                }
                            } else { invalid_usage(app, None); }
                        },

                        //THE DEVICE LIST IS ENUMERATED HERE, ONCE, SO THE DRAW PATH NEVER TALKS TO cpal
                        Command::Settings => app.settings.open(audio_devices().await),

                        Command::Server => server_command(app, write_stream, parameters).await,

                        Command::UsernameColor => color_handler(app, "username_color", parameters),
                        Command::MessageColor => color_handler(app, "message_color", parameters),

                        #[cfg(feature = "client_voice")]
                        Command::Mute => mute(app, parameters),

                        //NOTHING WENT TO THE SERVER BECAUSE NOTHING HAD TO: THE SHARE IS ALREADY UP AND
                        //ONLY THE MONITOR UNDER IT CHANGED, WHICH THE RUNNING CAPTURE PICKS UP ON ITS OWN
                        #[cfg(feature = "client_screen")]
                        Command::Screen => app.push_styled(match screen::capture::current_monitor()
                        {
                            Some(monitor) => format!("Sharing {monitor} now."),
                            None => String::from("Swapped the shared monitor."),
                        }, theme::OK),

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
