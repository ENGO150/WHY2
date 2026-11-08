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

//! # Connection State
//!
//! [`Connection`] is everything the server knows about one client on its main connection,
//! stored in [`CONNECTIONS`](super::CONNECTIONS) by peer address.
//!
//! A connection starts [`NonAuthenticated`](Connection::NonAuthenticated) — keys and a chosen
//! username at most — and becomes [`Authenticated`](Connection::Authenticated) on login, which
//! adds its identity, its rate-limit state and its attached transfers. Accessors for
//! authenticated-only state return `None` (or a neutral value) for an unauthenticated
//! connection.
//!
//! A connection holds [`AbortHandle`]s rather than sockets for its tasks: tokio has no way to
//! shut down a split stream from outside, so force-closing a connection means aborting the
//! task that owns its halves.

use std::
{
    path::PathBuf,
    net::SocketAddr,
    time::{ Instant, Duration },
    collections::HashMap,
    sync::{ Arc, Mutex as MutexSync },
};

use tokio::
{
    sync::Mutex,
    net::tcp::OwnedWriteHalf,
    task::AbortHandle,
};

use zeroize::Zeroizing;

use crate::
{
    config,
    role::Role,
    consts::SharedKeys,
    network::codes::Device,
};

//STRUCTS
/// A file shared by a user, available for download.
#[derive(Clone)]
pub struct AvailableFile //UPLOADED FILE
{
    /// Whether the file has a persistent at-rest authentication tag.
    pub persistent: bool,
    /// SHA-256 of the file.
    pub hash: [u8; 32],   //FILE HASH
    /// Where the sealed file is on disk.
    pub path: PathBuf,    //PATH
    /// The file's name.
    pub filename: String, //FILENAME
    /// Plaintext size in bytes.
    pub size: u64,        //FILE SIZE
    /// The file's at-rest grid key, derived for persistent files and random for temporary ones.
    pub key: Zeroizing<Vec<i64>>,
    /// The file's at-rest nonce.
    pub nonce: Vec<i64>,
}

/// A viewer's attachment to a screen share.
#[derive(Clone)]
pub struct Attach //SCREEN ATTACHMENT
{
    /// The viewer's socket, frames are written to.
    pub stream: Arc<Mutex<OwnedWriteHalf>>, //RECEIVE STREAM
    /// Session id of the sharer.
    pub target_id: usize,                   //ID OF SCREENSHARER
    /// The attachment's token, which keys its packet stream.
    pub token: [u8; 32],                    //TOKEN FOR REXSTREAM
    /// Whether the viewer muted the share's audio.
    pub muted: bool,                        //SHARE AUDIO MUTED
}

//ENUMS
/// What an auxiliary connection is for, as recorded with its token.
pub enum ConnectionType //TYPES OF TCP CHANNEL
{
    /// Receives a fileshare upload.
    FileUpload
    {
        uid: u64,
    },
    /// Receives a persistent attachment upload.
    Attachment
    {
        uid: u64,
    },
    /// Receives an avatar upload.
    Avatar
    {
        uid: u64,
    },
    /// Receives a server icon upload.
    Icon
    {
        uid: u64,
    },
    /// Receives a voice message upload.
    Voice
    {
        uid: u64,
    },
    /// Sends a shared file.
    FileDownload
    {
        uid: u64,
        file: AvailableFile,
    },
    /// Receives a screen share.
    Screen,
    /// Sends a screen share to a viewer; `id` is the sharer.
    Attach
    {
        id: usize,
    },
}

/// One client's main connection.
#[derive(Clone)]
pub enum Connection //CLIENT CONNECTION (WHAT IS PUSHED TO connections LIST)
{
    /// A logged-in client.
    Authenticated
    {
        /// The write half, shared with everything that sends to this client.
        write_stream: Arc<Mutex<OwnedWriteHalf>>,                //STREAM
        /// The connection's handler task; aborting it force-closes the connection.
        task: AbortHandle,                                       //HANDLER TASK (USED TO FORCE-CLOSE THE CONNECTION)
        /// Tasks of active file transfers, by upload id.
        file_streams: Arc<MutexSync<HashMap<u64, AbortHandle>>>, //ACTIVE FILE TRANSFER TASKS
        /// Task receiving this client's screen share.
        screen_stream: Option<AbortHandle>,                      //SCREEN UPLOAD TASK
        /// The client's address.
        peer_addr: SocketAddr,                                   //ADDRESS & PORT
        /// The account's username.
        username: String,                                        //USERNAME
        /// What the client says it runs on.
        device: Option<Device>,                                  //WHAT THE CLIENT SAYS IT RUNS ON
        /// The account's role, kept current when it is changed.
        role: Role,                                              //ROLE
        /// The session id.
        id: usize,                                               //ID OF USER
        /// The session keys.
        keys: SharedKeys,                                        //SHARED KEYS BETWEEN SERVER AND CLIENT (one to one)
        /// The screen share this client is watching.
        attached_screen: Option<Attach>,                         //SCREEN DOWNLOAD STREAM & TARGET ID
        /// When the client last sent a chat message; drives the message gap and the inactivity timeout.
        last_activity: Instant,                                  //TIME OF LAST MESSAGE (USED FOR TIMEOUT)
        /// When the keys were last rotated.
        last_key_exchange: Instant,                              //TIME OF LAST REKEY
        /// When an attachment fetch was last served.
        last_attachment: Instant,                                //TIME OF LAST SERVED ATTACHMENT FETCH
        /// When a typing notice was last forwarded.
        last_typing: Instant,                                    //TIME OF LAST FORWARDED TYPING NOTICE
        /// Session id of whoever last sent this client a private message.
        last_pm: Option<usize>,                                  //ID OF LAST PM SENDER
        /// Consecutive message-gap violations.
        spam_violations: usize,                                  //SPAM VIOLATIONS (unexpected, huh?)
        /// Tokens left in the packet-rate bucket.
        credit: f32,                                             //PACKET RATE TOKENS LEFT
        /// When the bucket was last topped up.
        refill: Instant,                                         //WHEN credit WAS LAST TOPPED UP
        /// Consecutive packets that had to wait for credit.
        throttles: usize,                                        //PACKETS IN A ROW THAT HAD TO WAIT FOR CREDIT
        /// The channel; `None` is the lobby.
        channel: Option<String>,                                 //CHANNEL
        /// Last sequence number received.
        seq: usize,                                              //SEQUENCE NUMBER (CLIENT -> SERVER)
        /// Last sequence number sent.
        server_seq: usize,                                       //SEQUENCE NUMBER (SERVER -> CLIENT)
        /// Whether the client answered the last keepalive.
        alive: bool,                                             //RESPONDED TO KEEPALIVE
        /// Whether a moderator muted the client.
        muted: bool,                                             //USER HAS SAID TOO MUCH, GIVING HIM A REST
    },

    /// A client that has not logged in yet.
    NonAuthenticated
    {
        /// The write half.
        write_stream: Arc<Mutex<OwnedWriteHalf>>, //STREAM
        /// The connection's handler task.
        task: AbortHandle,                        //HANDLER TASK
        /// The client's address.
        peer_addr: SocketAddr,                    //ADDRESS & PORT
        /// The username the client asked for.
        username: Option<String>,                 //CHOSEN USERNAME
        /// The session keys, once agreed.
        keys: Option<SharedKeys>,                 //SHARED KEYS
        /// The client's opening bytes; obfuscates traffic before keys exist.
        obfuscation_key: [u8; 32],                //OBFUSCATION KEY FROM PLAIN PACKETS
        /// When the client last sent a chat message.
        last_activity: Instant,                   //TIME OF LAST MESSAGE
        /// Last sequence number received.
        seq: usize,                               //SEQUENCE NUMBER
        /// When the client connected; bounds the time allowed to log in.
        connect: Instant,                         //TIME OF CONNECTION
    },
}

//IMPLEMENTATIONS
impl Connection
{
    //GET STREAM FROM Connection
    /// Returns the shared write half.
    pub fn write_stream(&self) -> &Arc<Mutex<OwnedWriteHalf>>
    {
        match self
        {
            Self::Authenticated { write_stream, .. } => write_stream,
            Self::NonAuthenticated { write_stream, .. } => write_stream,
        }
    }

    //GET HANDLER TASK FROM Connection
    /// Returns the handler task's abort handle.
    pub fn task(&self) -> &AbortHandle
    {
        match self
        {
            Self::Authenticated { task, .. } => task,
            Self::NonAuthenticated { task, .. } => task,
        }
    }

    //GET ALL ACTIVE FILE STREAMS
    /// Returns the active file transfer tasks.
    pub fn file_streams(&self) -> Option<&Arc<MutexSync<HashMap<u64, AbortHandle>>>>
    {
        match self
        {
            Self::Authenticated { file_streams, .. } => Some(file_streams),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET PEER ADDR FROM Connection#stream
    /// Returns the client's address.
    pub fn peer_addr(&self) -> &SocketAddr
    {
        match self
        {
            Self::Authenticated { peer_addr, .. } => peer_addr,
            Self::NonAuthenticated { peer_addr, .. } => peer_addr,
        }
    }

    //GET USERNAME FROM Connection
    /// Returns the username, if one is known.
    pub fn username(&self) -> Option<&String>
    {
        match self
        {
            Self::Authenticated { username, .. } => Some(username),
            Self::NonAuthenticated { username, .. } => username.as_ref(),
        }
    }

    //GET USERNAME FROM Connection AS MUTABLE
    /// Returns the chosen username of an unauthenticated connection, for setting.
    pub(super) fn username_mut(&mut self) -> &mut Option<String>
    {
        match self
        {
            Self::Authenticated { .. } => panic!("Do not use username_mut() on Authenticated client"),
            Self::NonAuthenticated { username, .. } => username,
        }
    }

    //GET ROLE
    /// Returns the role.
    pub fn role(&self) -> Option<&Role>
    {
        match self
        {
            Self::Authenticated { role, .. } => Some(role),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET ID FROM Connection
    /// Returns the session id.
    pub fn id(&self) -> Option<&usize>
    {
        match self
        {
            Self::Authenticated { id, .. } => Some(id),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET SHARED KEYS FROM Connection
    /// Returns the session keys, once agreed.
    pub fn keys(&self) -> Option<&SharedKeys>
    {
        match self
        {
            Self::Authenticated { keys, .. } => Some(keys),
            Self::NonAuthenticated { keys, .. } => keys.as_ref(),
        }
    }

    //GET OBFUSCATION KEY
    /// Returns the obfuscation key of an unauthenticated connection.
    pub fn obfuscation_key(&self) -> Option<&[u8; 32]>
    {
        match self
        {
            Self::Authenticated { .. } => None,
            Self::NonAuthenticated { obfuscation_key, .. } => Some(obfuscation_key),
        }
    }

    //GET LAST ACTIVITY FROM Connection
    /// Returns when the client last sent a chat message.
    pub fn last_activity(&self) -> &Instant
    {
        match self
        {
            Self::Authenticated { last_activity, .. } => last_activity,
            Self::NonAuthenticated { last_activity, .. } => last_activity,
        }
    }

    //GET LAST ACTIVITY FROM Connection AS MUTABLE
    /// Returns when the client last sent a chat message, for setting.
    pub fn last_activity_mut(&mut self) -> &mut Instant
    {
        match self
        {
            Self::Authenticated { last_activity, .. } => last_activity,
            Self::NonAuthenticated { last_activity, .. } => last_activity,
        }
    }

    //GET LAST SERVED ATTACHMENT FETCH FROM Connection
    /// Returns when an attachment fetch was last served.
    pub fn last_attachment(&self) -> Option<&Instant>
    {
        match self
        {
            Self::Authenticated { last_attachment, .. } => Some(last_attachment),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET LAST SERVED ATTACHMENT FETCH AS MUTABLE
    /// Returns when an attachment fetch was last served, for setting.
    pub fn last_attachment_mut(&mut self) -> Option<&mut Instant>
    {
        match self
        {
            Self::Authenticated { last_attachment, .. } => Some(last_attachment),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET LAST FORWARDED TYPING NOTICE
    /// Returns when a typing notice was last forwarded.
    pub fn last_typing(&self) -> Option<&Instant>
    {
        match self
        {
            Self::Authenticated { last_typing, .. } => Some(last_typing),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET LAST FORWARDED TYPING NOTICE AS MUTABLE
    /// Returns when a typing notice was last forwarded, for setting.
    pub fn last_typing_mut(&mut self) -> Option<&mut Instant>
    {
        match self
        {
            Self::Authenticated { last_typing, .. } => Some(last_typing),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET LAST PM SENDER
    /// Returns the session id of whoever last sent this client a private message.
    pub fn last_pm(&self) -> Option<usize>
    {
        match self
        {
            Self::Authenticated { last_pm, .. } => *last_pm,
            Self::NonAuthenticated { .. } => None,
        }
    }

    //SET LAST PM SENDER
    /// Records who last sent this client a private message.
    pub fn set_last_pm(&mut self, id: usize)
    {
        match self
        {
            Self::Authenticated { last_pm, .. } => *last_pm = Some(id),
            _ => {},
        }
    }

    //UNSET LAST PM SENDER
    /// Forgets who last sent this client a private message.
    pub fn unset_last_pm(&mut self)
    {
        match self
        {
            Self::Authenticated { last_pm, .. } => *last_pm = None,
            _ => {},
        }
    }

    //GET LAST KEY EXCHANGE FROM Connection
    /// Returns when the keys were last rotated.
    pub fn last_key_exchange(&self) -> Option<&Instant>
    {
        match self
        {
            Self::Authenticated { last_key_exchange, .. } => Some(last_key_exchange),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET SPAM VIOLATIONS FROM Connection
    /// Returns the consecutive message-gap violations.
    pub fn spam_violations(&self) -> Option<&usize>
    {
        match self
        {
            Self::Authenticated { spam_violations, .. } => Some(spam_violations),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET SPAM VIOLATIONS FROM Connection AS MUTABLE
    /// Returns the consecutive message-gap violations, for setting.
    pub fn spam_violations_mut(&mut self) -> Option<&mut usize>
    {
        match self
        {
            Self::Authenticated { spam_violations, .. } => Some(spam_violations),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET PACKET RATE CREDIT FROM Connection
    /// Returns the tokens left in the packet-rate bucket.
    pub fn credit(&self) -> Option<&f32>
    {
        match self
        {
            Self::Authenticated { credit, .. } => Some(credit),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET LAST CREDIT REFILL FROM Connection
    /// Returns when the packet-rate bucket was last topped up.
    pub fn refill(&self) -> Option<&Instant>
    {
        match self
        {
            Self::Authenticated { refill, .. } => Some(refill),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET CONSECUTIVE THROTTLES FROM Connection
    /// Returns the consecutive packets that had to wait for credit.
    pub fn throttles(&self) -> Option<&usize>
    {
        match self
        {
            Self::Authenticated { throttles, .. } => Some(throttles),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //PAY FOR ONE PACKET OUT OF THE RATE BUCKET
    /// Pays for one packet out of the packet-rate bucket.
    ///
    /// The bucket refills lazily at `rate` tokens a second up to `burst`, so it needs no timer.
    /// An overdrawn packet is not refused: the returned wait is how long the caller holds this
    /// connection's own read loop, which is the backpressure.
    ///
    /// With credit $c$, refill rate $r$, bucket size $b$ and $\Delta t$ the time since the last
    /// packet, each packet does
    ///
    /// $$ c \leftarrow \min(c + r\Delta t,\ b) - 1 \qquad w = \begin{cases} 0 & c \ge 0 \\\\ -c/r & c \lt 0 \end{cases} $$
    ///
    /// so an overdrawn packet waits exactly until the bucket would have refilled to zero.
    ///
    /// # Parameters
    /// - `rate`: Refill rate in packets per second (`max_packet_rate`).
    /// - `burst`: Bucket size (`max_packet_burst`).
    ///
    /// # Returns
    /// How long to wait before handling the packet; zero if there was credit. An unauthenticated
    /// connection always gets zero.
    pub fn take_credit(&mut self, rate: f32, burst: f32) -> Duration
    {
        let Self::Authenticated { credit, refill, throttles, .. } = self else { return Duration::ZERO; };

        //A CONFIG OF ZERO WOULD NEVER REFILL
        let rate = rate.max(0.001);
        let burst = burst.max(1.0);

        //TOP UP FOR THE ELAPSED TIME, THEN PAY
        *credit = (*credit + refill.elapsed().as_secs_f32() * rate).min(burst) - 1.0;
        *refill = Instant::now();

        if *credit >= 0.0
        {
            *throttles = 0;
            return Duration::ZERO;
        }

        //OVERDRAWN - WAIT ON THIS CONNECTION'S OWN LOOP
        *throttles += 1;

        Duration::from_secs_f32(-*credit / rate)
    }

    //GET DEVICE
    /// Returns what the client says it runs on.
    pub fn device(&self) -> &Option<Device>
    {
        match self
        {
            Self::Authenticated { device, .. } => device,
            Self::NonAuthenticated { .. } => &None,
        }
    }

    //GET CHANNEL
    /// Returns the channel; `None` is the lobby.
    pub fn channel(&self) -> &Option<String>
    {
        match self
        {
            Self::Authenticated { channel, .. } => channel,
            Self::NonAuthenticated { .. }  => &None,
        }
    }

    //GET LAST SEQUENCE NUMBER
    /// Returns the last sequence number received.
    pub fn seq(&self) -> &usize
    {
        match self
        {
            Self::Authenticated { seq, .. } => seq,
            Self::NonAuthenticated { seq, .. } => seq,
        }
    }

    //GET LAST SEQUENCE NUMBER AS MUTABLE
    /// Returns the last sequence number received, for setting.
    pub fn seq_mut(&mut self) -> &mut usize
    {
        match self
        {
            Self::Authenticated { seq, .. } => seq,
            Self::NonAuthenticated { seq, .. } => seq,
        }
    }

    //GET LAST SERVER SEQUENCE NUMBER
    /// Returns the last sequence number sent.
    pub fn server_seq(&self) -> Option<&usize>
    {
        match self
        {
            Self::Authenticated { server_seq, .. } => Some(server_seq),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //GET LAST SERVER SEQUENCE NUMBER AS MUTABLE
    /// Returns the last sequence number sent, for setting.
    pub fn server_seq_mut(&mut self) -> Option<&mut usize>
    {
        match self
        {
            Self::Authenticated { server_seq, .. } => Some(server_seq),
            Self::NonAuthenticated { .. } => None,
        }
    }

    //CHECK IF CONNECTION IS INACTIVE
    /// Returns whether the connection should be dropped for inactivity: no chat message for
    /// `communication_time` seconds when authenticated, or no login within `max_auth_time`.
    ///
    /// # Parameters
    /// - `now`: The time to measure against, so a sweep uses one instant for every connection.
    pub(super) fn is_inactive(&self, now: Option<Instant>) -> bool
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
    /// Returns whether the client has logged in.
    pub fn is_authenticated(&self) -> bool
    {
        match self
        {
            Self::Authenticated { .. } => true,
            Self::NonAuthenticated { .. } => false,
        }
    }

    //SET CONNECTION TO ALIVE
    /// Records whether the client answered the last keepalive.
    pub fn set_alive(&mut self, val: bool)
    {
        match self
        {
            Self::Authenticated { alive, .. } => *alive = val,
            _ => {}
        }
    }

    //CHECK IF CONNECTION IS ALIVE
    /// Returns whether the client answered the last keepalive.
    pub fn is_alive(&self) -> &bool
    {
        match self
        {
            Self::Authenticated { alive, .. } => alive,
            Self::NonAuthenticated { .. } => &false,
        }
    }

    //ADD FILE STREAM
    /// Registers a file transfer task.
    pub fn add_file_stream(&self, uid: u64, task: AbortHandle)
    {
        if let Self::Authenticated { file_streams, .. } = self
        {
            file_streams.lock().unwrap().insert(uid, task);
        }
    }

    //REMOVE FILE STREAM
    /// Unregisters a file transfer task and aborts it, closing its socket.
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
    /// Returns the task receiving this client's screen share.
    pub fn screen_stream(&self) -> &Option<AbortHandle>
    {
        match self
        {
            Self::Authenticated { screen_stream, .. } => screen_stream,
            Self::NonAuthenticated { .. } => &None,
        }
    }

    //GET ATTACHED SCREENSHARE
    /// Returns the screen share this client is watching.
    pub fn attached_screen(&self) -> &Option<Attach>
    {
        match self
        {
            Self::Authenticated { attached_screen, .. } => attached_screen,
            Self::NonAuthenticated { .. } => &None,
        }
    }

    //SET ATTACHED SCREENSHARE
    /// Attaches this client to a screen share, unmuted.
    ///
    /// # Parameters
    /// - `target_id`: The sharer's session id.
    /// - `stream`: The viewer socket's write half.
    /// - `token`: The attachment's token.
    pub fn attach_screen(&mut self, target_id: usize, stream: Arc<Mutex<OwnedWriteHalf>>, token: [u8; 32])
    {
        match self
        {
            Self::Authenticated { attached_screen, .. } => *attached_screen = Some(Attach
            {
                target_id,
                stream,
                token,
                muted: false,
            }),
            _ => {},
        }
    }

    //TOGGLE MUTE ATTACHED SCREENSHARE
    /// Toggles the audio of the watched share.
    ///
    /// # Returns
    /// `true` if it is now muted; `false` also when nothing is attached.
    pub fn toggle_mute_screen(&mut self) -> bool
    {
        match self
        {
            Self::Authenticated { attached_screen, .. } =>
            {
                if let Some(attached_screen) = attached_screen
                {
                    let new_mute = !attached_screen.muted;
                    attached_screen.muted = new_mute;

                    return new_mute;
                }
            },
            _ => {},
        }
        false
    }

    //UNSET ATTACHED SCREENSHARE
    /// Detaches this client from the share it is watching.
    pub fn deattach_screen(&mut self)
    {
        match self
        {
            Self::Authenticated { attached_screen, .. } =>
            {
                let target = attached_screen.take().map(|attached| attached.target_id);

                log::info!("Stop screen attach: {} (was watching id {})", self.peer_addr(),
                    target.map(|id| id.to_string()).unwrap_or_else(|| String::from("nobody")));
            },
            _ => {},
        }
    }

    //ADD SCREEN UPLOAD STREAM
    /// Registers the task receiving this client's screen share, aborting any previous one.
    pub fn set_screen_stream(&mut self, task: AbortHandle)
    {
        //CLEAN OLD STREAM
        self.remove_screen_stream();

        if let Self::Authenticated { screen_stream, .. } = self
        {
            *screen_stream = Some(task);
        }
    }

    //TAKE SCREEN UPLOAD STREAM WITHOUT ABORTING IT
    /// Unregisters the screen share task without aborting it.
    ///
    /// # Returns
    /// The session id and the task, if a share was running.
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
    /// Unregisters and aborts the screen share task.
    ///
    /// # Returns
    /// The session id, if a share was running.
    pub fn remove_screen_stream(&mut self) -> Option<usize>
    {
        self.take_screen_stream().map(|(id, old_task)|
        {
            old_task.abort();
            id
        })
    }

    //MUTE
    /// Toggles the client's server-side mute.
    pub fn toggle_mute(&mut self)
    {
        if let Self::Authenticated { muted, .. } = self
        {
            *muted = !*muted;
        }
    }

    //SET ROLE
    /// Updates the client's role in its live session.
    pub fn set_role(&mut self, new_role: Role)
    {
        if let Self::Authenticated { role, .. } = self
        {
            *role = new_role;
        }
    }

    //MUTED
    /// Returns whether a moderator muted the client.
    pub fn muted(&self) -> &bool
    {
        match self
        {
            Self::Authenticated { muted, .. } => muted,
            Self::NonAuthenticated { .. } => &false,
        }
    }
}
