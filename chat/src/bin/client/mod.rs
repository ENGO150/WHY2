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
    path::PathBuf,
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

use unicode_width::UnicodeWidthStr;

use ratatui::text::{ Line, Span };

use tui::
{
    theme,
    palette,
    account::{ Account, Kind },
    App,
    TerminalGuard,
    settings::Devices,
};

#[cfg(feature = "client_voice")]
use tui::settings::DeviceEntry;

use why2_chat::
{
    t,
    tn,
    i18n,
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
        client::{ self, ClientEvent, image as client_image },
        codes::
        {
            PacketCode,
            Device,
        },
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
fn invalid_usage(app: &mut App, key: Option<&'static str>) //PUSH 'INVALID' MESSAGE
{
    app.push_styled(i18n::text(key.unwrap_or("invalid.usage")), theme::error());
}

//MODERATION ACTIONS - /server <action> [target]
async fn server_command(app: &mut App, write_stream: &Arc<MutexAsync<OwnedWriteHalf>>, tx: &Sender<ClientEvent>,
    parameters: Option<String>)
{
    let Some(info) = command::COMMAND_LIST.iter().find(|info| info.command == Command::Server) else { return };

    //A COMMAND OUR ROLE MAY NOT RUN IS NO COMMAND
    if !info.available(app.role) { return invalid_usage(app, Some("invalid.command")); }

    let Some(parameters) = parameters else { return invalid_usage(app, None) };

    //THE ACTION IS THE FIRST WORD
    let (action, tail) = match parameters.split_once(char::is_whitespace)
    {
        Some((action, tail)) => (action, tail.trim()),
        None => (parameters.as_str(), ""),
    };

    let Some(sub) = info.action(action) else { return invalid_usage(app, Some("invalid.action")) };

    //AN ACTION ABOVE OUR ROLE IS UNKNOWN
    if !sub.available(app.role) { return invalid_usage(app, Some("invalid.action")); }

    //AN ACTION THAT TAKES A PARAMETER NEEDS ONE
    if sub.args.iter().any(|arg| arg.required) && tail.is_empty() { return invalid_usage(app, None); }

    //SOME ACTIONS TAKE AN ID, THE REST TAKE TEXT
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
                target: tail.to_owned(),
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
            network::send(&mut *write_stream.lock().await, PacketCode::ServerBansRequest,
                options::get_keys().as_ref()).await;
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

        //THE ONE ACTION THAT TAKES A USER AND MORE
        Subcommand::Role =>
        {
            let Some((target, role)) = tail.split_once(char::is_whitespace) else { return invalid_usage(app, None) };

            let Ok(role) = role.trim().parse::<Role>() else { return invalid_usage(app, Some("invalid.role")) };

            network::send(&mut *write_stream.lock().await, PacketCode::ServerRoleRequest
            {
                target: target.to_owned(),
                role,
            }, options::get_keys().as_ref()).await;
        },

        Subcommand::Settings =>
        {
            network::send(&mut *write_stream.lock().await, PacketCode::ServerSettingsRequest,
                options::get_keys().as_ref()).await;
        },

        //UPLOADED LIKE AN AVATAR, OR DROPPED WITHOUT A PATH
        Subcommand::Icon => match tail.is_empty()
        {
            true => network::send(&mut *write_stream.lock().await, PacketCode::ServerIconSave { hash: None },
                options::get_keys().as_ref()).await,

            false => if let Err(error) = upload(write_stream, tail, Upload::Icon, Some(tx.clone()))
            {
                app.push_styled(error, theme::error());
            },
        },

        //ACCOUNT ACTIONS
        Subcommand::Delete | Subcommand::Passwd => invalid_usage(app, Some("invalid.action")),
    }
}

//ACCOUNT ACTIONS - /account <action>
fn account_command(app: &mut App, parameters: Option<String>)
{
    let Some(info) = command::COMMAND_LIST.iter().find(|info| info.command == Command::Account) else { return };

    let Some(sub) = parameters.as_deref().and_then(|p| info.action(p)) else { return invalid_usage(app, Some("invalid.action")) };

    app.account = Some(Account::new(match sub.subcommand
    {
        Subcommand::Delete => Kind::Delete,
        _ => Kind::Passwd,
    }));
}

fn hearts(app: &mut App, parameters: Option<String>) //LIST WHO HEARTED A MESSAGE
{
    let Some(message_id) = parameters.and_then(|p| p.parse::<u64>().ok()) else { return invalid_usage(app, None) };

    let Some(hearts) = app.hearts_of(message_id) else
    {
        app.push_styled(t!("hearts.not_loaded", message_id), theme::error());
        return;
    };

    if hearts.is_empty()
    {
        app.push_styled(t!("hearts.none", message_id), theme::notice());
        return;
    }

    app.push_styled(t!("hearts.title", message_id, count = hearts.len()), theme::title());

    let last = hearts.len() - 1;

    for (index, username) in hearts.into_iter().enumerate()
    {
        app.push(Line::from(vec![Span::styled(tui::branch(index == last), theme::border()), Span::raw(username)]));
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
    let message = match (options::toggle_mute(id), id)
    {
        (true, Some(id)) => t!("mute.muted_id", id),
        (false, Some(id)) => t!("mute.unmuted_id", id),
        (true, None) => t!("mute.muted").to_owned(),
        (false, None) => t!("mute.unmuted").to_owned(),
    };

    app.push_styled(message, theme::ok());
}

//A TYPED NAME TO THE CODE THE WIRE CARRIES
fn to_color(color: &str) -> Option<u8>
{
    //FORMAT COLOR STRING
    let mut formatted_color = color.replace(" ", "_").to_lowercase();
    if formatted_color.starts_with("dark") && !formatted_color.starts_with("dark_")
    {
        formatted_color = formatted_color.replacen("dark", "dark_", 1);
    }

    colors::code(&formatted_color)
}

//WHAT WE TELL THE SERVER WE ARE RUNNING
fn share_device() -> Option<Device>
{
    if config::read_config::<bool>("share_device") { Some(Device::TUI) } else { None }
}

//HANDLE COLOR CHANGE: CHECK IT AND ASK THE SERVER
async fn color_handler
(
    app: &mut App,
    write_stream: &Arc<MutexAsync<OwnedWriteHalf>>,
    username: bool,
    parameters: Option<String>,
)
{
    //CHECK FOR PARAMETERS
    let Some(parameters) = parameters else { return invalid_usage(app, None) };

    //CHECK FOR COLOR VALIDITY
    let Some(code) = to_color(&parameters) else
    {
        return app.push_styled(t!("invalid.color"), theme::error());
    };

    network::send(&mut *write_stream.lock().await, PacketCode::ColorRequest { username, color: code },
        options::get_keys().as_ref()).await;
}

//EVERY DEVICE THE VOICE CLIENT COULD OPEN (BLOCKING)
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
    //RESTORE THE TERMINAL EVEN ON A PANIC
    tui::install_panic_hook();

    //CREATE CHANNEL
    let (tx, rx) = mpsc::channel::<ClientEvent>(consts::EVENT_CHANNEL_BOUND);

    //CONFIGURATION
    config::init_config(); //CREATE client.toml CONFIGURATION

    //CHECK WHY2 VERSION - REPORTED THROUGH tx
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
    //CHECK IF SOCKS5 IS ENABLED
    if config::read_config("socks5_enabled")
    {
        options::enable_socks5();
    }

    //ENTER THE TUI RIGHT AWAY
    let mut app = App::new();

    let guard = TerminalGuard::enter().expect("Entering the alternate screen failed");
    let mut terminal = tui::init().expect("Creating the terminal backend failed");

    //KEY RELEASES FOR PUSH-TO-TALK
    app.key_release = guard.releases();

    //ASK THE TERMINAL WHAT IT CAN DRAW
    app.init_picker();

    tui::run(&mut terminal, &mut app, &mut rx, &tx).await;

    //LEAVE THE ALTERNATE SCREEN FIRST
    drop(guard);

    if let Some(message) = app.quit_message.take() { println!("{message}"); }

    process::exit(app.exit_code);
}

//WHAT AN UPLOAD IS FOR
#[derive(Clone, Copy, PartialEq)]
pub enum Upload
{
    File,
    Image,
    Avatar,
    Icon,
}

//CHECK A FILE, THEN HASH IT AND ASK THE SERVER FOR AN UPLOAD
pub fn upload(write_stream: &Arc<MutexAsync<OwnedWriteHalf>>, path: &str, kind: Upload, tx: Option<Sender<ClientEvent>>)
    -> Result<(), String>
{
    let (file, path) = check_upload(path, kind)?;

    let write_stream = write_stream.clone();
    let keys = options::get_keys();

    tokio::spawn(async move
    {
        //HASH IT, OR CUT IT FIRST (BLOCKING I/O + CPU)
        let prepared = task::spawn_blocking(move || match kind
        {
            Upload::Avatar | Upload::Icon => cut_avatar(file),
            _ => hash_file(file).map(|hash| (hash, path)).ok_or_else(|| t!("upload.read_failed").to_owned()),
        }).await.expect("Hashing file failed");

        let (hash, path) = match prepared
        {
            Ok(prepared) => prepared,
            Err(error) =>
            {
                if let Some(tx) = tx { tx.send(ClientEvent::AvatarFailed(error)).await.ok(); }
                return;
            },
        };

        //STORE UPLOAD IN ACTIVE UPLOADS LIST
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("unnamed_file").to_string();

        client::ACTIVE_UPLOADS.lock().unwrap().insert(hash, path.canonicalize().unwrap_or(path));

        //SEND UPLOAD REQUEST
        let request = match kind
        {
            Upload::Avatar => PacketCode::AvatarRequest { hash: Some(hash) },
            Upload::Icon => PacketCode::ServerIconSave { hash: Some(hash) },
            Upload::Image => PacketCode::ImageRequest { hash, filename },
            Upload::File => PacketCode::UploadRequest { hash },
        };

        network::send(&mut *write_stream.lock().await, request, keys.as_ref()).await;
    });

    Ok(())
}

//UPLOAD A RECORDED CLIP
#[cfg(feature = "client_voice")]
pub fn send_voice(write_stream: &Arc<MutexAsync<OwnedWriteHalf>>, clip: Vec<u8>, tx: Sender<ClientEvent>)
{
    let write_stream = write_stream.clone();
    let keys = options::get_keys();

    tokio::spawn(async move
    {
        let hash: [u8; 32] = Sha256::digest(&clip).into();
        let path = misc::voice_temp(&hash);

        if tokio::fs::write(&path, &clip).await.is_err()
        {
            tx.send(ClientEvent::VoiceMessageFailed(t!("upload.read_failed").to_owned())).await.ok();
            return;
        }

        client::ACTIVE_UPLOADS.lock().unwrap().insert(hash, path);

        network::send(&mut *write_stream.lock().await, PacketCode::VoiceMessageRequest { hash }, keys.as_ref()).await;
    });
}

//SHA256 OF A WHOLE FILE
fn hash_file(mut file: File) -> Option<[u8; 32]>
{
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; consts::UPLOAD_CHUNK_SIZE];

    //LOOP READING
    loop
    {
        match file.read(&mut buffer)
        {
            Ok(0) => break Some(hasher.finalize().into()),
            Ok(bytes) => hasher.update(&buffer[..bytes]),
            Err(_) => break None,
        }
    }
}

//CUT AN AVATAR TO ITS SQUARE AND PARK IT FOR THE UPLOAD
fn cut_avatar(mut file: File) -> Result<([u8; 32], PathBuf), String>
{
    let mut data = Vec::new();
    file.read_to_end(&mut data).map_err(|_| t!("upload.read_failed").to_owned())?;

    let (avatar, extension) = client_image::make_avatar(&data).ok_or_else(|| t!("upload.unreadable_image").to_owned())?;

    if avatar.len() > consts::MAX_AVATAR_SIZE
    {
        return Err(t!("upload.avatar_too_large", limit = consts::MAX_AVATAR_SIZE / consts::MEGABYTE));
    }

    let hash: [u8; 32] = Sha256::digest(&avatar).into();
    let path = misc::avatar_temp(&hash, extension);

    std::fs::write(&path, &avatar).map_err(|_| t!("upload.avatar_write_failed").to_owned())?;

    Ok((hash, path))
}

//OPEN A FILE AND REFUSE WHAT THE SERVER WOULD
pub fn check_upload(path: &str, kind: Upload) -> Result<(File, PathBuf), String>
{
    //THE PALETTE OFFERS ~ PATHS, SO ONE HAS TO OPEN
    let path = palette::expand_home(path.trim());

    //TRY TO OPEN FILE
    let Ok(mut file) = File::open(&path) else { return Err(t!("upload.not_found").to_owned()) };

    if !path.is_file() || path.file_name().and_then(|n| n.to_str()).is_none()
    {
        return Err(t!("upload.not_found").to_owned());
    }

    if kind == Upload::File { return Ok((file, path)); }

    //READ THE HEADER BEFORE ASKING THE SERVER
    let mut header = Vec::new();

    file.by_ref().take(consts::IMAGE_HEADER_SIZE as u64).read_to_end(&mut header).ok();

    //GIVE BACK WHAT WAS READ - THE HASH REUSES IT
    file.rewind().ok();

    //REFUSE AN OVERSIZED IMAGE HERE
    if path.metadata().map(|m| m.len()).unwrap_or(0) > consts::MAX_IMAGE_SIZE as u64
    {
        return Err(t!("upload.image_too_large", limit = consts::MAX_IMAGE_SIZE / consts::MEGABYTE));
    }

    if !misc::is_image(&header) { return Err(t!("upload.not_image").to_owned()); }

    Ok((file, path))
}

//HANDLE ONE SUBMITTED LINE
pub async fn submit(app: &mut App, write_stream: &Arc<MutexAsync<OwnedWriteHalf>>, tx: &Sender<ClientEvent>, input: String)
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
            //SEND A SIMPLE COMMAND'S CODE, ELSE CONTINUE
            let sent = command::send_command_code(&mut *write_stream.lock().await, &command, &parameters).await;

            //ECHO A REQUEST/RESPONSE ANSWER INTO THE PANE
            if sent == Some(true)
            {
                match command
                {
                    Command::List => app.list_requested = true,
                    #[cfg(feature = "client_screen")]
                    Command::Screens => app.screens_requested = true,

                    //A DISCONNECT THE USER ASKED FOR ENDS THE CLIENT
                    Command::Exit => app.leaving = true,

                    //A /logout IS NOT AN ERROR EITHER
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
                            //THE COMMANDS OUR ROLE MAY RUN
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

                            //MEASURE THE COLUMN WIDTHS
                            let signature_width = commands.iter().map(palette::Entry::width).max().unwrap_or(0);

                            //ONLY SHORTCUT-CARRYING ROWS ARE PADDED
                            let description_width = commands.iter()
                                .filter(|entry| !entry.shortcut().is_empty())
                                .map(|entry| entry.description().width()).max().unwrap_or(0);

                            let last = commands.len().saturating_sub(1);

                            app.push_styled(t!("help.title"), theme::title());

                            for (index, entry) in commands.into_iter().enumerate() //ITERATE OVER ALL COMMANDS WE MAY RUN
                            {
                                let shortcut = entry.shortcut();
                                let padding = signature_width - entry.width();

                                let mut spans = vec![Span::styled(tui::branch(index == last), theme::border())];

                                spans.extend(entry.spans(None));
                                spans.push(Span::raw(" ".repeat(padding + 2)));

                                spans.push(Span::styled(format!
                                (
                                    "{description:<width$}",
                                    description = entry.description(),
                                    width = if shortcut.is_empty() { 0 } else { description_width },
                                ), theme::dim()));

                                if !shortcut.is_empty() { spans.push(Span::styled(format!("  [{shortcut}]"), theme::accent())); }

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
                                let (word, action) = match parameters.split_once(char::is_whitespace)
                                {
                                    Some((word, action)) => (word, Some(action.trim())),
                                    None => (parameters.as_str(), None),
                                };

                                if let Some(info) = command::COMMAND_LIST.iter()
                                    .find(|c| c.available(app.role) && c.triggers.iter().any(|t| t.eq_ignore_ascii_case(word)))
                                    //A NAMED ACTION HAS TO EXIST AND BE OURS TO RUN
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
                                        (t!("info.aliases"), if triggers.len() > 1 { triggers[1..].join(", ") } else { t!("info.none").to_owned() }),
                                        (t!("info.shortcut"), if shortcut.is_empty() { t!("info.none").to_owned() } else { shortcut }),
                                        (t!("info.description"), entry.description().to_string()),
                                    ];

                                    let label_width = fields.iter().map(|(label, _)| label.width()).max().unwrap_or(0) + 2;
                                    let last = fields.len() - 1;

                                    for (index, (label, value)) in fields.into_iter().enumerate()
                                    {
                                        app.push(Line::from(vec!
                                        [
                                            Span::styled(tui::branch(index == last), theme::border()),
                                            Span::styled(format!("{label:<label_width$}"), theme::dim()),
                                            Span::raw(value),
                                        ]));
                                    }

                                    valid = true;
                                }
                            }

                            if !valid { invalid_usage(app, None); }
                        },

                        Command::Upload | Command::Image => match parameters
                        {
                            Some(path) =>
                            {
                                let kind = if command == Command::Image { Upload::Image } else { Upload::File };

                                if let Err(error) = upload(write_stream, &path, kind, None) { app.push_styled(error, theme::error()); }
                            },

                            None => invalid_usage(app, None),
                        },

                        //ENUMERATE THE DEVICES ONCE, OFF THE DRAW PATH
                        Command::Settings => app.settings.open(audio_devices().await),

                        Command::Server => server_command(app, write_stream, tx, parameters).await,

                        Command::Account => account_command(app, parameters),

                        Command::Hearts => hearts(app, parameters),

                        Command::UsernameColor => color_handler(app, write_stream, true, parameters).await,
                        Command::MessageColor => color_handler(app, write_stream, false, parameters).await,

                        #[cfg(feature = "client_voice")]
                        Command::Mute => mute(app, parameters),

                        #[cfg(feature = "client_voice")]
                        Command::Record => tui::voice_message::toggle(app, write_stream, tx),

                        //NO ID STOPS WHATEVER PLAYS
                        #[cfg(feature = "client_voice")]
                        Command::Play => match parameters.map(|p| p.parse::<u64>())
                        {
                            None => tui::voice_message::stop(app),
                            Some(Ok(message_id)) => match app.voice_of(message_id)
                            {
                                Ok(hash) => tui::voice_message::play(app, hash, tx),
                                Err(error) => app.push_styled(error, theme::error()),
                            },
                            Some(Err(_)) => invalid_usage(app, None),
                        },

                        //A SWAP OR SOUND CHANGE SENT NOTHING
                        #[cfg(feature = "client_screen")]
                        Command::Screen =>
                        {
                            let (monitor, sound) = command::screen_parameters(parameters.as_deref());

                            if monitor.is_some()
                            {
                                app.push_styled(match screen::capture::current_monitor()
                                {
                                    Some(monitor) => t!("screen.swapped_to", monitor),
                                    None => t!("screen.swapped").to_owned(),
                                }, theme::ok());
                            }

                            if let Some(sound) = sound
                            {
                                app.push_styled(if sound { t!("screen.sound_on") } else { t!("screen.sound_off") }, theme::ok());
                            }
                        },

                        //INVALID COMMAND
                        Command::Invalid => invalid_usage(app, Some("invalid.command")),

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
            PacketCode::Username
            {
                username: input,
                device: share_device(),
            }
        },
        LoginState::Login => PacketCode::Login { password: input },
        LoginState::Register => PacketCode::Register { password: input },
        LoginState::None => PacketCode::MessageRequest { text: input, reply: None },
    };

    network::send(&mut *write_stream.lock().await, packet, options::get_keys().as_ref()).await;
}
