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

//ENUMS
pub enum ConnectionType //TYPES OF TCP CHANNEL
{
    FileUpload
    {
        uid: u64,
    },
    Image
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
        write_stream: Arc<Mutex<OwnedWriteHalf>>,                //STREAM
        task: AbortHandle,                                       //HANDLER TASK (USED TO FORCE-CLOSE THE CONNECTION)
        file_streams: Arc<MutexSync<HashMap<u64, AbortHandle>>>, //ACTIVE FILE TRANSFER TASKS
        screen_stream: Option<AbortHandle>,                      //SCREEN UPLOAD TASK
        peer_addr: SocketAddr,                                   //ADDRESS & PORT
        username: String,                                        //USERNAME
        role: Role,                                              //ROLE
        id: usize,                                               //ID OF USER
        keys: SharedKeys,                                        //SHARED KEYS BETWEEN SERVER AND CLIENT (one to one)
        attached_screen: Option<Attach>,                         //SCREEN DOWNLOAD STREAM & TARGET ID
        last_activity: Instant,                                  //TIME OF LAST MESSAGE (USED FOR TIMEOUT)
        last_key_exchange: Instant,                              //TIME OF LAST REKEY
        spam_violations: usize,                                  //SPAM VIOLATIONS (unexpected, huh?)
        channel: Option<String>,                                 //CHANNEL
        seq: usize,                                              //SEQUENCE NUMBER (CLIENT -> SERVER)
        server_seq: usize,                                       //SEQUENCE NUMBER (SERVER -> CLIENT)
        alive: bool,                                             //RESPONDED TO KEEPALIVE
        muted: bool,                                             //USER HAS SAID TOO MUCH, GIVING HIM A REST
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
    pub(super) fn username_mut(&mut self) -> &mut Option<String>
    {
        match self
        {
            Self::Authenticated { .. } => panic!("Do not use username_mut() on Authenticated client"),
            Self::NonAuthenticated { username, .. } => username,
        }
    }

    //GET ROLE
    pub fn role(&self) -> Option<&Role>
    {
        match self
        {
            Self::Authenticated { role, .. } => Some(role),
            Self::NonAuthenticated { .. } => None,
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

    //MUTE
    pub fn toggle_mute(&mut self)
    {
        if let Self::Authenticated { muted, .. } = self
        {
            *muted = !*muted;
        }
    }

    //SET ROLE
    pub fn set_role(&mut self, new_role: Role)
    {
        if let Self::Authenticated { role, .. } = self
        {
            *role = new_role;
        }
    }

    //MUTED
    pub fn muted(&self) -> &bool
    {
        match self
        {
            Self::Authenticated { muted, .. } => muted,
            Self::NonAuthenticated { .. } => &false,
        }
    }
}
