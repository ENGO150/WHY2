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

//! # Server Connection
//!
//! The server's side of the main connection: [`listen_client`] runs one client's whole session,
//! from the handshake through login to the packet loop, and the helpers around it broadcast to
//! other clients and tear connections down.
//!
//! # Session Lifecycle
//! 1. **Handshake** — the connection is registered as
//!    [`NonAuthenticated`](connection::Connection::NonAuthenticated) and a key exchange runs
//!    (`handshake::key_exchange`).
//! 2. **Login** — the server sends its rules, checks the client's version and walks it through
//!    the username and password prompts, refusing banned names and addresses and a username
//!    that is already connected.
//! 3. **Session** — once authenticated the connection becomes
//!    [`Authenticated`](connection::Connection::Authenticated), the client is pushed the history,
//!    the user list and the voice roster unasked, and every packet is dispatched until the
//!    connection ends.
//!
//! # Shared State
//! [`CONNECTIONS`] holds every connection by peer address. Nothing may hold a guard on it across
//! an `.await`: the pattern throughout is to collect what is needed into locals, drop the guard,
//! then await.
//!
//! # Broadcasting
//! [`send_to_all`] sends to every authenticated client with no channel filter; a packet whose
//! meaning depends on a channel names it and the client files it. Each recipient's copy is sent
//! from its own task, so a slow client delays nobody else. [`send_to_others`] does filter by
//! channel, for notices only worth anything to those present (the typing indicator).
//!
//! # Logging
//! A log line identifies a client by its address only. An auxiliary connection is logged as the
//! main connection that asked for it ([`log_addr`]).

//MODULES
pub mod connection;
pub mod handshake;
mod session;

use std::
{
    future::Future,
    net::{ IpAddr, SocketAddr },
    time::{ Instant, Duration },
    collections::{ HashSet, HashMap },
    sync::
    {
        Arc,
        LazyLock,
        Mutex as MutexSync,
    },
};

use tokio::
{
    fs,
    sync::{ Mutex, oneshot },
    net::tcp::OwnedWriteHalf,
    task::{ self, AbortHandle },
};

use rand::
{
    TryRng,
    rngs::SysRng,
};

use zeroize::Zeroizing;

use dashmap::DashMap;

use connection::
{
    AvailableFile,
    ConnectionType,
    Connection,
};

use crate::
{
    misc,
    options,
    role::Role,
    crypto::password,
    config::
    {
        self,
        private_messages::Participants,
    },
    consts::
    {
        self,
        SharedKeys,
        Streams,
    },
    network::
    {
        self,
        voice::server as voice_server,
        file::server as file,
        server::session::ClientSession,
        codes::
        {
            PacketCode,
            OnlineUser,
            OfflineUser,
            Device,
        },
    },
};

//LISTS
/// Tokens minted for auxiliary connections, awaiting their socket: `token → (client id, kind,
/// minted at)`.
pub static PENDING_TOKENS: LazyLock<DashMap<[u8; 32], (usize, ConnectionType, Instant)>> = LazyLock::new(|| DashMap::new());
/// Every connection, by peer address.
pub static CONNECTIONS: LazyLock<DashMap<SocketAddr, Connection>> = LazyLock::new(|| DashMap::new());     //LIST FOR EACH CLIENT CONNECTION
/// Every user's shared files, by username.
pub static AVAILABLE_FILES: LazyLock<DashMap<String, Vec<AvailableFile>>> = LazyLock::new(|| DashMap::new()); //LIST FOR UPLOADED FILES

//PRIVATE
//STRUCTS
/// A client that has completed the handshake and the login prompts.
struct AuthenticatedClient
{
    /// The agreed session keys.
    keys: SharedKeys,
    /// The account's username.
    username: String,
    /// What the client says it runs on.
    device: Option<Device>,
}

//FUNCTIONS
/// Returns the address of the live session an account has open, if any.
fn session_of(id: usize) -> Option<SocketAddr> //THE SESSION THE ACCOUNT HAS OPEN
{
    CONNECTIONS.iter()
        .find(|entry| entry.role().is_some() && entry.id() == Some(&id))
        .map(|entry| *entry.key())
}

/// Strips control characters from profile text and trims it.
fn clean(text: &str) -> String //WHAT A CLIENT MAY PUT IN A PROFILE
{
    text.chars().filter(|character| !character.is_control()).collect::<String>().trim().to_string()
}

/// Sends the user list: everybody connected who is not hidden, plus the offline accounts if
/// `show_offline_users` is on.
///
/// # Parameters
/// - `write_stream`: The client's write half.
/// - `peer_addr`: The client's address.
/// - `keys`: The client's session keys.
async fn send_list(write_stream: &Arc<Mutex<OwnedWriteHalf>>, peer_addr: &SocketAddr, keys: &SharedKeys) //SEND LIST OF USERS
{
    let mut users = Vec::new();

    //ITERATE OVER CONNECTIONS, CREATE JSON OF USERS
    for connection_enum in CONNECTIONS.iter()
    {
        if let Connection::Authenticated { username: uname, id: user_id, channel, device, .. } = connection_enum.value()
        {
            //SKIP "OFFLINE" ONLINE USERS
            if !config::users::online_status(uname) { continue; }

            users.push(OnlineUser
            {
                username: uname.clone(),
                username_color: config::users::colors(uname).username_color,
                id: *user_id,
                channel: channel.clone(),
                device: device.clone(),
                role: config::users::role(uname).unwrap_or_default(),
            });
        }
    }

    //GET NON-BANNED OFFLINE USERS (IF ENABLED)
    let offline = if config::read_config::<bool>("show_offline_users")
    {
        let connected: HashSet<&str> = users.iter().map(|user| user.username.as_str()).collect();

        Some(config::users::all().into_iter()
            .filter(|username| !connected.contains(username.as_str()) && !config::bans::banned(username))
            .map(|username|
            {
                //GET UNAME COLOR & ROLE
                let username_color = config::users::colors(&username).username_color;
                let role = config::users::role(&username).unwrap_or_default();

                OfflineUser
                {
                    id: config::users::id(&username).unwrap_or_default(),
                    username,
                    username_color,
                    role,
                }
            }).collect::<Vec<OfflineUser>>())
    } else { None };

    //LOG
    log::debug!("Sending user list ({} online{}): {peer_addr}", users.len(), if let Some(ref offline) = offline
    {
        format!(", {} offline", offline.len())
    } else { String::new() });

    //SEND LIST BACK TO CLIENT
    network::send(&mut *write_stream.lock().await, PacketCode::List
    {
        online: users,
        offline,
    }, Some(&keys)).await;
}

/// Sends the whole ban list.
async fn send_bans(write_stream: &Arc<Mutex<OwnedWriteHalf>>, keys: &SharedKeys) //SEND THE WHOLE BAN LIST
{
    network::send(&mut *write_stream.lock().await, PacketCode::ServerBans
    {
        users: config::bans::users(),
        ips: config::bans::ips(),
    }, Some(keys)).await;
}

/// Sends one page of the lobby history, if `persistent_messages` is on.
///
/// # Parameters
/// - `write_stream`: The client's write half.
/// - `keys`: The client's session keys.
/// - `before`: The page cursor, or `None` for the newest page (sent at login, and skipped when
///   nothing is stored).
async fn send_history(write_stream: &Arc<Mutex<OwnedWriteHalf>>, keys: &SharedKeys, before: Option<u64>) //SEND A PAGE OF THE STORED LOBBY MESSAGES
{
    if !config::read_config::<bool>("persistent_messages") { return; }

    let count: usize = config::read_config("history_page");
    let page = config::messages::page(before, count.max(1), consts::MAX_HISTORY_SIZE);

    //NOTHING STORED IS NOT WORTH A HEADING
    if before.is_none() && page.messages.is_empty() { return; }

    log::debug!("Replaying history ({} messages)", page.messages.len());

    network::send(&mut *write_stream.lock().await, PacketCode::History
    {
        messages: page.messages,
        start: page.start,
        more: page.more,
        kept: page.kept,
        older: before.is_some(),
    }, Some(keys)).await;
}

/// Sends one page of `username`'s private history, if `persistent_private_messages` is on.
///
/// # Parameters
/// - `write_stream`: The client's write half.
/// - `keys`: The client's session keys.
/// - `username`: The account the page is built for.
/// - `before`: The page cursor, or `None` for the newest page (sent at login, and skipped when
///   nothing is stored).
async fn send_private_history
(
    write_stream: &Arc<Mutex<OwnedWriteHalf>>,
    keys: &SharedKeys,
    username: &str,
    before: Option<u64>,
) //SEND A PAGE OF OUR STORED PRIVATE MESSAGES
{
    if !config::read_config::<bool>("persistent_private_messages") { return; }

    let count: usize = config::read_config("history_page");
    let page = config::private_messages::page(username, before, count.max(1), consts::MAX_HISTORY_SIZE);

    //NOTHING STORED IS NOT WORTH A HEADING
    if before.is_none() && page.messages.is_empty() { return; }

    log::debug!("Replaying private history ({} messages)", page.messages.len());

    network::send(&mut *write_stream.lock().await, PacketCode::PrivateHistory
    {
        messages: page.messages,
        start: page.start,
        more: page.more,
        kept: page.kept,
        older: before.is_some(),
    }, Some(keys)).await;
}

/// Removes every connection from one address.
async fn remove_connections(addr: &IpAddr, grace: bool, info: Option<&str>) //REMOVE CONNECTIONS BY IP
{
    let addrs: Vec<SocketAddr> = CONNECTIONS.iter()
        .filter(|conn| conn.key().ip() == *addr)
        .map(|conn| *conn.key()).collect();

    for addr in addrs
    {
        remove_connection(&addr, grace, info).await;
    }
}

/// Checks a new password against `min_password_length`.
fn validate_password(password: &str) -> bool //CHECK PASSWORD LENGTH (AND POSSIBLY STRENGTH IN THE FUTURE)
{
    password.len() >= config::read_config("min_password_length")
}

/// Verifies a login password on a blocking thread.
///
/// # Returns
/// `false` for an unknown user as well as for a wrong password.
async fn verify_password(username: &str, password: Zeroizing<String>) -> bool
{
    if let Some(hashed) = config::users::password(username)
    {
        task::spawn_blocking(move || password::compare_password_hash(&hashed, &password))
            .await.expect("Comparing password failed")
    } else //UNKNOWN USER (OR FAKE LOGIN)
    {
        false
    }
}

//PUBLIC
//A CLIENT'S MAIN ADDRESS (NEVER UNDER A GUARD)
/// Returns the main-connection address of client `id`, for logging an auxiliary connection.
///
/// Walks [`CONNECTIONS`], so it must never be called while a guard on that map is held.
pub fn log_addr(id: &usize) -> String
{
    CONNECTIONS.iter().find(|conn| conn.id() == Some(id))
        .map(|conn| conn.peer_addr().to_string())
        .unwrap_or_else(|| String::from("<gone>"))
}

/// Spawns a task that is handed its own [`AbortHandle`].
///
/// The handle is passed through a oneshot before the task body runs, so a task can register
/// itself (for example on its [`Connection`]) without racing its own abort.
///
/// # Returns
/// The task's abort handle.
pub fn spawn_with_abort<F, Fut>(f: F) -> AbortHandle //SPAWN TASK WHICH KNOWS ITS OWN AbortHandle
where
    F: FnOnce(AbortHandle) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    //CHANNEL FOR HANDING THE HANDLE OVER TO THE TASK
    let (tx, rx) = oneshot::channel::<AbortHandle>();

    let handle = tokio::spawn(async move
    {
        //WAIT FOR OWN HANDLE (PREVENTS A RACE)
        let task = match rx.await
        {
            Ok(t) => t,
            Err(_) => return
        };

        f(task).await;
    });

    let task = handle.abort_handle();
    tx.send(task.clone()).ok();

    task
}

/// Sends a packet to every authenticated client.
pub fn send_to_all(code: PacketCode) //SEND PACKET TO ALL CLIENTS
{
    send_to_all_but(code, None);
}

/// Sends a private update to every connected session of its two participants.
fn send_to_accounts(participants: &Participants, packet: impl Fn(u64) -> PacketCode) //NEVER BROADCAST PRIVATE UPDATES
{
    let entries: Vec<_> = CONNECTIONS.iter().filter_map(|entry|
    {
        let id = participants.message_id(entry.username()?)?;
        Some((entry.write_stream().clone(), entry.keys().cloned(), id))
    }).collect();

    for (stream, keys, id) in entries
    {
        let code = packet(id);
        tokio::spawn(async move
        {
            network::send(&mut *stream.lock().await, code, keys.as_ref()).await;
        });
    }
}

/// Sends a packet to every authenticated client except one.
///
/// Each copy is encrypted under its recipient's keys and sent from a task of its own.
///
/// # Parameters
/// - `code`: The packet.
/// - `except`: The session id to skip.
pub fn send_to_all_but(code: PacketCode, except: Option<usize>) //SEND PACKET TO ALL CLIENTS, ONE EXCLUDED
{
    //COLLECT EACH CLIENT
    let entries: Vec<Connection> = CONNECTIONS.iter().filter_map(|entry|
    {
        match entry.value()
        {
            Connection::Authenticated { id, .. } if except != Some(*id) =>
            {
                //FOUND, COLLECT
                Some(entry.value().clone())
            },
            _ => None,
        }
    }).collect();

    for ref entry in entries
    {
        let write_stream = entry.write_stream().clone();
        let code = code.clone();
        let keys = entry.keys().cloned();

        tokio::spawn(async move
        {
            network::send(&mut *write_stream.lock().await, code, keys.as_ref()).await;
        });
    }
}

/// Sends a packet to every authenticated client in one channel except the sender.
///
/// # Parameters
/// - `code`: The packet.
/// - `channel`: The channel; `None` is the lobby.
/// - `except`: The sender's session id.
pub fn send_to_others(code: PacketCode, channel: Option<&str>, except: usize) //SEND PACKET TO A CHANNEL, SENDER EXCLUDED
{
    //COLLECT EACH OTHER CLIENT IN SAME CHANNEL
    let entries: Vec<Connection> = CONNECTIONS.iter().filter_map(|entry|
    {
        match entry.value()
        {
            Connection::Authenticated { channel: c, id, .. } if c.as_deref() == channel && *id != except =>
            {
                //FOUND, COLLECT
                Some(entry.value().clone())
            },
            _ => None,
        }
    }).collect();

    for ref entry in entries
    {
        let write_stream = entry.write_stream().clone();
        let code = code.clone();
        let keys = entry.keys().cloned();

        tokio::spawn(async move
        {
            network::send(&mut *write_stream.lock().await, code, keys.as_ref()).await;
        });
    }
}

/// Removes a connection and everything attached to it, then aborts its task.
///
/// Aborts its uploads and screen share, detaches its viewers, leaves voice, broadcasts its
/// [`Leave`](PacketCode::Leave) and announces an abandoned channel. The connection's own task is
/// aborted **last**, since this is frequently called by the very task it aborts.
///
/// # Parameters
/// - `peer_addr`: The connection's address.
/// - `grace`: Whether to tell the client first ([`Disconnect`](PacketCode::Disconnect)).
/// - `info`: A short reason for the log.
pub async fn remove_connection(peer_addr: &SocketAddr, grace: bool, info: Option<&str>) //REMOVE CONNECTION BY PEER ADDRESS
{
    //REMOVE CONNECTION
    let mut connection = match CONNECTIONS.remove(peer_addr)
    {
        Some((_, conn)) => conn,
        None => return
    };

    //SEND DISCONNECT CODE IF GRACEFUL
    if grace
    {
        network::send(&mut *connection.write_stream().lock().await, PacketCode::Disconnect, connection.keys()).await;
    }

    //CLOSE ALL FILE STREAMS
    if let Some(streams) = connection.file_streams()
    {
        //COLLECT UIDS
        let uids: Vec<u64> = streams.lock().unwrap().keys().copied().collect();

        for uid in uids
        {
            connection.remove_file_stream(uid);
        }
    }

    //CLOSE SCREEN UPLOAD STREAM
    if let Some(id) = connection.remove_screen_stream()
    {
        //DEATTACH ALL ATTACHED CLIENTS
        deattach(id, connection.username().unwrap()).await;
    }

    //AUTHENTICATED ACTIONS
    if connection.is_authenticated()
    {
        //TELL THE SHARER WE STOPPED WATCHING
        if let Some(target_id) = connection.attached_screen().as_ref().map(|a| a.target_id)
        {
            notify(target_id, PacketCode::Deattached
            {
                username: connection.username().unwrap().to_owned(),
            }).await;
        }

        //DISCONNECT FROM VOICE CHAT
        if options::voice_chat_enabled()
        {
            let id = *connection.id().unwrap();

            //VOICE MEMBERSHIP ENDS INDEPENDENTLY OF ONLINE VISIBILITY
            if voice_server::CONNECTIONS.contains_key(&id)
            {
                send_to_all(PacketCode::VoiceLeave { id, channel: connection.channel().clone() });
            }

            voice_server::remove_connection(&id);
        }

        //REMOVE UPLOADS
        let username = connection.username().unwrap();
        let _ = fs::remove_dir_all(misc::get_upload_dir(username)).await; //REMOVE FILES
        file::ACTIVE_FILESHARES.retain(|_, u| u.client_id != *connection.id().unwrap());
        AVAILABLE_FILES.remove(username); //REMOVE AVAILABLE FILES

        //REMOVE FROM last_pm
        for mut conn in CONNECTIONS.iter_mut().filter(|c| c.last_pm().as_ref() == connection.id())
        {
            conn.unset_last_pm();
        }

        //SEND LEAVE MESSAGE, UNLESS THE ACCOUNT WAS HIDDEN
        if config::users::online_status(username)
        {
            send_to_all(PacketCode::Leave
            {
                username: username.to_string(),
                id: *connection.id().unwrap(),
                registered: config::users::contains(username),
            });
        }
    }

    log::info!
    (
        "Close connection{}: {peer_addr} ({}, {} left)",

        if let Some(info) = info
        {
            format!(" ({info})")
        } else { String::new() },

        if connection.is_authenticated() { "authenticated" } else { "unauthenticated" },
        CONNECTIONS.len(),
    );

    //SHUT DOWN THE HANDLER TASK (MUST BE LAST)
    connection.task().abort();
}

/// Returns whether a session with this username is connected.
fn user_connected(username: &str) -> bool //CHECK IF CLIENT WITH username IS CONNECTED
{
    CONNECTIONS.iter().any(|conn|
    {
        conn.username().map_or(false, |u| u == &username.to_string())
    })
}

/// Stores the agreed keys on a connection after a key exchange.
fn update_client_keys(peer_addr: &SocketAddr, keys: &SharedKeys) //ADD KEY TO NonAuthenticated CLIENT AFTER KEY EXCHANGE
{
    //UPDATE CONNECTION
    CONNECTIONS.alter(peer_addr, |_, old_connection|
    {
        match old_connection
        {
            Connection::NonAuthenticated { write_stream, task, seq, peer_addr, connect, obfuscation_key, .. } =>
            {
                Connection::NonAuthenticated
                {
                    write_stream,
                    task,
                    peer_addr,
                    username: None,
                    keys: Some(keys.to_owned()),
                    obfuscation_key,
                    last_activity: Instant::now(),
                    seq,
                    connect,
                }
            },

            Connection::Authenticated { write_stream, task, file_streams, screen_stream, username, device, role,
                id, attached_screen, last_activity, last_attachment, last_typing, last_pm, channel, seq, server_seq, peer_addr, alive,
                muted, credit, refill, throttles, .. } =>
            {
                Connection::Authenticated
                {
                    write_stream,
                    task,
                    file_streams,
                    screen_stream,
                    peer_addr,
                    username,
                    device,
                    role,
                    id,
                    keys: keys.to_owned(),
                    attached_screen,
                    last_activity,
                    last_key_exchange: Instant::now(),
                    last_attachment,
                    last_typing,
                    last_pm,
                    spam_violations: 0,
                    credit,
                    refill,
                    throttles,
                    channel,
                    seq,
                    server_seq,
                    alive,
                    muted,
                }
            }
        }
    });
}

/// Promotes a connection to [`Authenticated`](Connection::Authenticated) and gives it a shared
/// file list.
///
/// The rate limits start fresh: the message gap and attachment and typing timers start satisfied, and
/// the packet bucket full.
fn authenticate_client(peer_addr: &SocketAddr, username: &str, device: &Option<Device>, role: Role, id: usize) //MOVE CONNECTION FROM NonAuthenticated TO Authenticated
{
    //UPDATE CONNECTION
    CONNECTIONS.alter(&peer_addr, |_, old_connection|
    {
        Connection::Authenticated
        {
            write_stream: old_connection.write_stream().clone(),
            task: old_connection.task().clone(),
            file_streams: Arc::new(MutexSync::new(HashMap::new())),
            screen_stream: None,
            peer_addr: *old_connection.peer_addr(),
            username: username.to_string(),
            device: device.clone(),
            role,
            id,
            keys: old_connection.keys().unwrap().to_owned(),
            attached_screen: None,
            last_activity: Instant::now() - Duration::from_millis(config::read_config("min_message_delay")),
            last_key_exchange: old_connection.last_key_exchange().copied().unwrap_or_else(Instant::now),
            last_attachment: Instant::now() - consts::ATTACHMENT_REQUEST_DELAY,
            last_typing: Instant::now() - consts::TYPING_INTERVAL,
            last_pm: old_connection.last_pm(),
            spam_violations: 0,
            credit: config::read_config::<f32>("max_packet_burst"),
            refill: Instant::now(),
            throttles: 0,
            channel: None,
            seq: *old_connection.seq(),
            server_seq: 0,
            alive: true,
            muted: false,
        }
    });

    //CREATE AVAILABLE FILES ENTRY
    AVAILABLE_FILES.insert(username.to_string(), Vec::new());

    log::info!("Authenticate connection: {peer_addr} (id {id}, role {role})");
}

/// Moves a client to a channel, announcing a channel that was created or abandoned.
///
/// The connection's rate-limit state is carried across, so switching channels cannot reset it.
fn update_client_channel(peer_addr: &SocketAddr, channel: &Option<String>) //MOVE CLIENT TO CHANNEL
{
    let mut old_channel = None; //PREVIOUS CHANNEL

    //UPDATE CONNECTION
    CONNECTIONS.alter(&peer_addr, |_, old_connection|
    {
        //GET PREVIOUS CHANNEL
        old_channel = old_connection.channel().clone();

        Connection::Authenticated
        {
            write_stream: old_connection.write_stream().clone(),
            task: old_connection.task().clone(),
            file_streams: old_connection.file_streams().unwrap().clone(),
            screen_stream: old_connection.screen_stream().clone(),
            peer_addr: *old_connection.peer_addr(),
            username: old_connection.username().unwrap().clone(),
            device: old_connection.device().clone(),
            role: *old_connection.role().unwrap(),
            id: *old_connection.id().unwrap(),
            keys: old_connection.keys().unwrap().to_owned(),
            attached_screen: old_connection.attached_screen().clone(),
            last_activity: Instant::now(),
            last_key_exchange: *old_connection.last_key_exchange().unwrap(),
            last_attachment: *old_connection.last_attachment().unwrap(),
            last_typing: *old_connection.last_typing().unwrap(),
            last_pm: old_connection.last_pm(),
            spam_violations: *old_connection.spam_violations().unwrap(),
            credit: *old_connection.credit().unwrap(),
            refill: *old_connection.refill().unwrap(),
            throttles: *old_connection.throttles().unwrap(),
            channel: channel.clone(),
            seq: *old_connection.seq(),
            server_seq: *old_connection.server_seq().unwrap(),
            alive: true,
            muted: *old_connection.muted(),
        }
    });

    //RETURN IF CLIENT SWITCHED TO SAME CHANNEL
    if old_channel == *channel { return; }

    log::info!("Channel switch: {peer_addr} ({} -> {})",
        if old_channel.is_some() { "channel" } else { "lobby" },
        if channel.is_some() { "channel" } else { "lobby" });

    //CHECK IF CHANNEL WAS ABANDONED
    if let Some(old_channel) = old_channel
    {
        if !CONNECTIONS.iter().any(|c| c.channel().as_ref() == Some(&old_channel))
        {
            //NO CLIENT IS IN OLD CHANNEL
            log::info!("Channel destroyed (last client left): {peer_addr}");

            send_to_all(PacketCode::ChannelDestroyed
            {
                name: old_channel,
            });
        }
    }

    //CHECK IF CHANNEL WAS CREATED
    if let Some(channel) = channel
    {
        if CONNECTIONS.iter().filter(|c| c.channel().as_ref() == Some(channel)).count() == 1
        {
            //CLIENT IS FIRST IN CHANNEL
            log::info!("Channel created: {peer_addr}");

            send_to_all(PacketCode::ChannelCreated
            {
                name: channel.clone(),
            });
        }
    }
}

/// Sends client `id` the voice roster of its channel, itself excluded.
async fn send_voice_clients(stream: &mut OwnedWriteHalf, keys: &SharedKeys, id: usize)
{
    //FIND CHANNEL
    let sender_channel = match CONNECTIONS.iter().find(|e| e.value().id() == Some(&id))
    {
        Some(entry) => entry.value().channel().clone(),
        None => return,
    };

    let mut clients: Vec<(usize, String)> = Vec::new();

    //COLLECT VOICE CLIENTS
    for entry in CONNECTIONS.iter()
    {
        let conn = entry.value();

        let uid = match conn.id()
        {
            Some(i) => *i,
            None => continue
        };

        //FILTERS
        if uid == id { continue; } // IGNORE SELF
        if conn.channel() != &sender_channel { continue; } //IGNORE ANOTHER CHANNELS

        //CHECK IF IS IN VOICE
        if voice_server::CONNECTIONS.contains_key(&uid)
        {
            //ADD USERNAMES
            if let Some(username) = conn.username()
            {
                 clients.push((uid, username.clone()));
            }
        }
    }

    //SEND
    network::send(stream, PacketCode::VoiceClients { clients }, Some(keys)).await;
}

/// Mints a one-time token for an auxiliary connection of client `id`.
fn open_connection(id: usize, conn_type: ConnectionType) -> [u8; 32] //ADD NEW TOKEN
{
    //GENERATE RANDOM TOKEN
    let mut token = [0u8; 32];
    SysRng.try_fill_bytes(&mut token).unwrap();

    //OPEN NEW CONNECTION
    PENDING_TOKENS.insert(token, (id, conn_type, Instant::now()));

    token
}

//PUBLIC
/// Sends a packet to the client with session id `id`, if connected.
pub async fn notify(id: usize, code: PacketCode) //SEND PACKET TO THE CLIENT WITH id
{
    //COLLECT INTO LOCALS - NO GUARD ACROSS THE await
    let target = CONNECTIONS.iter().find(|conn| conn.id() == Some(&id))
        .map(|conn| (conn.write_stream().clone(), conn.keys().cloned()));

    if let Some((write_stream, keys)) = target
    {
        network::send(&mut *write_stream.lock().await, code, keys.as_ref()).await;
    }
}

/// Broadcasts the current server icon, acknowledging the change to client `id`.
pub async fn icon_changed(id: usize) //TELL EVERYBODY THE SERVER'S PICTURE, AND id THAT IT WAS SAVED
{
    let hash = config::server_icon();

    send_to_all_but(PacketCode::ServerIcon { hash, save: false }, Some(id));
    notify(id, PacketCode::ServerIcon { hash, save: true }).await;
}

/// Detaches every viewer from a screen share and tells them.
///
/// # Parameters
/// - `sharer_id`: The sharer's session id.
/// - `sharer_uname`: The sharer's username, as shown to the viewers.
pub async fn deattach(sharer_id: usize, sharer_uname: &String) //DEATTACH ALL ATTACHED CLIENTS
{
    let mut to_notify = Vec::new();

    for mut conn in CONNECTIONS.iter_mut()
        .filter(|conn| conn.attached_screen().as_ref().is_some_and(|a| a.target_id == sharer_id))
    {
        conn.deattach_screen();
        to_notify.push((conn.write_stream().clone(), conn.keys().cloned()));
    }

    for (stream_mutex, keys) in to_notify
    {
        network::send(&mut *stream_mutex.lock().await,
            PacketCode::Deattach { username: sharer_uname.to_owned() }, keys.as_ref()).await;
    }
}

/// Registers a newly accepted connection before it has identified itself.
///
/// # Parameters
/// - `streams`: The connection's halves.
/// - `peer_addr`: The client's address.
/// - `obfuscation_key`: The 32 bytes the client opened with.
/// - `task`: This task's abort handle, stored on the connection.
fn register_connection
(
    streams: &Streams<'_>,
    peer_addr: SocketAddr,
    obfuscation_key: [u8; 32],
    task: AbortHandle,
)
{
    //PUSH NEW CONNECTION
    CONNECTIONS.insert(peer_addr, Connection::NonAuthenticated
    {
        write_stream: streams.1.clone(),
        task,
        peer_addr,
        username: None,
        keys: None,
        obfuscation_key,
        last_activity: Instant::now(),
        seq: 0,
        connect: Instant::now(),
    });
}

/// Performs the key exchange, the version check and the welcome packet.
///
/// A refusal removes the connection.
///
/// # Returns
/// The agreed session keys, or `None` if the connection was refused.
async fn establish_session
(
    streams: &mut Streams<'_>,
    peer_addr: &SocketAddr,
    obfuscation_key: &[u8; 32],
) -> Option<SharedKeys>
{
    //GET ENCRYPTION & MAC KEYS
    let mut keys = (Zeroizing::new(vec![]), Zeroizing::new(vec![]));
    handshake::key_exchange(streams, peer_addr, obfuscation_key, &mut keys, None).await;

    //CHECK FOR VALID KEYS
    if keys.0.is_empty() || keys.1.is_empty()
    {
        log::warn!("Key exchange failed: {peer_addr}");

        remove_connection(peer_addr, false, None).await;
        return None;
    }

    log::debug!("Key exchange done: {peer_addr}");

    //ASK CLIENT FOR THEIR PACKAGE VERSION
    if config::read_config("check_client_version")
    {
        let version = handshake::ask_version(streams, &keys).await;
        if version.is_none() || version != Some(misc::get_version().to_string())
        {
            log::warn!("Version mismatch: {peer_addr} (client {}, server {})",
                version.as_deref().unwrap_or("none"), misc::get_version());

            remove_connection(peer_addr, true, Some("version")).await;
            return None;
        }
    }

    //SEND PACKET WITH REQUIRED SERVER INFO
    handshake::send_welcome_packet(&mut *streams.1.lock().await, &keys).await;

    Some(keys)
}

/// Asks for a username until one is accepted, and records it on the connection.
///
/// # Returns
/// The chosen username and device, or `None` if the client never sent a valid one.
async fn ask_username
(
    streams: &mut Streams<'_>,
    peer_addr: &SocketAddr,
    keys: &SharedKeys,
    disabled_registration: bool,
) -> Option<(String, Option<Device>)>
{
    //GET USERNAME FROM USER
    let mut username: Option<String> = None; //USER ENTERED USERNAME
    let mut device: Option<Device> = None; //USER DEVICE

    //USERNAME CONFIGS
    let max_tries = config::read_config::<usize>("max_auth_tries"); //MAX n
    let min_len = config::read_config::<usize>("min_username_length");
    let max_len = config::read_config::<usize>("max_username_length");

    //TELL USER IF REGISTRATIONS ARE DISABLED
    if disabled_registration
    {
        network::send(&mut *streams.1.lock().await, PacketCode::RegisterDisabled, Some(keys)).await;
    }

    //ASK n TIMES
    for attempt in 0..max_tries
    {
        log::debug!("Asking for username: {peer_addr} (try {}/{max_tries})", attempt + 1);

        //SEND PICK USERNAME CODE
        network::send(&mut *streams.1.lock().await, PacketCode::UsernameRequest, Some(keys)).await;

        //READ USERNAME
        if let Some(PacketCode::Username { username: uname, device: dev }) =
            network::receive(streams, Some(keys), None).await
        {
            //CHECK USERNAME VALIDITY
            if uname.len() >= min_len && uname.len() <= max_len &&
                uname.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') &&
                !user_connected(&uname) && uname != options::get_server_username() &&
                uname != "everyone"
            {
                username = Some(uname);
                device = dev;
                break; //USERNAME CONDITIONS MET, STOP ASKING
            }

            log::debug!("Username refused: {peer_addr}");
        }
    }

    //NO USERNAME RECEIVED, DISCONNECT CLIENT
    let Some(username) = username else
    {
        remove_connection(peer_addr, true, Some("username")).await;
        return None;
    };

    //UPDATE USERNAME IN NonAuthenticated
    if let Some(mut conn) = CONNECTIONS.get_mut(peer_addr)
    {
        if !conn.is_authenticated()
        {
            //UPDATE
            *conn.username_mut() = Some(username.clone());
        }
    }

    Some((username, device))
}

/// Registers a new account, asking for a password until one is valid.
///
/// Refusals remove the connection.
///
/// # Returns
/// `true` if the account was created and the client may continue.
async fn register_account
(
    streams: &mut Streams<'_>,
    peer_addr: &SocketAddr,
    keys: &SharedKeys,
    username: &str,
) -> bool
{
    let max_tries = config::read_config::<usize>("max_auth_tries"); //MAX n
    let mut password: Option<Zeroizing<String>> = None;

    //KEEP ASKING FOR PASSWORD n TIMES
    for _ in 0..max_tries
    {
        //SEND REGISTER CODE
        network::send(&mut *streams.1.lock().await, PacketCode::RegisterRequest, Some(keys)).await;

        //WAIT FOR ANSWER
        match network::receive(streams, Some(keys), None).await
        {
            Some(PacketCode::Register { password: pass }) =>
            {
                let pass = Zeroizing::new(pass);

                //VALIDATE
                if validate_password(&pass)
                {
                    password = Some(pass);
                    break;
                }
            },

            _ =>
            {
                remove_connection(peer_addr, false, Some("register")).await;
                return false;
            }
        };
    }

    if password.is_none()
    {
        remove_connection(peer_addr, true, Some("register")).await;
        return false;
    }

    log::info!("Registering new user: {peer_addr}");

    //HASH PASSWORD (ARGON2 IS CPU HEAVY)
    let hash = task::spawn_blocking(move || password::hash_password(password.as_ref().unwrap().as_str()))
        .await.expect("Hashing password failed");

    //SAVE PASSWORD
    if config::users::add(username, &hash)
    {
        //FIRST USER, NOTIFY ABOUT OWNER ROLE
        log::info!("First user registered, granting {}: {peer_addr}", Role::Owner);

        network::send(&mut *streams.1.lock().await, PacketCode::FirstUser, Some(keys)).await;
    }

    true
}

/// Logs an existing account in.
///
/// Refusals remove the connection.
///
/// # Returns
/// `true` if the password was accepted and the client may continue.
async fn login_account
(
    streams: &mut Streams<'_>,
    peer_addr: &SocketAddr,
    keys: &SharedKeys,
    username: &str,
) -> bool
{
    //SEND LOGIN CODE
    network::send(&mut *streams.1.lock().await, PacketCode::LoginRequest, Some(keys)).await;

    //WAIT FOR ANSWER
    let password = loop
    {
        match network::receive(streams, Some(keys), None).await
        {
            Some(PacketCode::Login { password }) => break Zeroizing::new(password),

            _ =>
            {
                remove_connection(peer_addr, false, Some("login")).await;
                return false;
            },
        }
    };

    //VERIFY PASSWORD (ARGON2 IS CPU HEAVY)
    let valid = if password.is_empty() || config::bans::banned(username)
    {
        false
    } else { verify_password(username, password).await };

    //INVALID PASSWORD, DISCONNECT CLIENT
    if !valid
    {
        log::warn!("Login refused: {peer_addr}");

        remove_connection(peer_addr, true, Some("login")).await;
        return false;
    }

    log::debug!("Login accepted: {peer_addr}");
    true
}

/// Runs the key exchange and the login prompts for a new connection.
///
/// Every refusal removes the connection before returning.
///
/// # Parameters
/// - `streams`: The connection's halves.
/// - `peer_addr`: The client's address.
/// - `obfuscation_key`: The 32 bytes the client opened with; obfuscates the handshake and is
///   signed into the server's offer as its nonce.
///
/// # Returns
/// The client's keys, username and device, or `None` if the connection was refused.
async fn authenticate_connection
(
    streams: &mut Streams<'_>,
    peer_addr: &SocketAddr,
    obfuscation_key: &[u8; 32],
) -> Option<AuthenticatedClient>
{
    let keys = establish_session(streams, peer_addr, obfuscation_key).await?;

    let disabled_registration = !config::read_config::<bool>("allow_register");
    let (username, device) = ask_username(streams, peer_addr, &keys, disabled_registration).await?;

    let user_exists = config::users::contains(&username);

    //REGISTRATION (OR "FAKE" LOGIN ON DISABLED REGISTER) OR LOGIN
    let allowed = if !user_exists && !disabled_registration
    {
        register_account(streams, peer_addr, &keys, &username).await
    } else
    {
        login_account(streams, peer_addr, &keys, &username).await
    };

    if !allowed { return None; }

    Some(AuthenticatedClient
    {
        keys,
        username,
        device,
    })
}

/// Runs one client's session, from the handshake to the end of the connection.
///
/// See the module documentation for the lifecycle. Every exit path removes the connection.
///
/// # Parameters
/// - `streams`: The connection's halves.
/// - `peer_addr`: The client's address.
/// - `obfuscation_key`: The 32 bytes the client opened with; obfuscates the handshake and is
///   signed into the server's offer as its nonce.
/// - `task`: This task's abort handle, stored on the connection.
pub async fn listen_client //CLIENT -> SERVER COMMUNICATION
(
    streams: &mut Streams<'_>,
    peer_addr: SocketAddr,
    obfuscation_key: [u8; 32],
    task: AbortHandle,
)
{
    log::info!("New connection: {peer_addr}");

    register_connection(streams, peer_addr, obfuscation_key, task);

    //HANDSHAKE, VERSION CHECK & LOGIN
    let Some(client) = authenticate_connection(streams, &peer_addr, &obfuscation_key).await else { return; };

    let AuthenticatedClient { keys, username, device } = client;

    let id = config::users::id(&username).unwrap_or_default(); //CONSTANT USER ID
    let role = config::users::role(&username).unwrap_or_default(); //WHAT THIS CLIENT IS ALLOWED TO ASK FOR

    //AUTHENTICATE CLIENT
    authenticate_client(&peer_addr, &username, &device, role, id);

    //SEND WHAT WAS SAID IN THE LOBBY BEFORE THIS
    send_history(&streams.1, &keys, None).await;
    send_private_history(&streams.1, &keys, &username, None).await; //OFC ONLY USER'S OWN PMS

    //TELL CLIENT ALL REQUIRED DATA
    network::send(&mut *streams.1.lock().await, PacketCode::Accept
    {
        id,
        username: username.clone(),
        role,
        colors: config::users::colors(&username),
    }, Some(&keys)).await;

    //SEND LIST OF USERS
    send_list(&streams.1, &peer_addr, &keys).await;

    //SEND JOIN ANNOUNCEMENT, IF NOT HIDDEN
    if config::users::online_status(&username)
    {
        send_to_all(PacketCode::Join
        {
            username: username.clone(),
            username_color: config::users::colors(&username).username_color,
            id,
            device,
            role,
        });
    }

    //TELL THE CLIENT WHO IS ALREADY IN VOICE
    if options::voice_chat_enabled()
    {
        send_voice_clients(&mut *streams.1.lock().await, &keys, id).await;
    }

    //THE PACKET LOOP LIVES IN ITS OWN MODULE
    ClientSession::new(streams, peer_addr, obfuscation_key, keys, username, id, role).run().await;
}

/// Gracefully disconnects every client.
pub async fn disconnect_all() //DISCONNECT ALL CLIENTS
{
    //ITERATE OVER ALL ADDRESSES, REMOVE CONNECTIONS
    let addrs: Vec<SocketAddr> = CONNECTIONS.iter().map(|conn| *conn.peer_addr()).collect();

    log::info!("Disconnecting {} connections", addrs.len());
    for addr in &addrs
    {
        remove_connection(addr, true, None).await; //REMOVE GRACEFULLY
    }
}

/// Disconnects every connection that has been inactive too long: no chat message for
/// `communication_time` seconds, or no login within `max_auth_time`.
pub async fn disconnect_inactive() //DISCONNECT ALL INACTIVE CLIENTS
{
    let now = Instant::now();

    //COLLECT ADDRESSES OF INACTIVE CONNECTIONS
    let inactive_addrs: Vec<SocketAddr> = CONNECTIONS.iter()
        .filter(|conn| conn.is_inactive(Some(now)))
        .map(|conn| *conn.peer_addr())
        .collect();

    if !inactive_addrs.is_empty() { log::debug!("Disconnecting {} inactive connections", inactive_addrs.len()); }

    //DISCONNECT INACTIVE CLIENTS
    for addr in &inactive_addrs
    {
        remove_connection(addr, true, Some("inactive")).await;
    }
}

/// Sends a keepalive to every authenticated client, first disconnecting any that did not answer
/// the previous one.
pub async fn send_keepalive() //SEND KEEPALIVE PACKET TO ALL CLIENTS
{
    //COLLECT ALL CLIENT ADDRESSES
    let addresses: Vec<SocketAddr> = CONNECTIONS.iter()
        .filter(|entry| entry.is_authenticated())
        .map(|entry| *entry.key())
        .collect();

    let mut dead_clients = Vec::new();

    //PREPARE
    for addr in addresses
    {
        let mut stream = None;
        let mut keys = None;

        if let Some(mut conn) = CONNECTIONS.get_mut(&addr)
        {
            //COLLECT DEAD BODIES
            if !conn.is_alive()
            {
                dead_clients.push(addr);
                continue;
            }

            //COPY STREAM & KEYS
            stream = Some(conn.write_stream().clone());
            keys = conn.keys().cloned();

            //PRONOUNCE DEAD UNTIL ECHO
            conn.set_alive(false);
        }

        //SEND KEEPALIVES
        if let Some(stream) = stream
        {
            network::send(&mut *stream.lock().await, PacketCode::KeepAlive, keys.as_ref()).await;
        }
    }

    //DISCONENCT DEAD CONNECTIONS
    for dead in dead_clients
    {
        //HAIL SATAN, AVE CLIENT
        log::warn!("Missed keepalive: {dead}");

        remove_connection(&dead, false, Some("dead")).await;
    }
}
