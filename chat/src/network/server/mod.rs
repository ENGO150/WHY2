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
    net::tcp::OwnedWriteHalf,
    sync::
    {
        Mutex,
        mpsc::{ self, UnboundedSender },
        oneshot::{ self, Sender },
    },
    task::
    {
        self,
        Id,
        AbortHandle,
    },
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
    SharedFiles,
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
/// Tokens minted for auxiliary connections, awaiting their socket
pub static PENDING_TOKENS: LazyLock<DashMap<[u8; 32], PendingToken>> = LazyLock::new(|| DashMap::new());
/// Every connection, by peer address.
pub static CONNECTIONS: LazyLock<DashMap<SocketAddr, Connection>> = LazyLock::new(|| DashMap::new());     //LIST FOR EACH CLIENT CONNECTION
/// Every user's shared files, by username.
pub static AVAILABLE_FILES: LazyLock<DashMap<String, Arc<SharedFiles>>> = LazyLock::new(|| DashMap::new());

/// Serializes account presence changes and queues roster packets in the same order.
static PRESENCE: LazyLock<MutexSync<Presence>> = LazyLock::new(|| MutexSync::new(Presence::default()));

//TYPES
type PresencePacket = (PacketCode, Option<Sender<()>>);

//IMPLEMENTATIONS
impl Presence
{
    fn send(&self, peer_addr: &SocketAddr, code: PacketCode)
    {
        if let Some(sender) = self.clients.get(peer_addr)
        {
            let _ = sender.send((code, None));
        }
    }

    fn send_voice_roster(&self, peer_addr: &SocketAddr, id: usize, channel: &Option<String>)
    {
        self.send(peer_addr, PacketCode::VoiceClients { clients: voice_server::roster(id, channel) });
    }

    fn close_voice(&mut self, peer_addr: SocketAddr, id: usize, token: Option<&[u8; 32]>) -> bool
    {
        let Some(slot) = voice_server::remove_connection(id, peer_addr, token) else { return false; };
        self.send(&peer_addr, PacketCode::Voice { token: None });
        self.broadcast(PacketCode::VoiceLeave { id, channel: slot.channel });
        true
    }

    fn toggle_voice(&mut self, peer_addr: SocketAddr, id: usize, username: &str, channel: &Option<String>)
    {
        if !self.clients.contains_key(&peer_addr) || self.close_voice(peer_addr, id, None) { return; }

        //REPLACE THE SLOT BEFORE NOTIFYING EITHER DEVICE
        let (token, previous) = voice_server::open_connection(id, peer_addr, username.to_string(), channel.clone());
        if let Some(previous) = previous
        {
            log::info!("Voice moved from {} to {peer_addr}", previous.owner_addr);
            self.send(&previous.owner_addr, PacketCode::VoiceMoved);
            self.broadcast(PacketCode::VoiceLeave { id, channel: previous.channel });
        } else
        {
            log::info!("Voice slot opened: {peer_addr}");
        }

        self.send(&peer_addr, PacketCode::Voice { token: Some(token) });
        self.broadcast(PacketCode::VoiceJoin { username: username.to_string(), id, channel: channel.clone() });
        self.send_voice_roster(&peer_addr, id, channel);
    }

    fn broadcast(&mut self, code: PacketCode)
    {
        self.clients.retain(|_, sender| sender.send((code.clone(), None)).is_ok());
    }
}

//PRIVATE
//STRUCTS
#[derive(Default)]
struct Presence
{
    /// Clients start receiving updates when their initial list is queued.
    clients: HashMap<SocketAddr, UnboundedSender<PresencePacket>>,
}

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

/// Whether both accounts may exchange live PMs under the server's presence settings.
pub(crate) fn private_live_allowed(sender: &str, recipient: &str) -> bool
{
    !config::read_config::<bool>("disable_ghost_pms") ||
        config::read_config::<bool>("persistent_private_messages") ||
        (config::users::online_status(sender) && config::users::online_status(recipient))
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
async fn send_list(write_stream: &Arc<Mutex<OwnedWriteHalf>>, peer_addr: &SocketAddr) //SEND LIST OF USERS
{
    let sent =
    {
        let mut presence = PRESENCE.lock().unwrap();
        if !CONNECTIONS.get(peer_addr).is_some_and(|conn| Arc::ptr_eq(conn.write_stream(), write_stream))
        {
            return;
        }

        let (sent, delivered) = oneshot::channel();

        //QUEUE THE SNAPSHOT BEFORE LATER PRESENCE EVENTS
        let list = user_list(peer_addr);
        let sender = presence.clients.entry(*peer_addr).or_insert_with(||
        {
            let (sender, mut receiver) = mpsc::unbounded_channel::<PresencePacket>();
            let write_stream = write_stream.clone();
            let peer_addr = *peer_addr;

            tokio::spawn(async move
            {
                while let Some((packet, sent)) = receiver.recv().await
                {
                    let mut stream = write_stream.lock().await;

                    //READ CURRENT KEYS AFTER TAKING THE WRITE LOCK
                    let keys = CONNECTIONS.get(&peer_addr)
                        .filter(|conn| Arc::ptr_eq(conn.write_stream(), &write_stream))
                        .and_then(|conn| conn.keys().cloned());

                    let Some(keys) = keys else { break; };
                    network::send(&mut *stream, packet, Some(&keys)).await;
                    if let Some(sent) = sent { let _ = sent.send(()); }
                }
            });
            sender
        });

        if sender.send((list, Some(sent))).is_err() { return; }
        delivered
    };

    let _ = sent.await;
}

/// Builds a roster snapshot while presence transitions are locked.
fn user_list(peer_addr: &SocketAddr) -> PacketCode
{
    let mut sessions: Vec<_> = CONNECTIONS.iter().filter_map(|entry|
    {
        let Connection::Authenticated { username, id, channel, device, .. } = entry.value() else { return None; };
        Some((*entry.key(), username.clone(), *id, channel.clone(), device.clone()))
    }).collect();

    //PICK THE LOWEST PEER ADDRESS FOR EACH USER
    sessions.sort_unstable_by_key(|(addr, ..)| *addr);

    let mut usernames = HashSet::new();
    let users: Vec<_> = sessions.into_iter().filter_map(|(_, username, id, channel, device)|
    {
        if !usernames.insert(username.clone()) || !config::users::online_status(&username) { return None; }

        Some(OnlineUser
        {
            username_color: config::users::colors(&username).username_color,
            role: config::users::role(&username).unwrap_or_default(),
            username,
            id,
            channel,
            device,
        })
    }).collect();

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

    PacketCode::List
    {
        online: users,
        offline,
    }
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
//STRUCTS
/// A token for opening an auxiliary connection
pub struct PendingToken
{
    pub owner_addr: SocketAddr,
    pub kind: ConnectionType,
    pub created: Instant,
}

//FUNCTIONS
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
pub(crate) fn send_to_accounts(participants: &Participants, packet: impl Fn(&str, u64) -> PacketCode) //NEVER BROADCAST PRIVATE UPDATES
{
    let entries: Vec<_> = CONNECTIONS.iter().filter_map(|entry|
    {
        let username = entry.username()?;
        entry.id()?;
        let id = participants.message_id(username)?;
        Some((entry.write_stream().clone(), entry.keys().cloned(), username.clone(), id))
    }).collect();

    for (stream, keys, username, id) in entries
    {
        let code = packet(&username, id);
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
/// - `except`: The peer address to skip.
pub fn send_to_all_but(code: PacketCode, except: Option<SocketAddr>) //SEND PACKET TO ALL CLIENTS, ONE EXCLUDED
{
    //COLLECT EACH CLIENT
    let entries: Vec<Connection> = CONNECTIONS.iter().filter_map(|entry|
    {
        match entry.value()
        {
            Connection::Authenticated { peer_addr, .. } if except != Some(*peer_addr) =>
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
    let mut connection =
    {
        let mut presence = PRESENCE.lock().unwrap();
        let Some((_, connection)) = CONNECTIONS.remove_if(peer_addr, |_, _|
        {
            PENDING_TOKENS.retain(|_, pending| pending.owner_addr != *peer_addr);
            true
        }) else { return; };

        presence.clients.remove(peer_addr);
        if let Some(&id) = connection.id()
        {
            presence.close_voice(*peer_addr, id, None);
        }

        if let (Some(username), Some(id)) = (connection.username(), connection.id()) && !user_connected(username)
        {
            //UNLIST SHARED FILES ON THE FINAL DISCONNECT
            AVAILABLE_FILES.remove(username);
            if config::users::online_status(username)
            {
                presence.broadcast(PacketCode::Leave
                {
                    username: username.clone(),
                    id: *id,
                    registered: config::users::contains(username),
                });
            }
        }
        connection
    };

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

    //REMOVE PM NON-PERSISTENT UPLOADS
    if let Some(files) = connection.private_files()
    {
        files.lock().unwrap().clear();
    }

    //SEND DISCONNECT CODE IF GRACEFUL
    if grace
    {
        network::send(&mut *connection.write_stream().lock().await, PacketCode::Disconnect, connection.keys()).await;
    }

    //CLOSE SCREEN UPLOAD STREAM
    if connection.remove_screen_stream()
    {
        //DEATTACH ALL ATTACHED CLIENTS
        deattach(*peer_addr, connection.username().unwrap()).await;
    }

    //AUTHENTICATED ACTIONS
    if connection.is_authenticated()
    {
        //TELL THE SHARER WE STOPPED WATCHING
        if let Some(target_addr) = connection.attached_screen().as_ref().map(|a| a.target_peer_addr)
        {
            notify(&target_addr, PacketCode::Deattached
            {
                username: connection.username().unwrap().to_owned(),
            }).await;
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

/// Returns whether a session with this username is connected (and authenticated).
fn user_connected(username: &str) -> bool //CHECK IF CLIENT WITH username IS CONNECTED
{
    CONNECTIONS.iter().any(|conn|
    {
        conn.is_authenticated() && conn.username().is_some_and(|u| u == username)
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

            Connection::Authenticated { write_stream, task, file_streams, shared_files, private_files, screen_stream, username, device, role,
                id, attached_screen, last_activity, last_attachment, last_typing, channel, seq, server_seq, peer_addr, alive,
                muted, credit, refill, throttles, .. } =>
            {
                Connection::Authenticated
                {
                    write_stream,
                    task,
                    file_streams,
                    shared_files,
                    private_files,
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
fn authenticate_client(peer_addr: &SocketAddr, username: &str, device: &Option<Device>, role: Role, id: usize) -> bool
{
    let mut presence = PRESENCE.lock().unwrap();
    let first_session = !user_connected(username);
    let Some(mut connection) = CONNECTIONS.get_mut(peer_addr) else { return false; };
    let old_connection = connection.value();

    //GIVE EACH ONLINE PERIOD ITS OWN DIRECTORY
    let shared_files = AVAILABLE_FILES.entry(username.to_string()).or_insert_with(|| Arc::new(SharedFiles
    {
        directory: misc::get_upload_dir(username).join(format!("{:032x}", rand::random::<u128>())),
        files: MutexSync::new(Vec::new()),
    })).clone();

    *connection = Connection::Authenticated
    {
        write_stream: old_connection.write_stream().clone(),
        task: old_connection.task().clone(),
        file_streams: Arc::new(MutexSync::new(HashMap::new())),
        shared_files,
        private_files: Arc::new(MutexSync::new(Vec::new())),
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
        spam_violations: 0,
        credit: config::read_config::<f32>("max_packet_burst"),
        refill: Instant::now(),
        throttles: 0,
        channel: None,
        seq: *old_connection.seq(),
        server_seq: 0,
        alive: true,
        muted: false,
    };
    drop(connection);

    if first_session && config::users::online_status(username)
    {
        presence.broadcast(PacketCode::Join
        {
            username: username.to_string(),
            username_color: config::users::colors(username).username_color,
            id,
            device: device.clone(),
            role,
        });
    }

    log::info!("Authenticate connection: {peer_addr} (id {id}, role {role})");
    true
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
            shared_files: old_connection.shared_files().unwrap().clone(),
            private_files: old_connection.private_files().unwrap().clone(),
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

/// Mints a one-time token for an auxiliary connection of client peer address.
fn open_connection(owner_addr: SocketAddr, kind: ConnectionType) -> [u8; 32] //ADD NEW TOKEN
{
    //GENERATE RANDOM TOKEN
    let mut token = [0u8; 32];
    SysRng.try_fill_bytes(&mut token).unwrap();

    //HOLD THE CONNECTION GUARD SO TEARDOWN CANNOT MISS THE NEW TOKEN
    if let Some(conn) = CONNECTIONS.get(&owner_addr) && conn.is_authenticated()
    {
        PENDING_TOKENS.insert(token, PendingToken { owner_addr, kind, created: Instant::now() });
    }

    token
}

//PUBLIC
/// Consumes an auxiliary token and captures the current main connection's task identity.
pub fn take_pending_token(token: &[u8; 32]) -> Option<(PendingToken, Id)>
{
    let owner_addr = PENDING_TOKENS.get(token)?.owner_addr;
    let conn = CONNECTIONS.get(&owner_addr)?;
    let (_, pending) = PENDING_TOKENS.remove(token)?;
    (conn.is_authenticated() && pending.created.elapsed() < Duration::from_secs(5))
        .then(|| (pending, conn.task().id()))
}

/// Sends a packet to the client with peer address `peer_addr`, if connected.
pub async fn notify(peer_addr: &SocketAddr, code: PacketCode) //SEND PACKET TO THE CLIENT WITH id
{
    //COLLECT INTO LOCALS - NO GUARD ACROSS THE await
    let target = CONNECTIONS.get(peer_addr).map(|conn| (conn.write_stream().clone(), conn.keys().cloned()));

    if let Some((write_stream, keys)) = target
    {
        network::send(&mut *write_stream.lock().await, code, keys.as_ref()).await;
    }
}

/// Broadcasts the server icon and acknowledges the change to the requesting connection.
pub async fn icon_changed(peer_addr: &SocketAddr) //TELL EVERYBODY THE SERVER'S PICTURE, AND id THAT IT WAS SAVED
{
    let hash = config::server_icon();

    send_to_all_but(PacketCode::ServerIcon { hash, save: false }, Some(*peer_addr));
    notify(peer_addr, PacketCode::ServerIcon { hash, save: true }).await;
}

/// Detaches every viewer from a screen share and tells them.
///
/// # Parameters
/// - `sharer_addr`: The sharer's main connection address.
/// - `sharer_uname`: The sharer's username, as shown to the viewers.
pub async fn deattach(sharer_addr: SocketAddr, sharer_uname: &str) //DEATTACH ALL ATTACHED CLIENTS
{
    let mut to_notify = Vec::new();

    for mut conn in CONNECTIONS.iter_mut()
        .filter(|conn| conn.attached_screen().as_ref().is_some_and(|a| a.target_peer_addr == sharer_addr))
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
                uname != options::get_server_username() && uname != "everyone"
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

    let Some(role) = config::users::add(username, &hash) else
    {
        log::warn!("Registration refused (username already registered): {peer_addr}");
        remove_connection(peer_addr, true, Some("registration")).await;
        return false;
    };

    if role == Role::Owner
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
    if !authenticate_client(&peer_addr, &username, &device, role, id) { return; }

    //SEND WHAT WAS SAID IN THE LOBBY BEFORE THIS
    send_history(&streams.1, &keys, None).await;
    send_private_history(&streams.1, &keys, &username, None).await; //OFC ONLY USER'S OWN PMS

    if config::read_config::<bool>("persistent_private_messages") &&
        config::read_config::<usize>("max_persistent_private_messages") > 0
    {
        //Snapshot under the same write lock used for live PM delivery.
        let mut stream = streams.1.lock().await;
        let (next, senders) = config::private_messages::unread(&username);
        network::send(&mut *stream, PacketCode::PrivateUnread { next, senders }, Some(&keys)).await;
    }

    //TELL CLIENT ALL REQUIRED DATA
    network::send(&mut *streams.1.lock().await, PacketCode::Accept
    {
        id,
        username: username.clone(),
        role,
        colors: config::users::colors(&username),
    }, Some(&keys)).await;

    //SEND LIST OF USERS
    send_list(&streams.1, &peer_addr).await;

    //TELL THE CLIENT WHO IS ALREADY IN VOICE
    if options::voice_chat_enabled()
    {
        PRESENCE.lock().unwrap().send_voice_roster(&peer_addr, id, &None);
    }

    //THE PACKET LOOP LIVES IN ITS OWN MODULE
    ClientSession::new(streams, peer_addr, obfuscation_key, keys, username, id, role).run().await;
}

/// Gracefully disconnects every client.
pub async fn disconnect_all(grace: bool) //DISCONNECT ALL CLIENTS
{
    //ITERATE OVER ALL ADDRESSES, REMOVE CONNECTIONS
    let addrs: Vec<SocketAddr> = CONNECTIONS.iter().map(|conn| *conn.peer_addr()).collect();

    log::info!("Disconnecting {} connections", addrs.len());
    for addr in &addrs
    {
        remove_connection(addr, grace, None).await; //REMOVE GRACEFULLY
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
