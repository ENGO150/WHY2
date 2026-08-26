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

use std::
{
    path::PathBuf,
    future::Future,
    net::{ IpAddr, SocketAddr },
    time::{ Instant, Duration },
    collections::{ HashSet, HashMap },
    sync::
    {
        Arc,
        LazyLock,
        Mutex as MutexSync,
        atomic::{ AtomicUsize, Ordering },
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

use crate::
{
    config,
    options,
    misc,
    crypto::{ kex, password },
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
        codes::
        {
            PacketCode,
            OnlineUser,
            UserFile,
            UserScreen,
        },
    },
};

//STRUCTS
#[derive(Clone)]
pub struct AvailableFile //UPLOADED FILE
{
    pub hash: [u8; 32],   //FILE HASH
    pub path: PathBuf,    //PATH
    pub filename: String, //FILENAME
    pub size: u64,        //FILE SIZE
    pub key: Zeroizing<Vec<i64>>,
    pub nonce: Vec<i64>,
}

#[derive(Clone)]
pub struct Attach //SCREEN ATTACHMENT
{
    pub stream: Arc<Mutex<OwnedWriteHalf>>, //RECEIVE STREAM
    pub target_id: usize,                   //ID OF SCREENSHARER
    pub token: [u8; 32],                    //TOKEN FOR REXSTREAM
}

pub struct HandshakeSlot //RESERVATION HELD BY A SOCKET THAT HAS NOT IDENTIFIED ITSELF YET (SEE HANDSHAKE BUDGET)
{
    ip: IpAddr, //PEER THE SLOT WAS TAKEN FOR
}

//ENUMS
pub enum ConnectionType //TYPES OF TCP CHANNEL
{
    FileUpload
    {
        uid: u64,
    },
    FileDownload
    {
        uid: u64,
        file: AvailableFile,
    },
    Screen,
    Attach
    {
        id: usize,
    },
}

#[derive(Clone)]
pub enum Connection //CLIENT CONNECTION (WHAT IS PUSHED TO connections LIST)
{
    Authenticated
    {
        write_stream: Arc<Mutex<OwnedWriteHalf>>,                    //STREAM
        task: AbortHandle,                                           //HANDLER TASK (USED TO FORCE-CLOSE THE CONNECTION)
        file_streams: Arc<MutexSync<HashMap<u64, AbortHandle>>>,     //ACTIVE FILE TRANSFER TASKS
        screen_stream: Option<AbortHandle>,                          //SCREEN UPLOAD TASK
        peer_addr: SocketAddr,                                       //ADDRESS & PORT
        username: String,                                            //USERNAME
        id: usize,                                                   //ID OF USER
        keys: SharedKeys,                                            //SHARED KEYS BETWEEN SERVER AND CLIENT (one to one)
        attached_screen: Option<Attach>,                             //SCREEN DOWNLOAD STREAM & TARGET ID
        last_activity: Instant,                                      //TIME OF LAST MESSAGE (USED FOR TIMEOUT)
        last_key_exchange: Instant,                                  //TIME OF LAST REKEY
        spam_violations: usize,                                      //SPAM VIOLATIONS (unexpected, huh?)
        channel: Option<String>,                                     //CHANNEL
        seq: usize,                                                  //SEQUENCE NUMBER (CLIENT -> SERVER)
        server_seq: usize,                                           //SEQUENCE NUMBER (SERVER -> CLIENT)
        alive: bool,                                                 //RESPONDED TO KEEPALIVE
    },

    NonAuthenticated
    {
        write_stream: Arc<Mutex<OwnedWriteHalf>>, //STREAM
        task: AbortHandle,                        //HANDLER TASK
        peer_addr: SocketAddr,                    //ADDRESS & PORT
        username: Option<String>,                 //CHOSEN USERNAME
        keys: Option<SharedKeys>,                 //SHARED KEYS
        obfuscation_key: [u8; 32],                //OBFUSCATION KEY FROM PLAIN PACKETS
        last_activity: Instant,                   //TIME OF LAST MESSAGE
        seq: usize,                               //SEQUENCE NUMBER
        connect: Instant,                         //TIME OF CONNECTION
    },
}

//IMPLEMENTATIONS
impl Connection
{
    //GET STREAM FROM Connection
    pub fn write_stream(&self) -> &Arc<Mutex<OwnedWriteHalf>>
    {
        match self
        {
            Self::Authenticated { write_stream, .. } => write_stream,
            Self::NonAuthenticated { write_stream, .. } => write_stream,
        }
    }

    //GET HANDLER TASK FROM Connection
    pub fn task(&self) -> &AbortHandle
    {
        match self
        {
            Self::Authenticated { task, .. } => task,
            Self::NonAuthenticated { task, .. } => task,
        }
    }

    //GET ALL ACTIVE FILE STREAMS
    pub fn file_streams(&self) -> Option<&Arc<MutexSync<HashMap<u64, AbortHandle>>>>
    {
        match self
        {
            Self::Authenticated { file_streams, .. } => Some(file_streams),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET PEER ADDR FROM Connection#stream
    pub fn peer_addr(&self) -> &SocketAddr
    {
        match self
        {
            Self::Authenticated { peer_addr, .. } => peer_addr,
            Self::NonAuthenticated { peer_addr, .. } => peer_addr,
        }
    }

    //GET USERNAME FROM Connection
    pub fn username(&self) -> Option<&String>
    {
        match self
        {
            Self::Authenticated { username, .. } => Some(username),
            Self::NonAuthenticated { username, .. } => username.as_ref(),
        }
    }

    //GET USERNAME FROM Connection AS MUTABLE
    fn username_mut(&mut self) -> &mut Option<String>
    {
        match self
        {
            Self::Authenticated { .. } => panic!("Do not use username_mut() on Authenticated client"),
            Self::NonAuthenticated { username, .. } => username,
        }
    }

    //GET ID FROM Connection
    pub fn id(&self) -> Option<&usize>
    {
        match self
        {
            Self::Authenticated { id, .. } => Some(id),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET SHARED KEYS FROM Connection
    pub fn keys(&self) -> Option<&SharedKeys>
    {
        match self
        {
            Self::Authenticated { keys, .. } => Some(keys),
            Self::NonAuthenticated { keys, .. } => keys.as_ref(),
        }
    }

    //GET OBFUSCATION KEY
    pub fn obfuscation_key(&self) -> Option<&[u8; 32]>
    {
        match self
        {
            Self::Authenticated { .. } => None,
            Self::NonAuthenticated { obfuscation_key, .. } => Some(obfuscation_key),
        }
    }

    //GET LAST ACTIVITY FROM Connection
    pub fn last_activity(&self) -> &Instant
    {
        match self
        {
            Self::Authenticated { last_activity, .. } => last_activity,
            Self::NonAuthenticated { last_activity, .. } => last_activity,
        }
    }

    //GET LAST ACTIVITY FROM Connection AS MUTABLE
    pub fn last_activity_mut(&mut self) -> &mut Instant
    {
        match self
        {
            Self::Authenticated { last_activity, .. } => last_activity,
            Self::NonAuthenticated { last_activity, .. } => last_activity,
        }
    }

    //GET LAST KEY EXCHANGE FROM Connection
    pub fn last_key_exchange(&self) -> Option<&Instant>
    {
        match self
        {
            Self::Authenticated { last_key_exchange, .. } => Some(last_key_exchange),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET SPAM VIOLATIONS FROM Connection
    pub fn spam_violations(&self) -> Option<&usize>
    {
        match self
        {
            Self::Authenticated { spam_violations, .. } => Some(spam_violations),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET SPAM VIOLATIONS FROM Connection AS MUTABLE
    pub fn spam_violations_mut(&mut self) -> Option<&mut usize>
    {
        match self
        {
            Self::Authenticated { spam_violations, .. } => Some(spam_violations),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET CHANNEL
    pub fn channel(&self) -> &Option<String>
    {
        match self
        {
            Self::Authenticated { channel, .. } => channel,
            Self::NonAuthenticated { .. }  => &None,
        }
    }

    //GET LAST SEQUENCE NUMBER
    pub fn seq(&self) -> &usize
    {
        match self
        {
            Self::Authenticated { seq, .. } => seq,
            Self::NonAuthenticated { seq, .. } => seq,
        }
    }

    //GET LAST SEQUENCE NUMBER AS MUTABLE
    pub fn seq_mut(&mut self) -> &mut usize
    {
        match self
        {
            Self::Authenticated { seq, .. } => seq,
            Self::NonAuthenticated { seq, .. } => seq,
        }
    }

    //GET LAST SERVER SEQUENCE NUMBER
    pub fn server_seq(&self) -> Option<&usize>
    {
        match self
        {
            Self::Authenticated { server_seq, .. } => Some(server_seq),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET LAST SERVER SEQUENCE NUMBER AS MUTABLE
    pub fn server_seq_mut(&mut self) -> Option<&mut usize>
    {
        match self
        {
            Self::Authenticated { server_seq, .. } => Some(server_seq),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //CHECK IF CONNECTION IS INACTIVE
    fn is_inactive(&self, now: Option<Instant>) -> bool
    {
        match self
        {
            Self::Authenticated { last_activity, .. } =>
            {
                now.unwrap_or_else(Instant::now).duration_since(*last_activity) >
                    Duration::from_secs(config::read_config::<u64>("communication_time"))
            },

            Self::NonAuthenticated { connect, .. } =>
            {
                now.unwrap_or_else(Instant::now).duration_since(*connect) >
                    Duration::from_secs(config::read_config::<u64>("max_auth_time"))
            },
        }
    }

    //IS AUTHENTICATED
    pub fn is_authenticated(&self) -> bool
    {
        match self
        {
            Self::Authenticated { .. } => true,
            Self::NonAuthenticated { .. } => false,
        }
    }

    //SET CONNECTION TO ALIVE
    pub fn set_alive(&mut self, val: bool)
    {
        match self
        {
            Self::Authenticated { alive, .. } => *alive = val,
            _ => {}
        }
    }

    //CHECK IF CONNECTION IS ALIVE
    pub fn is_alive(&self) -> &bool
    {
        match self
        {
            Self::Authenticated { alive, .. } => alive,
            Self::NonAuthenticated { .. } => &false,
        }
    }

    //ADD FILE STREAM
    pub fn add_file_stream(&self, uid: u64, task: AbortHandle)
    {
        if let Self::Authenticated { file_streams, .. } = self
        {
            file_streams.lock().unwrap().insert(uid, task);
        }
    }

    //REMOVE FILE STREAM
    pub fn remove_file_stream(&self, uid: u64)
    {
        if let Self::Authenticated { file_streams, .. } = self
        {
            if let Some(task) = file_streams.lock().unwrap().remove(&uid)
            {
                task.abort(); //DROPS THE STREAM HALVES, CLOSING THE SOCKET
            }
        }
    }

    //GET SCREEN UPLOAD STREAM
    pub fn screen_stream(&self) -> &Option<AbortHandle>
    {
        match self
        {
            Self::Authenticated { screen_stream, .. } => screen_stream,
            Self::NonAuthenticated { .. } => &None,
        }
    }

    //GET ATTACHED SCREENSHARE
    pub fn attached_screen(&self) -> &Option<Attach>
    {
        match self
        {
            Self::Authenticated { attached_screen, .. } => attached_screen,
            Self::NonAuthenticated { .. } => &None,
        }
    }

    //SET ATTACHED SCREENSHARE
    pub fn attach_screen(&mut self, target_id: usize, stream: Arc<Mutex<OwnedWriteHalf>>, token: [u8; 32])
    {
        match self
        {
            Self::Authenticated { attached_screen, .. } => *attached_screen = Some(Attach
            {
                target_id,
                stream,
                token,
            }),
            _ => {},
        }
    }

    //UNSET ATTACHED SCREENSHARE
    pub fn deattach_screen(&mut self)
    {
        match self
        {
            Self::Authenticated { attached_screen, .. } =>
            {
                *attached_screen = None;
                log::info!("Stop screen attach: {}", self.peer_addr());
            },
            _ => {},
        }
    }

    //ADD SCREEN UPLOAD STREAM
    pub fn set_screen_stream(&mut self, task: AbortHandle)
    {
        //CLEAN OLD STREAM
        self.remove_screen_stream();

        if let Self::Authenticated { screen_stream, .. } = self
        {
            *screen_stream = Some(task);
        }
    }

    //TAKE SCREEN UPLOAD STREAM WITHOUT ABORTING IT (FOR THE SHARE TASK TEARING ITSELF DOWN)
    pub fn take_screen_stream(&mut self) -> Option<(usize, AbortHandle)>
    {
        if let Self::Authenticated { screen_stream, peer_addr, id, .. } = self
        {
            if let Some(old_task) = screen_stream.take()
            {
                log::info!("Stop screenshare: {}", peer_addr);

                return Some((*id, old_task));
            }
        }

        None
    }

    //REMOVE SCREEN UPLOAD STREAM
    pub fn remove_screen_stream(&mut self) -> Option<usize>
    {
        self.take_screen_stream().map(|(id, old_task)|
        {
            old_task.abort();
            id
        })
    }
}

impl HandshakeSlot
{
    //TAKE A SLOT FOR A FRESHLY ACCEPTED SOCKET, None IF THE BUDGET IS FULL
    pub fn reserve(ip: IpAddr) -> Option<Self>
    {
        if HANDSHAKES.load(Ordering::Relaxed) >= *MAX_HANDSHAKES { return None; }

        //PER-IP SO ONE PEER CANNOT TAKE THE WHOLE BUDGET
        {
            let mut slots = HANDSHAKES_PER_IP.entry(ip).or_insert(0);
            if *slots >= consts::MAX_HANDSHAKES_PER_IP { return None; }

            *slots += 1;
        }

        HANDSHAKES.fetch_add(1, Ordering::Relaxed);

        Some(Self { ip })
    }
}

impl Drop for HandshakeSlot
{
    //RELEASE THE SLOT, WHICHEVER WAY THE HANDSHAKE ENDED
    fn drop(&mut self)
    {
        HANDSHAKES.fetch_sub(1, Ordering::Relaxed);

        //DROP THE GUARD BEFORE REMOVING - BOTH TOUCH THE SAME SHARD
        let empty = if let Some(mut slots) = HANDSHAKES_PER_IP.get_mut(&self.ip)
        {
            *slots = slots.saturating_sub(1);
            *slots == 0
        } else { false };

        if empty { HANDSHAKES_PER_IP.remove_if(&self.ip, |_, slots| *slots == 0); }
    }
}

//LISTS
pub static PENDING_TOKENS: LazyLock<DashMap<[u8; 32], (usize, ConnectionType, Instant)>> = LazyLock::new(|| DashMap::new());
pub static CONNECTIONS: LazyLock<DashMap<SocketAddr, Connection>> = LazyLock::new(|| DashMap::new());     //LIST FOR EACH CLIENT CONNECTION
pub static AVAILABLE_FILES: LazyLock<DashMap<String, Vec<AvailableFile>>> = LazyLock::new(|| DashMap::new()); //LIST FOR UPLOADED FILES

//HANDSHAKE BUDGET
static MAX_HANDSHAKES: LazyLock<usize> = LazyLock::new(||
{
    (config::read_config::<usize>("max_clients") + config::read_config::<usize>("max_unauth_clients")) * consts::MAX_HANDSHAKES_PER_IP
});
static HANDSHAKES: AtomicUsize = AtomicUsize::new(0);
static HANDSHAKES_PER_IP: LazyLock<DashMap<IpAddr, usize>> = LazyLock::new(|| DashMap::new());

//PRIVATE
async fn untrusted_read<F>(streams: &mut Streams<'_>, is_match: F, keys: Option<&SharedKeys>) -> Option<PacketCode>
where
    F: Fn(&PacketCode) -> bool
{
    let mut invalid_packets = 0; //INVALID KEY EXCHANGE PACKETS COUNTER

    //WAIT FOR KeyExchange
    let message = loop
    {
        //READ MESSAGE (WITH TIMEOUT FOR ZOMBIE CONNECTIONS)
        let received = match tokio::time::timeout(Duration::from_millis(2000), network::receive(streams, keys, None)).await
        {
            Ok(Some(r)) => r,
            _ => return None
        };

        if is_match(&received) { break received; }

        //CHECK INVALID PACKETS COUNTER
        if invalid_packets == 3 { return None; }
        invalid_packets += 1; //INCREMENT
    };

    Some(message)
}

async fn key_exchange //KEY EXCHANGE FOR SERVER-SIDE
(
    streams: &mut Streams<'_>,
    peer_addr: &SocketAddr,
    keys: &mut SharedKeys,
    rekey_trigger: Option<&SharedKeys>,
)
{
    //LOAD KEYS
    let (sk, pk) = kex::get_server_keys();          //ECC
    let (pq_sk, pq_pk) = kex::get_server_pq_keys(); //PQ (ML-KEM)

    //ATOMIC SEND
    {
        let mut write = streams.1.lock().await;

        //TRIGGER REKEY
        let keys = if let Some(current_keys) = rekey_trigger
        {
            network::send(&mut write, PacketCode::Rekey, Some(current_keys)).await;

            //ENCRYPT PUBKEYS
            Some(current_keys)
        } else { None }; //OBFUSCATE PUBKEYS

        //SEND ENCRYPTED PUBKEYS TO CLIENT
        network::send(&mut write, PacketCode::KeyExchange { ecc: pk, pq: pq_pk }, keys).await;
    }

    //READ FROM UNTRUSTED CLIENT
    let message = match untrusted_read(streams, |code| matches!(code, PacketCode::KeyExchange { .. }), rekey_trigger).await
    {
        Some(r) => r,
        None => return
    };

    //DERIVE SHARED KEYS
    let new_keys = (||
    {
        if let PacketCode::KeyExchange { ecc, pq } = message
        {
            //DECAPSULATE PQ
            let pq_secret = kex::decapsulate_pq(&pq_sk, &pq)?;

            //DERIVE KEYS
            kex::derive_shared_secret(sk, ecc, pq_secret)
        } else { unreachable!("what"); }
    })();

    //UPDATE CLIENT KEYS
    if let Some(new_keys) = new_keys
    {
        update_client_keys(peer_addr, &new_keys);
        *keys = new_keys;
    }
}

async fn send_welcome_packet(write_stream: &mut OwnedWriteHalf, keys: &SharedKeys) //send welcome packet you idiot
{
    //SEND
    network::send(write_stream, PacketCode::Welcome
    {
        min_pass: config::read_config::<u64>("min_password_length"),
        max_uname: config::read_config::<u64>("max_username_length"),
        min_uname: config::read_config::<u64>("min_username_length"),
        server_name: config::read_config::<String>("server_name"),
        server_uname: options::get_server_username(),
        git_hash: env!("WHY2_GIT_HASH").to_owned(),
    }, Some(keys)).await;
}

//PUBLIC
pub fn spawn_with_abort<F, Fut>(f: F) -> AbortHandle //SPAWN TASK WHICH KNOWS ITS OWN AbortHandle
where
    F: FnOnce(AbortHandle) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    //CHANNEL FOR HANDING THE HANDLE OVER TO THE TASK
    let (tx, rx) = oneshot::channel::<AbortHandle>();

    let handle = tokio::spawn(async move
    {
        //WAIT FOR OWN HANDLE (PREVENTS RACE WITH REGISTRATION)
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

pub fn send_to_all(code: PacketCode, filter_channel: bool, channel: Option<&str>) //SEND PACKET TO ALL CLIENTS
{
    //COLLECT EACH CLIENT IN SAME CHANNEL
    let entries: Vec<Connection> = CONNECTIONS.iter().filter_map(|entry|
    {
        match entry.value()
        {
            Connection::Authenticated { channel: c, .. } if !filter_channel || c.as_deref() == channel =>
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
        //DISCONNECT FROM VOICE CHAT
        if options::voice_chat_enabled()
        {
            voice_server::remove_connection(connection.id().unwrap());
        }

        //REMOVE UPLOADS
        let username = connection.username().unwrap();
        let _ = fs::remove_dir_all(misc::get_upload_dir(username)).await; //REMOVE FILES
        file::ACTIVE_FILESHARES.retain(|_, u| u.client_id != *connection.id().unwrap());
        AVAILABLE_FILES.remove(username); //REMOVE AVAILABLE FILES

        //SEND LEAVE MESSAGE
        send_to_all(PacketCode::Leave
        {
            username: connection.username().unwrap().to_string(),
            id: *connection.id().unwrap(),
        }, false, None);
    }

    log::info!
    (
        "Close connection{}: {peer_addr}",

        if let Some(info) = info
        {
            format!(" ({info})")
        } else { String::new() }
    );

    //SHUT DOWN THE HANDLER TASK (MUST BE LAST - THIS MAY BE THE CALLING TASK ITSELF)
    connection.task().abort();
}

fn user_connected(username: &str) -> bool //CHECK IF CLIENT WITH username IS CONNECTED
{
    CONNECTIONS.iter().any(|conn|
    {
        conn.username().map_or(false, |u| u == &username.to_string())
    })
}

fn get_latest_id() -> usize
{
    //GET HashSet OF IDS
    let ids: HashSet<usize> = CONNECTIONS.iter().filter_map(|conn|
    {
        if let Some(id) = conn.id()
        {
            Some(*id)
        } else
        {
            None
        }
    }).collect();

    //GET SMALLEST UNUSED ID
    for i in 0..
    {
        if !ids.contains(&i) //ID FOUND, RETURN
        {
            return i;
        }
    }

    unreachable!("what the fuck");
}

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

            Connection::Authenticated { write_stream, task, file_streams, screen_stream, username, id, attached_screen, last_activity, channel,
                seq, server_seq, peer_addr, alive, .. } =>
            {
                Connection::Authenticated
                {
                    write_stream,
                    task,
                    file_streams,
                    screen_stream,
                    peer_addr,
                    username,
                    id,
                    keys: keys.to_owned(),
                    attached_screen,
                    last_activity,
                    last_key_exchange: Instant::now(),
                    spam_violations: 0,
                    channel,
                    seq,
                    server_seq,
                    alive,
                }
            }
        }
    });
}

fn authenticate_client(peer_addr: &SocketAddr, username: &str, id: usize) //MOVE CONNECTION FROM NonAuthenticated TO Authenticated
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
            id: id,
            keys: old_connection.keys().unwrap().to_owned(),
            attached_screen: None,
            last_activity: Instant::now() - Duration::from_millis(config::read_config("min_message_delay")),
            last_key_exchange: old_connection.last_key_exchange().copied().unwrap_or_else(Instant::now),
            spam_violations: 0,
            channel: None,
            seq: *old_connection.seq(),
            server_seq: 0,
            alive: true,
        }
    });

    //CREATE AVAILABLE FILES ENTRY
    AVAILABLE_FILES.insert(username.to_string(), Vec::new());

    log::info!("Authenticate connection: {}", peer_addr);
}

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
            id: *old_connection.id().unwrap(),
            keys: old_connection.keys().unwrap().to_owned(),
            attached_screen: old_connection.attached_screen().clone(),
            last_activity: Instant::now(),
            last_key_exchange: *old_connection.last_key_exchange().unwrap(),
            spam_violations: *old_connection.spam_violations().unwrap(),
            channel: channel.clone(),
            seq: *old_connection.seq(),
            server_seq: *old_connection.server_seq().unwrap(),
            alive: true,
        }
    });

    //RETURN IF CLIENT SWITCHED TO SAME CHANNEL
    if old_channel == *channel { return; }

    //CHECK IF CHANNEL WAS ABANDONED
    if let Some(old_channel) = old_channel
    {
        if !CONNECTIONS.iter().any(|c| c.channel().as_ref() == Some(&old_channel))
        {
            //NO CLIENT IS IN OLD CHANNEL
            send_to_all(PacketCode::ChannelDestroyed
            {
                name: old_channel,
            }, false, None);
        }
    }

    //CHECK IF CHANNEL WAS CREATED
    if let Some(channel) = channel
    {
        if CONNECTIONS.iter().filter(|c| c.channel().as_ref() == Some(channel)).count() == 1
        {
            //CLIENT IS FIRST IN CHANNEL
            send_to_all(PacketCode::ChannelCreated
            {
                name: channel.clone(),
            }, false, None);
        }
    }
}

async fn ask_version(streams: &mut Streams<'_>, keys: &SharedKeys) -> Option<String> //ASK CLIENT FOR VERSION
{
    //ASK FOR VERSION
    network::send(&mut *streams.1.lock().await,
        PacketCode::Version { version: Some(misc::get_version().to_string()) }, Some(keys)).await;

    //READ FROM UNTRUSTED CLIENT
    let read = untrusted_read(streams, |code| matches!(code, PacketCode::Version { .. }), Some(keys)).await?;

    if let PacketCode::Version { version } = read
    {
        return version;
    } { unreachable!("what"); }
}

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
            PacketCode::Deattach { username: Some(sharer_uname.to_owned()) }, keys.as_ref()).await;
    }
}

pub async fn listen_client //CLIENT -> SERVER COMMUNICATION
(
    streams: &mut Streams<'_>,
    peer_addr: SocketAddr,
    obfuscation_key: [u8; 32],
    task: AbortHandle,
)
{
    log::info!("New connection: {peer_addr}");

    //PUSH NEW CONNECTION
    CONNECTIONS.insert(peer_addr, Connection::NonAuthenticated
    {
        write_stream: streams.1.clone(),
        task,
        peer_addr: peer_addr,
        username: None,
        keys: None,
        obfuscation_key,
        last_activity: Instant::now(),
        seq: 0,
        connect: Instant::now(),
    });

    //GET ENCRYPTION & MAC KEYS
    let mut keys = (Zeroizing::new(vec![]), Zeroizing::new(vec![]));
    key_exchange(streams, &peer_addr, &mut keys, None).await;

    //CHECK FOR VALID KEYS
    if keys.0.is_empty() || keys.1.is_empty()
    {
        return remove_connection(&peer_addr, false, None).await
    }

    //ASK CLIENT FOR THEIR PACKAGE VERSION
    if config::read_config("check_client_version")
    {
        let version = ask_version(streams, &keys).await;
        if version.is_none() || version != Some(misc::get_version().to_string())
        {
            return remove_connection(&peer_addr, true, Some("version")).await;
        }
    }

    //SEND PACKET WITH REQUIRED SERVER INFO
    send_welcome_packet(&mut *streams.1.lock().await, &keys).await;

    //GET USERNAME FROM USER
    let mut username: Option<String> = None; //USER ENTERED USERNAME

    //USERNAME CONFIGS
    let max_tries = config::read_config::<usize>("max_auth_tries"); //MAX n
    let min_len = config::read_config::<usize>("min_username_length");
    let max_len = config::read_config::<usize>("max_username_length");

    //TELL USER IF REGISTRATIONS ARE DISABLED
    let disabled_registration = !config::read_config::<bool>("allow_register");
    if disabled_registration
    {
        network::send(&mut *streams.1.lock().await, PacketCode::RegisterDisabled, Some(&keys)).await;
    }

    //ASK n TIMES
    for _ in 0..max_tries
    {
        //SEND PICK_USERNAME CODE
        network::send(&mut *streams.1.lock().await, PacketCode::Username { username: None }, Some(&keys)).await;

        match network::receive(streams, Some(&keys), None).await
        {
            //USERNAME CONDITIONS MET, BREAK LOOP
            Some(PacketCode::Username { username: uname }) =>
            {
                if let Some(uname) = uname
                {
                    if uname.len() >= min_len && uname.len() <= max_len &&
                        uname.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') &&
                        !user_connected(&uname) && uname != options::get_server_username()
                    {
                        username = Some(uname);
                        break;
                    }
                }
            },

            _ => return remove_connection(&peer_addr, false, Some("username")).await,
        }
    }

    //NO USERNAME RECEIVED, DISCONNECT CLIENT
    if username.is_none()
    {
        return remove_connection(&peer_addr, true, Some("username")).await;
    }

    let username = username.unwrap();

    //UPDATE USERNAME IN NonAuthenticated
    if let Some(mut conn) = CONNECTIONS.get_mut(&peer_addr)
    {
        if !conn.is_authenticated()
        {
            //UPDATE
            *conn.username_mut() = Some(username.clone());
        }
    }

    let user_exists = config::server_users_contains(&username);

    //ASK FOR PASSWORD
    if !user_exists && !disabled_registration //REGISTRATION (OR "FAKE" LOGIN ON DISABLED REGISTER)
    {
        let max_tries = config::read_config::<usize>("max_auth_tries"); //MAX n
        let mut password: Option<Zeroizing<String>> = None;

        //KEEP ASKING FOR PASSWORD n TIMES
        for _ in 0..max_tries
        {
            //SEND REGISTER CODE
            network::send(&mut *streams.1.lock().await, PacketCode::PasswordR { password: None }, Some(&keys)).await;

            //WAIT FOR ANSWER
            match network::receive(streams, Some(&keys), None).await
            {
                Some(PacketCode::PasswordR { password: pass }) =>
                {
                    if let Some(pass) = pass
                    {
                        let pass = Zeroizing::new(pass);
                        //CHECK LENGTH
                        if pass.len() >= config::read_config("min_password_length")
                        {
                            password = Some(pass);
                            break;
                        }
                    }
                },

                _ => return remove_connection(&peer_addr, false, Some("register")).await
            };
        }

        if password.is_none()
        {
            return remove_connection(&peer_addr, true, Some("register")).await;
        }

        //HASH PASSWORD (ARGON2 IS CPU HEAVY, KEEP IT OFF THE RUNTIME)
        let hash = task::spawn_blocking(move || password::hash_password(password.as_ref().unwrap().as_str()))
            .await.expect("Hashing password failed");

        //SAVE PASSWORD
        if config::server_users_add(&username, &hash)
        {
            //FIRST USER, NOTIFY ABOUT OWNER ROLE
            network::send(&mut *streams.1.lock().await, PacketCode::FirstUser, Some(&keys)).await;
        }
    } else //LOGIN
    {
        //SEND LOGIN CODE
        network::send(&mut *streams.1.lock().await, PacketCode::PasswordL { password: None }, Some(&keys)).await;

        //WAIT FOR ANSWER
        let password = loop
        {
            match network::receive(streams, Some(&keys), None).await
            {
                Some(PacketCode::PasswordL { password: Some(password) }) => break Zeroizing::new(password),

                _ => return remove_connection(&peer_addr, false, Some("login")).await,
            }
        };

        //VERIFY PASSWORD (ARGON2 IS CPU HEAVY, KEEP IT OFF THE RUNTIME)
        let valid = if password.is_empty()
        {
            false
        } else if let Some(hashed) = config::server_users_password(&username)
        {
            task::spawn_blocking(move || password::compare_password_hash(&hashed, &password))
                .await.expect("Comparing password failed")
        } else //UNKNOWN USER (OR FAKE LOGIN)
        {
            false
        };

        //INVALID PASSWORD (OR FAKE LOGIN), DISCONNECT CLIENT
        if !valid
        {
            return remove_connection(&peer_addr, true, Some("login")).await;
        }
    }

    //GENERATE ID FOR CLIENT
    let id = get_latest_id();

    let mut channel: Option<String> = None; //CURRENT CLIENT CHANNEL

    //AUTHENTICATE CLIENT
    authenticate_client(&peer_addr, &username, id);

    let role = config::server_users_role(&username).unwrap(); //WHAT THIS CLIENT IS ALLOWED TO ASK FOR

    //TELL CLIENT TO START CHATTING
    network::send(&mut *streams.1.lock().await, PacketCode::Accept { id, role }, Some(&keys)).await;

    //SEND JOIN MESSAGE
    send_to_all(PacketCode::Join { username: username.clone() }, false, None);

    //LOOP READING
    loop
    {
        //READ
        let read = match network::receive(streams, Some(&keys), None).await
        {
            Some(r) => r,
            None => return
        };

        //REKEY EVERY 10 MINUTES
        if Instant::now().duration_since(*CONNECTIONS.get(&peer_addr).unwrap().last_key_exchange().unwrap()) >=
            Duration::from_secs(consts::REKEY_INTERVAL) &&
            !file::ACTIVE_FILESHARES.iter().any(|entry| entry.client_id == id) //DO NOT REKEY ON FILE UPLOAD
        {
            //INFORM CLIENT ABOUT REKEYING
            let current_keys = keys.clone();
            key_exchange(streams, &peer_addr, &mut keys, Some(&current_keys)).await; //INIT REKEY
        }

        //CLIENT CODES
        match read
        {
            //MESSAGE
            PacketCode::Message { text, colors, .. } =>
            {
                //SEND MESSAGE TO ALL USERS
                send_to_all(PacketCode::Message
                {
                    text: text.trim().to_owned(),
                    username: Some(username.clone()),
                    id: Some(id),
                    colors,
                }, true, channel.as_deref());
            }

            //CLIENT QUITS
            PacketCode::Disconnect =>
            {
                //DISCONNECT CLIENT
                return remove_connection(&peer_addr, true, None).await;
            },

            //VOICE CALL
            PacketCode::Voice { .. } =>
            {
                //CHECK DISABLED FEATURE
                if !options::voice_chat_enabled()
                {
                    network::send(&mut *streams.1.lock().await, PacketCode::InvalidFeature, Some(&keys)).await;
                } else if !voice_server::CONNECTIONS.contains_key(&id) //IS NOT USING VOICE
                {
                    //OPEN THE VOICE SLOT AND ACKNOWLEDGE WITH THE TOKEN THAT CLAIMS IT OVER UDP
                    let token = voice_server::open_connection(id, username.clone());
                    network::send(&mut *streams.1.lock().await, PacketCode::Voice { token: Some(token) }, Some(&keys)).await;

                    //SEND CODE TO CHANNEL
                    send_to_all(PacketCode::ChannelJoin { username: username.clone(), id }, true, channel.as_deref());

                    //SEND CONNECTED CLIENTS
                    send_voice_clients(&mut *streams.1.lock().await, &keys, id).await;
                } else //IS USING VOICE
                {
                    //ACKNOWLEDGE THE LEAVE
                    network::send(&mut *streams.1.lock().await, PacketCode::Voice { token: None }, Some(&keys)).await;

                    //SEND CODE TO LAST CHANNEL
                    send_to_all(PacketCode::ChannelLeave { id }, true, channel.as_deref());

                    //REMOVE FROM VOICE
                    voice_server::remove_connection(&id);
                }
            },

            //SWITCH CHANNEL
            PacketCode::Channel { channel: tchannel } =>
            {
                //CHECK PARAMETER VALIDITY
                if tchannel.iter().all(|s| !s.is_empty() && s.len() <= config::read_config("max_channel_length") && s.chars().all(|c| c.is_ascii_alphanumeric() && c != ' '))
                {
                    //SEND ChannelLeave CODE TO OLD CHANNEL
                    if options::voice_chat_enabled()
                    {
                        send_to_all(PacketCode::ChannelLeave { id }, true, channel.as_deref());
                    }

                    //UPDATE CHANNEL
                    update_client_channel(&peer_addr, &tchannel);
                    channel = tchannel.clone();
                    network::send(&mut *streams.1.lock().await, PacketCode::Channel { channel: tchannel }, Some(&keys)).await;

                    //SEND CODE TO CHANNEL
                    if options::voice_chat_enabled() && voice_server::CONNECTIONS.contains_key(&id)
                    {
                        send_to_all(PacketCode::ChannelJoin { username: username.clone(), id }, true, channel.as_deref());
                    }

                    //SEND CONNECTED CLIENTS
                    send_voice_clients(&mut *streams.1.lock().await, &keys, id).await;
                } else //INVALID CHANNEL
                {
                    //SEND InvalidUsage CODE
                    network::send(&mut *streams.1.lock().await, PacketCode::InvalidUsage, Some(&keys)).await;
                }
            },

            //CLIENT REQUESTED LIST OF ONLINE USERS
            PacketCode::List { .. } =>
            {
                let mut users = Vec::new();

                //ITERATE OVER CONNECTIONS, CREATE JSON OF USERS
                for connection_enum in CONNECTIONS.iter()
                {
                    if let Connection::Authenticated { username: uname, id: user_id, channel, .. } = connection_enum.value()
                    {
                        users.push(OnlineUser
                        {
                            username: uname.clone(),
                            id: *user_id,
                            channel: channel.clone(),
                        });
                    }
                }

                //SEND LIST BACK TO CLIENT
                network::send(&mut *streams.1.lock().await, PacketCode::List { users: Some(users) }, Some(&keys)).await;
            },

            //NEW FILE UPLOAD
            PacketCode::Upload { hash, .. } =>
            {
                //PREVENT TOKEN SPAM
                let active_count = file::ACTIVE_FILESHARES.iter().filter(|u| u.client_id == id).count();
                if active_count >= config::read_config::<usize>("max_client_parallel_uploads")
                {
                    network::send(&mut *streams.1.lock().await, PacketCode::UploadLimit, Some(&keys)).await;
                    continue;
                }

                //GENERATE RANDOM UID
                let uid = rand::random::<u64>();
                let token = open_connection(id, ConnectionType::FileUpload { uid });

                //LOG FILE UPLOAD
                log::info!("Upload request: {peer_addr}");

                //SEND APPROVAL TO CLIENT
                network::send(&mut *streams.1.lock().await, PacketCode::Upload
                {
                    hash,
                    token: Some(token),
                    uid: Some(uid),
                }, Some(&keys)).await;
            },

            //DOWNLOAD
            PacketCode::Download { id: owner_id, file_id, .. } =>
            {
                //FIND USERNAME BY ID
                let username = CONNECTIONS.iter()
                    .find(|entry| entry.value().id() == owner_id.as_ref())
                    .and_then(|entry| entry.value().username().cloned());

                //GET USER UPLOADS
                if let Some(username) = username &&
                    let Some(file_id) = file_id &&
                    let Some(file) = AVAILABLE_FILES.get(&username).and_then(|f| f.value().get(file_id).cloned())
                {
                    //GENERATE RANDOM SHARE UID
                    let uid = rand::random::<u64>();

                    //OPEN NEW CONNECTION
                    let token = open_connection(id, ConnectionType::FileDownload { uid, file });

                    network::send(&mut *streams.1.lock().await, PacketCode::Download
                    {
                        token: Some(token),
                        file_id: None,
                        id: None,
                    }, Some(&keys)).await;

                    //LOG START
                    log::info!("Download request: {peer_addr}");
                } else
                {
                    network::send(&mut *streams.1.lock().await, PacketCode::InvalidUsage, Some(&keys)).await;
                }
            },

            //SCREEN SHARE
            PacketCode::Screen { .. } =>
            {
                //CHECK FOR DISABLING SCREEN SHARE
                if let Some((Some(removed_id), Some(username))) = CONNECTIONS.get_mut(&peer_addr)
                    .and_then(|mut conn| Some((conn.remove_screen_stream(), conn.username().cloned())))
                {
                    //DEATTACH ALL CLIENTS
                    deattach(removed_id, &username).await;

                    //SEND SCREEN DISABLE NOTIFICATION
                    network::send(&mut *streams.1.lock().await, PacketCode::Screen { token: None }, Some(&keys)).await;
                    continue;
                }

                //CHECK FOR ENABLED SCREENSHARE
                if config::read_config("enable_screenshare")
                {
                    //SEND SCREEN ACCEPT
                    network::send(&mut *streams.1.lock().await, PacketCode::Screen
                    {
                        token: Some(open_connection(id, ConnectionType::Screen))
                    }, Some(&keys)).await;

                    //LOG START
                    log::info!("Screen share: {peer_addr}");
                } else
                {
                    network::send(&mut *streams.1.lock().await, PacketCode::InvalidFeature, Some(&keys)).await;
                }
            },

            //SCREENSHARE ATTACH
            PacketCode::Attach { id: sharer_id, .. } =>
            {
                //FIND SHARER ADDRESS BY ID
                let sharer_info = sharer_id.and_then(|sid|
                {
                    CONNECTIONS.iter().find(|entry| entry.value().id() == Some(&sid) && entry.screen_stream().is_some())
                        .and_then(|conn| conn.username().map(|u| (sid, u.to_owned())))
                });

                if let Some((sharer_id, sharer_username)) = sharer_info
                { //VALID SHARER FOUND
                    //OPEN NEW CONNECTION
                    let token = open_connection(id, ConnectionType::Attach
                    {
                        id: sharer_id,
                    });

                    //SEND ACCEPT
                    network::send(&mut *streams.1.lock().await, PacketCode::Attach
                    {
                        id: None,
                        username: Some(sharer_username.to_owned()),
                        token: Some(token),
                    }, Some(&keys)).await;

                    //LOG START
                    log::info!("Screen attach: {peer_addr}");
                } else
                {
                    //INVALID ARGS
                    network::send(&mut *streams.1.lock().await, PacketCode::InvalidUsage, Some(&keys)).await;
                }
            },

            //SCREENSHARE DEATTACH
            PacketCode::Deattach { .. } =>
            {
                //DEATTACH
                if let Some(sharer_id) = if let Some(mut conn) = CONNECTIONS.get_mut(&peer_addr)
                {
                    if let Some(target_id) = conn.attached_screen().as_ref().map(|a| a.target_id)
                    {
                        conn.deattach_screen();
                        Some(target_id)
                    } else { None }
                } else { None }
                {
                    //FIND SHARER USERNAME
                    let sharer_uname = CONNECTIONS.iter()
                        .find(|c| c.value().id() == Some(&sharer_id))
                        .and_then(|c| c.value().username().cloned());

                    //SEND ACCEPT
                    network::send(&mut *streams.1.lock().await, PacketCode::Deattach { username: sharer_uname }, Some(&keys)).await;
                } else
                {
                    //NOT ATTACHED
                    network::send(&mut *streams.1.lock().await, PacketCode::InvalidUsage, Some(&keys)).await;
                }
            },

            //LIST FILES
            PacketCode::Files { .. } =>
            {
                //GET ALL UPLOADS
                let mut users = Vec::new();
                for entry in AVAILABLE_FILES.iter()
                {
                    //GET VALUES
                    let username = entry.key();
                    let uploads = entry.value();

                    if !uploads.is_empty() //DO NOT ADD USERS WITH NO UPLOADS
                    {
                        //GET ID TO THE USERNAME
                        let id = CONNECTIONS.iter()
                            .find(|c| c.username() == Some(&username))
                            .and_then(|c| c.id().copied()).unwrap();

                        //GET USER'S UPLOADS
                        let upload: Vec<(String, usize)> = uploads.iter().enumerate()
                            .map(|(idx, u)| (u.filename.clone(), idx)).collect();

                        //ADD TO LIST
                        users.push(UserFile
                        {
                            username: username.to_string(),
                            id,
                            upload,
                        });
                    }
                }

                //SEND LIST BACK TO CLIENT
                network::send(&mut *streams.1.lock().await, PacketCode::Files { users: Some(users) }, Some(&keys)).await;
            },

            //LIST SCREENSHARES
            PacketCode::Screens { .. } =>
            {
                let mut users = Vec::new();

                //ITERATE OVER CONNECTIONS, CREATE JSON OF USERS
                for connection_enum in CONNECTIONS.iter()
                {
                    if let Connection::Authenticated { username: uname, id: user_id, screen_stream, .. } = connection_enum.value()
                    {
                        if screen_stream.is_none() { continue; }

                        users.push(UserScreen
                        {
                            username: uname.clone(),
                            id: *user_id,
                        });
                    }
                }

                //SEND LIST BACK TO CLIENT
                network::send(&mut *streams.1.lock().await, PacketCode::Screens { users: Some(users) }, Some(&keys)).await;
            },

            //PRIVATE MESSAGE
            PacketCode::PrivateMessage { text, id: recipient_id, .. } =>
            {
                //FIND RECIPIENT BY ID
                let recipient_addr = CONNECTIONS.iter()
                    .find(|entry| entry.value().id() == Some(&recipient_id))
                    .map(|entry| *entry.key());

                if let Some(recipient_addr) = recipient_addr
                {
                    //SEND TO RECIPIENT (IF NOT SELF-MESSAGE)
                    if recipient_id != id
                    {
                        let recipient_data = if let Some(recipient) =
                            CONNECTIONS.get(&recipient_addr)
                        {
                            Some((recipient.write_stream().clone(), recipient.keys().cloned()))
                        } else
                        {
                            None
                        };

                        //SEND
                        if let Some((recipient_stream, recipient_keys)) = recipient_data
                        {
                            network::send(&mut *recipient_stream.lock().await, PacketCode::PrivateMessage
                            {
                                text: text.clone(),
                                username: Some(username.clone()),
                                id,
                            }, recipient_keys.as_ref()).await;
                        }
                    }

                    //SEND CONFIRMATION BACK TO SENDER
                    let recipient_uname = CONNECTIONS.get(&recipient_addr).and_then(|e| e.username().cloned()).unwrap();
                    network::send(&mut *streams.1.lock().await, PacketCode::PrivateMessageBack
                    {
                        text,
                        id: recipient_id,
                        username: recipient_uname,
                    }, Some(&keys)).await;
                } else
                {
                    //INVALID PM FORMAT
                    network::send(&mut *streams.1.lock().await, PacketCode::InvalidUsage, Some(&keys)).await;
                }
            },

            //SERVER CONFIGURATION
            PacketCode::ServerSettings { settings, save } =>
            {
                //VERIFY PERMISSIONS
                if role < consts::SERVER_SETTINGS_ROLE
                {
                    network::send(&mut *streams.1.lock().await, PacketCode::InvalidUsage, Some(&keys)).await;
                    continue;
                }

                //A SAVE WITHOUT ROWS IS NOT A SAVE, AND A READ IGNORES WHATEVER IT WAS SENT WITH
                if save && let Some(settings) = &settings
                {
                    config::server_settings_write(settings);
                }

                network::send(&mut *streams.1.lock().await, PacketCode::ServerSettings
                {
                    settings: Some(config::server_settings()),
                    save,
                }, Some(&keys)).await;
            },

            //KEEPALIVE
            PacketCode::KeepAlive =>
            {
                //SET TO ALIVE
                if let Some(mut conn) = CONNECTIONS.get_mut(&peer_addr)
                {
                    conn.set_alive(true);
                }
            },

            _ => {}
        }
    }
}

pub async fn disconnect_all() //DISCONNECT ALL CLIENTS
{
    //ITERATE OVER ALL ADDRESSES, REMOVE CONNECTIONS
    let addrs: Vec<SocketAddr> = CONNECTIONS.iter().map(|conn| *conn.peer_addr()).collect();
    for addr in &addrs
    {
        remove_connection(addr, true, None).await; //REMOVE GRACEFULLY
    }
}

pub async fn disconnect_inactive() //DISCONNECT ALL INACTIVE CLIENTS
{
    let now = Instant::now();

    //COLLECT ADDRESSES OF INACTIVE CONNECTIONS
    let inactive_addrs: Vec<SocketAddr> = CONNECTIONS.iter()
        .filter(|conn| conn.is_inactive(Some(now)))
        .map(|conn| *conn.peer_addr())
        .collect();

    //DISCONNECT INACTIVE CLIENTS
    for addr in &inactive_addrs
    {
        remove_connection(addr, true, Some("inactive")).await;
    }
}

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
        remove_connection(&dead, false, Some("dead")).await;
    }
}
