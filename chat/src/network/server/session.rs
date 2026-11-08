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

//! # Client Session
//!
//! The packet loop of [`listen_client`](super::listen_client), split out of it so no single
//! function carries the whole protocol. [`ClientSession`] owns one authenticated client's
//! streams, keys and identity; [`ClientSession::run`] reads packets and hands each one to the
//! handler for its domain.
//!
//! The invariants are the parent module's: no guard on [`server::CONNECTIONS`](server::CONNECTIONS) is
//! held across an `.await`, and every handler returns [`PacketFlow`] instead of reaching out of
//! the loop.

use std::
{
    path::Path,
    net::SocketAddr,
    time::{ Instant, Duration },
};

use tokio::
{
    time,
    task,
};

use zeroize::Zeroizing;

use crate::
{
    config,
    misc,
    options,
    role::Role,
    crypto::password,
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
        server::
        {
            self,
            handshake,
            connection::ConnectionType,
        },
        codes::
        {
            PacketCode,
            MessageColors,
            ServerSetting,
            UserFile,
            UserProfile,
            UserScreen,
        },
    },
};

//ENUMS
/// What the read loop should do after a packet.
enum PacketFlow
{
    /// Keep listening.
    Continue,
    /// Remove this connection and end the session.
    Disconnect
    {
        /// Whether to tell the client first.
        grace: bool,
        /// Why, for the log.
        reason: Option<&'static str>,
    },
}

//STRUCTS
/// One authenticated client's session.
pub(super) struct ClientSession<'a, 's>
{
    /// The connection's halves.
    streams: &'a mut Streams<'s>,
    /// The client's address.
    peer_addr: SocketAddr,
    /// The bytes the client opened with; the rekey's nonce.
    obfuscation_key: [u8; 32],
    /// The session keys.
    keys: SharedKeys,
    /// The account's username.
    username: String,
    /// The session id.
    id: usize,
    /// The account's role, refreshed before every packet.
    role: Role,
    /// The current channel; `None` is the lobby.
    channel: Option<String>,
}

//IMPLEMENTATIONS
impl<'a, 's> ClientSession<'a, 's>
{
    /// Creates a session for an authenticated client.
    pub(super) fn new
    (
        streams: &'a mut Streams<'s>,
        peer_addr: SocketAddr,
        obfuscation_key: [u8; 32],
        keys: SharedKeys,
        username: String,
        id: usize,
        role: Role,
    ) -> Self
    {
        Self
        {
            streams,
            peer_addr,
            obfuscation_key,
            keys,
            username,
            id,
            role,
            channel: None,
        }
    }

    /// Runs the read loop until the connection ends.
    pub(super) async fn run(mut self)
    {
        //LOOP READING
        loop
        {
            //READ
            let read = match network::receive(&mut *self.streams, Some(&self.keys), None).await
            {
                Some(read) => read,
                None => return,
            };

            //REKEY EVERY 10 MINUTES
            if self.rekey_due()
            {
                self.rekey().await;
            }

            //RE-READ THE ROLE RATHER THAN LATCH IT
            self.role = server::CONNECTIONS.get(&self.peer_addr)
                .and_then(|entry| entry.role().copied())
                .unwrap_or(self.role);

            //WHAT ARRIVED, NEVER WHAT WAS IN IT
            log::debug!("Packet {}: {}", read.name(), self.peer_addr);

            //DISPATCH
            match self.dispatch(read).await
            {
                PacketFlow::Continue => {},

                PacketFlow::Disconnect { grace, reason } =>
                    return server::remove_connection(&self.peer_addr, grace, reason).await,
            }
        }
    }

    //SEND ONE PACKET TO THIS CLIENT
    async fn send(&mut self, code: PacketCode)
    {
        network::send(&mut *self.streams.1.lock().await, code, Some(&self.keys)).await;
    }

    //IS THIS CLIENT MUTED
    fn muted(&self) -> bool
    {
        server::CONNECTIONS.get(&self.peer_addr).is_some_and(|conn| *conn.muted())
    }

    //CHECK WHETHER THE REKEY INTERVAL ELAPSED
    fn rekey_due(&self) -> bool
    {
        let last = server::CONNECTIONS.get(&self.peer_addr)
            .and_then(|entry| entry.last_key_exchange().copied());

        last.is_some_and(|last|
            Instant::now().duration_since(last) >= Duration::from_secs(consts::REKEY_INTERVAL)) &&
            !file::ACTIVE_FILESHARES.iter().any(|entry| entry.client_id == self.id)
    }

    //REKEY THE SESSION
    async fn rekey(&mut self)
    {
        log::debug!("Rekeying: {}", self.peer_addr);

        let current_keys = self.keys.clone();
        handshake::key_exchange(&mut *self.streams, &self.peer_addr, &self.obfuscation_key,
            &mut self.keys, Some(&current_keys)).await;
    }

    //CLIENT -> SERVER PACKET DISPATCH
    async fn dispatch(&mut self, packet: PacketCode) -> PacketFlow
    {
        match packet
        {
            //MESSAGE
            PacketCode::MessageRequest { text, reply } => self.message(text, reply).await,

            //SOMEBODY IS WRITING
            PacketCode::TypingRequest => self.typing().await,

            //CLIENT QUITS
            PacketCode::Disconnect => PacketFlow::Disconnect
            {
                grace: true,
                reason: None,
            },

            //VOICE CALL
            PacketCode::VoiceRequest => self.voice().await,

            //SWITCH CHANNEL
            PacketCode::Channel { channel } => self.change_channel(channel).await,

            //DELETE A STORED MESSAGE
            PacketCode::DeleteRequest { message_id } => self.delete_message(message_id).await,

            //REWORD A STORED MESSAGE
            PacketCode::EditRequest { message_id, text } => self.edit_message(message_id, text).await,

            //CLIENT REQUESTED LIST OF ONLINE USERS
            PacketCode::ListRequest => self.list().await,

            //NEW FILE UPLOAD
            PacketCode::UploadRequest { hash } => self.upload(hash, None).await,
            PacketCode::ImageRequest { hash, filename } => self.upload(hash, Some(filename)).await,

            //NEW VOICE MESSAGE
            PacketCode::VoiceMessageRequest { hash } => self.voice_message(hash).await,

            //DOWNLOAD
            PacketCode::DownloadRequest { id, file_id } => self.download(id, file_id).await,

            //SCREEN SHARE
            PacketCode::ScreenRequest => self.screen().await,
            PacketCode::AttachRequest { id } => self.attach(id).await,
            PacketCode::DeattachRequest => self.deattach_screen().await,
            PacketCode::MuteScreenRequest => self.mute_screen().await,

            //LIST FILES
            PacketCode::FilesRequest => self.files().await,

            //AN OLDER PAGE OF THE HISTORY
            PacketCode::HistoryRequest { before } => self.history(before).await,

            //ONE OF THE HISTORY'S PICTURES
            PacketCode::ImageDataRequest { hash } => self.image_data(hash).await,

            //LIST SCREENSHARES
            PacketCode::ScreensRequest => self.screens().await,

            //PRIVATE MESSAGE (OR REPLY)
            PacketCode::PrivateMessageRequest { text, id } => self.private_message(Some(id), text).await,
            PacketCode::Re { message } => self.private_message(None, message).await,

            //MESSAGE HEART REACTION
            PacketCode::HeartRequest { message_id } => self.heart(message_id).await,

            //TOGGLE A MESSAGE'S PIN
            PacketCode::PinRequest { message_id } => self.pin(message_id).await,

            //LIST PINNED MESSAGES
            PacketCode::PinsRequest => self.pins().await,

            //MUTE USER
            PacketCode::ServerMute { id } => self.server_mute(id).await,

            //KICK USER
            PacketCode::ServerKick { id } => self.server_kick(id).await,

            //BAN USER
            PacketCode::ServerBan { id } => self.server_ban(id).await,

            //BAN USER'S IP
            PacketCode::ServerBanIp { id } => self.server_ban_ip(id).await,

            //PASSWORD CHANGE
            PacketCode::AccountPasswdRequest { old_password, new_password } =>
                self.account_passwd(old_password, new_password).await,

            //ACCOUNT DELETION
            PacketCode::AccountDeleteRequest { password } => self.account_delete(password).await,

            //A /color OR /ucolor
            PacketCode::ColorRequest { username, color } => self.color(username, color).await,

            //A PROFILE, ASKED FOR OR WRITTEN
            PacketCode::ProfileRequest { id } => self.profile_request(id).await,
            PacketCode::ProfileSave { profile } => self.profile_save(profile).await,

            //THE PROFILE PICTURE, SET OR DROPPED
            PacketCode::AvatarRequest { hash } => self.avatar(hash).await,

            //THE SERVER'S PICTURE
            PacketCode::ServerIconRequest => self.server_icon_request().await,

            //THE SERVER'S PICTURE, SET OR DROPPED
            PacketCode::ServerIconSave { hash } => self.server_icon(hash).await,

            //BAN LIST
            PacketCode::ServerBansRequest => self.server_bans().await,

            //LIFT A USERNAME BAN
            PacketCode::ServerPardon { id } => self.server_pardon(id).await,

            //LIFT AN IP BAN
            PacketCode::ServerPardonIp { id } => self.server_pardon_ip(id).await,

            //SAY AS SERVER
            PacketCode::ServerSay { message } => self.server_say(message).await,

            //SET A USER'S ROLE
            PacketCode::ServerRoleRequest { id, role } => self.server_role(id, role).await,

            //SERVER CONFIGURATION
            PacketCode::ServerSettingsRequest => self.server_settings(None).await,
            PacketCode::ServerSettingsSave { settings } => self.server_settings(Some(settings)).await,

            //SERVER RESTART
            PacketCode::ServerRestart => self.server_restart().await,

            //KEEPALIVE
            PacketCode::KeepAlive => self.keepalive().await,

            _ => PacketFlow::Continue,
        }
    }

    //MESSAGE
    async fn message(&mut self, text: String, reply: Option<u64>) -> PacketFlow
    {
        //SILENCE MUTED USERS
        if self.muted()
        {
            log::debug!("Message dropped (muted): {}", self.peer_addr);

            self.send(PacketCode::Muted).await;
            return PacketFlow::Continue;
        }

        //CHECK REPLY VALIDITY
        if let Some(reply) = reply && !config::messages::exists(reply)
        {
            log::warn!("Message refused (invalid reply): {}", self.peer_addr);

            //SEND InvalidUsage CODE
            self.send(PacketCode::InvalidUsage).await;
            return PacketFlow::Continue;
        }

        let text = text.trim().to_owned();

        log::info!("Message ({} chars) in {}: {}", text.chars().count(),
            if self.channel.is_some() { "channel" } else { "lobby" }, self.peer_addr);

        let timestamp = config::messages::timestamp();

        //KEEP IT - ONLY THE LOBBY HAS A HISTORY
        let message_id = match self.channel.is_none() && config::read_config::<bool>("persistent_messages")
        {
            true => config::messages::store(&self.username, &text, timestamp, reply),
            false => config::messages::next_id(),
        };

        //SEND MESSAGE TO ALL USERS, TAGGED WITH ITS CHANNEL
        server::send_to_all(PacketCode::Message
        {
            text,
            username: self.username.clone(),
            id: self.id,
            message_id,
            colors: config::users::colors(&self.username),
            channel: Some(self.channel.clone()),
            timestamp,
            reply,
        });

        PacketFlow::Continue
    }

    //SOMEBODY IS WRITING
    async fn typing(&mut self) -> PacketFlow
    {
        //CHECK DISABLED FEATURE
        if !config::read_config::<bool>("typing_indicator")
        {
            log::debug!("Typing dropped (disabled): {}", self.peer_addr);
            return PacketFlow::Continue;
        }

        //SILENCE MUTED USERS
        if self.muted()
        {
            log::debug!("Typing dropped (muted): {}", self.peer_addr);
            return PacketFlow::Continue;
        }

        //ONE NOTICE PER TYPING_INTERVAL, WHATEVER THE CLIENT SENDS
        let fresh = server::CONNECTIONS.get(&self.peer_addr)
            .and_then(|conn| conn.last_typing().map(|last| last.elapsed() >= consts::TYPING_INTERVAL))
            .unwrap_or_default(); //GUARD DROPPED HERE

        if !fresh
        {
            log::debug!("Typing dropped (too soon): {}", self.peer_addr);
            return PacketFlow::Continue;
        }

        if let Some(mut conn) = server::CONNECTIONS.get_mut(&self.peer_addr)
            && let Some(last) = conn.last_typing_mut() { *last = Instant::now(); }

        //SEND NOTICE TO THE REST OF THE CHANNEL
        server::send_to_others(PacketCode::Typing { username: self.username.clone() },
            self.channel.as_deref(), self.id);

        PacketFlow::Continue
    }

    //VOICE CALL
    async fn voice(&mut self) -> PacketFlow
    {
        //CHECK DISABLED FEATURE
        if !options::voice_chat_enabled()
        {
            log::warn!("Voice refused (disabled): {}", self.peer_addr);

            self.send(PacketCode::InvalidFeature).await;
        } else if !voice_server::CONNECTIONS.contains_key(&self.id) //IS NOT USING VOICE
        {
            log::info!("Voice slot opened: {}", self.peer_addr);

            //OPEN THE VOICE SLOT AND SEND ITS TOKEN
            let token = voice_server::open_connection(self.id, self.username.clone());
            self.send(PacketCode::Voice { token: Some(token) }).await;

            //SEND CODE TO CHANNEL
            server::send_to_all(PacketCode::VoiceJoin
            {
                username: self.username.clone(),
                id: self.id,
                channel: self.channel.clone(),
            });

            //SEND CONNECTED CLIENTS
            server::send_voice_clients(&mut *self.streams.1.lock().await, &self.keys, self.id).await;
        } else //IS USING VOICE
        {
            log::info!("Voice leave: {}", self.peer_addr);

            //ACKNOWLEDGE THE LEAVE
            self.send(PacketCode::Voice { token: None }).await;

            //SEND CODE TO LAST CHANNEL
            server::send_to_all(PacketCode::VoiceLeave { id: self.id, channel: self.channel.clone() });

            //REMOVE FROM VOICE
            voice_server::remove_connection(&self.id);
        }

        PacketFlow::Continue
    }

    //SWITCH CHANNEL
    async fn change_channel(&mut self, tchannel: Option<String>) -> PacketFlow
    {
        //CHECK PARAMETER VALIDITY
        if tchannel.iter().all(|s| !s.is_empty() && s.len() <= config::read_config("max_channel_length") &&
            s.chars().all(|c| c.is_ascii_alphanumeric() && c != ' '))
        {
            //SEND VoiceLeave CODE TO OLD CHANNEL
            if options::voice_chat_enabled() && voice_server::CONNECTIONS.contains_key(&self.id)
            {
                server::send_to_all(PacketCode::VoiceLeave { id: self.id, channel: self.channel.clone() });
            }

            //UPDATE CHANNEL
            server::update_client_channel(&self.peer_addr, &tchannel);
            self.channel = tchannel.clone();
            self.send(PacketCode::Channel { channel: tchannel }).await;

            //SEND CODE TO CHANNEL
            if options::voice_chat_enabled() && voice_server::CONNECTIONS.contains_key(&self.id)
            {
                server::send_to_all(PacketCode::VoiceJoin
                {
                    username: self.username.clone(),
                    id: self.id,
                    channel: self.channel.clone(),
                });
            }

            //SEND CONNECTED CLIENTS
            server::send_voice_clients(&mut *self.streams.1.lock().await, &self.keys, self.id).await;
        } else //INVALID CHANNEL
        {
            log::warn!("Channel refused (invalid name): {}", self.peer_addr);

            //SEND InvalidUsage CODE
            self.send(PacketCode::InvalidUsage).await;
        }

        PacketFlow::Continue
    }

    //DELETE A STORED MESSAGE
    async fn delete_message(&mut self, message_id: u64) -> PacketFlow
    {
        let author = config::messages::author(message_id);
        let own = author.as_deref() == Some(self.username.as_str());

        //OWN, OR A LOWER RANK'S AS A MODERATOR
        let allowed = own || author.is_some_and(|author| self.role >= Role::Moderator
            && config::users::role(&author).is_none_or(|author_role| author_role < self.role));

        if allowed && config::messages::delete(message_id)
        {
            log::info!("Message deleted ({}): {}", if own { "own" } else { "peer" }, self.peer_addr);

            //EVERY CLIENT HOLDS A LOBBY PANE
            server::send_to_all(PacketCode::Deleted { message_id });
        } else
        {
            log::warn!("Delete refused (no such message, or permissions): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
        }

        PacketFlow::Continue
    }

    //REWORD A STORED MESSAGE
    async fn edit_message(&mut self, message_id: u64, text: String) -> PacketFlow
    {
        //SILENCE MUTED USERS
        if self.muted()
        {
            log::debug!("Edit dropped (muted): {}", self.peer_addr);

            self.send(PacketCode::Muted).await;
            return PacketFlow::Continue;
        }

        let text = text.trim().to_owned();

        //OWN TEXT MESSAGES ONLY
        if !text.is_empty() && config::messages::edit(message_id, &self.username, &text)
        {
            log::info!("Message edited ({} chars): {}", text.chars().count(), self.peer_addr);

            //EVERY CLIENT HOLDS A LOBBY PANE
            server::send_to_all(PacketCode::Edited { message_id, text });
        } else
        {
            log::warn!("Edit refused (no such message, empty, or permissions): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
        }

        PacketFlow::Continue
    }

    //LIST OF ONLINE USERS
    async fn list(&mut self) -> PacketFlow
    {
        server::send_list(&self.streams.1, &self.peer_addr, &self.keys).await;
        PacketFlow::Continue
    }

    //NEW FILE OR IMAGE UPLOAD
    async fn upload(&mut self, hash: [u8; 32], filename: Option<String>) -> PacketFlow
    {
        //SILENCE MUTED USERS
        if self.muted()
        {
            self.send(PacketCode::Muted).await;
            return PacketFlow::Continue;
        }

        let image = filename.is_some();

        //CHECK IF IMAGE WAS ALREADY UPLOADED
        if image && config::messages::stored(&hash)
        {
            let filename = filename.as_deref()
                .and_then(|filename| Path::new(filename).file_name())
                .and_then(|f| f.to_str())
                .unwrap_or("unnamed_file")
                .to_string();

            let timestamp = config::messages::timestamp();

            //KEEP IT, ON AN UPLOAD'S TERMS
            let message_id = match self.channel.is_none() && config::read_config::<bool>("persistent_messages")
            {
                true => config::messages::store_image(&self.username, &filename, &hash, timestamp),
                false => config::messages::next_id(),
            };

            //AN IMAGE LINE IS COLORED LIKE A MESSAGE
            server::send_to_all(PacketCode::ImageDisplay
            {
                username: self.username.clone(),
                filename: filename.clone(),
                message_id,
                hash,
                data: None,
                username_color: config::users::colors(&self.username).username_color,
                channel: Some(self.channel.clone()),
                timestamp,
            });

            //TELL THE UPLOADER THERE IS NOTHING TO SEND
            self.send(PacketCode::ImageDuplicate { hash }).await;

            log::info!("Image already stored, upload skipped: {}", self.peer_addr);
            return PacketFlow::Continue;
        }

        //PREVENT TOKEN SPAM
        let active_count = file::ACTIVE_FILESHARES.iter().filter(|u| u.client_id == self.id).count();
        if active_count >= config::read_config::<usize>("max_client_parallel_uploads")
        {
            log::warn!("Upload refused ({active_count} already running): {}", self.peer_addr);

            self.send(PacketCode::UploadLimit).await;
            return PacketFlow::Continue;
        }

        //GENERATE RANDOM UID
        let uid = rand::random::<u64>();
        let token = server::open_connection(self.id, if image
        {
            ConnectionType::Image { uid }
        } else
        {
            ConnectionType::FileUpload { uid }
        });

        //LOG FILE UPLOAD
        log::info!("Upload request ({}): {}", if image { "image" } else { "file" }, self.peer_addr);

        let packet = if image
        {
            PacketCode::Image
            {
                hash,
                token,
                uid,
            }
        } else
        {
            PacketCode::Upload
            {
                hash,
                token,
                uid,
            }
        };

        //SEND APPROVAL TO CLIENT
        self.send(packet).await;
        PacketFlow::Continue
    }

    //NEW VOICE MESSAGE
    async fn voice_message(&mut self, hash: [u8; 32]) -> PacketFlow
    {
        //SILENCE MUTED USERS
        if self.muted()
        {
            self.send(PacketCode::Muted).await;
            return PacketFlow::Continue;
        }

        //CLIP ALREADY STORED
        if let Some(voice) = config::messages::voice(&hash)
        {
            let timestamp = config::messages::timestamp();

            let message_id = match self.channel.is_none() && config::read_config::<bool>("persistent_messages")
            {
                true => config::messages::store_voice(&self.username, &voice, timestamp),
                false => config::messages::next_id(),
            };

            server::send_to_all(PacketCode::VoiceMessage
            {
                username: self.username.clone(),
                message_id,
                voice,
                data: None,
                username_color: config::users::colors(&self.username).username_color,
                channel: Some(self.channel.clone()),
                timestamp,
            });

            self.send(PacketCode::ImageDuplicate { hash }).await;

            log::info!("Voice message already stored, upload skipped: {}", self.peer_addr);
            return PacketFlow::Continue;
        }

        //PREVENT TOKEN SPAM
        let active_count = file::ACTIVE_FILESHARES.iter().filter(|u| u.client_id == self.id).count();
        if active_count >= config::read_config::<usize>("max_client_parallel_uploads")
        {
            log::warn!("Upload refused ({active_count} already running): {}", self.peer_addr);

            self.send(PacketCode::UploadLimit).await;
            return PacketFlow::Continue;
        }

        let uid = rand::random::<u64>();
        let token = server::open_connection(self.id, ConnectionType::Voice { uid });

        log::info!("Upload request (voice): {}", self.peer_addr);

        self.send(PacketCode::VoiceMessageUpload { hash, token, uid }).await;
        PacketFlow::Continue
    }

    //DOWNLOAD
    async fn download(&mut self, owner_id: usize, file_id: usize) -> PacketFlow
    {
        //FIND USERNAME BY ID
        let username = server::CONNECTIONS.iter()
            .find(|entry| entry.value().id() == Some(&owner_id))
            .and_then(|entry| entry.value().username().cloned());

        //GET USER UPLOADS
        if let Some(username) = username &&
            let Some(file) = server::AVAILABLE_FILES.get(&username).and_then(|f| f.value().get(file_id).cloned())
        {
            //GENERATE RANDOM SHARE UID
            let uid = rand::random::<u64>();

            let file_size = file.size;

            //OPEN NEW CONNECTION
            let token = server::open_connection(self.id, ConnectionType::FileDownload { uid, file });

            //SEND APPROVAL
            self.send(PacketCode::Download { token }).await;

            //LOG START
            log::info!("Download request ({} bytes): {}", file_size, self.peer_addr);
        } else
        {
            log::warn!("Download refused (unknown file): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
        }

        PacketFlow::Continue
    }

    //SCREEN SHARE
    async fn screen(&mut self) -> PacketFlow
    {
        //CHECK FOR DISABLING SCREEN SHARE
        if let Some((Some(removed_id), Some(username))) = server::CONNECTIONS.get_mut(&self.peer_addr)
            .and_then(|mut conn| Some((conn.remove_screen_stream(), conn.username().cloned())))
        {
            //DEATTACH ALL CLIENTS
            server::deattach(removed_id, &username).await;

            log::info!("Screen share stopped: {}", self.peer_addr);

            //SEND SCREEN DISABLE NOTIFICATION
            self.send(PacketCode::Screen { token: None }).await;

            //NOTIFY USERS ABOUT SCREEN
            server::send_to_all(PacketCode::ScreenshareEnd { username: username.clone() });

            return PacketFlow::Continue;
        }

        //CHECK FOR ENABLED SCREENSHARE
        if config::read_config("enable_screenshare")
        {
            //SEND SCREEN ACCEPT
            self.send(PacketCode::Screen
            {
                token: Some(server::open_connection(self.id, ConnectionType::Screen))
            }).await;

            //NOTIFY USERS ABOUT SCREEN
            server::send_to_all(PacketCode::Screenshare { username: self.username.clone() });

            //LOG START
            log::info!("Screen share: {}", self.peer_addr);
        } else
        {
            log::warn!("Screen share refused (disabled): {}", self.peer_addr);

            self.send(PacketCode::InvalidFeature).await;
        }

        PacketFlow::Continue
    }

    //SCREENSHARE ATTACH
    async fn attach(&mut self, sharer_id: usize) -> PacketFlow
    {
        //FIND SHARER ADDRESS BY ID
        let sharer_info = server::CONNECTIONS.iter()
            .find(|entry| entry.value().id() == Some(&sharer_id) && entry.screen_stream().is_some())
            .and_then(|conn| conn.username().map(|u| (sharer_id, u.to_owned())));

        if let Some((sharer_id, sharer_username)) = sharer_info &&
            server::CONNECTIONS.get(&self.peer_addr).is_some_and(|c| c.attached_screen().as_ref()
                .is_none_or(|s| s.target_id != sharer_id)) //PROHIBIT ATTACHING THE SAME SCREEN TWICE
        { //VALID SHARER FOUND
            //OPEN NEW CONNECTION
            let token = server::open_connection(self.id, ConnectionType::Attach
            {
                id: sharer_id,
            });

            //SEND ACCEPT
            self.send(PacketCode::Attach
            {
                username: sharer_username.to_owned(),
                token,
            }).await;

            //TELL THE SHARER WHO IS WATCHING
            server::notify(sharer_id, PacketCode::Attached
            {
                username: self.username.clone(),
            }).await;

            //LOG START
            log::info!("Screen attach: {} watching {}", self.peer_addr, server::log_addr(&sharer_id));
        } else
        {
            log::warn!("Screen attach refused (no such share): {}", self.peer_addr);

            //INVALID ARGS
            self.send(PacketCode::InvalidUsage).await;
        }

        PacketFlow::Continue
    }

    //SCREENSHARE DEATTACH
    async fn deattach_screen(&mut self) -> PacketFlow
    {
        //DEATTACH
        let sharer_id = if let Some(mut conn) = server::CONNECTIONS.get_mut(&self.peer_addr)
        {
            if let Some(target_id) = conn.attached_screen().as_ref().map(|a| a.target_id)
            {
                conn.deattach_screen();
                Some(target_id)
            } else { None }
        } else { None };

        if let Some(sharer_id) = sharer_id
        {
            //FIND SHARER USERNAME
            let sharer_uname = server::CONNECTIONS.iter()
                .find(|c| c.value().id() == Some(&sharer_id))
                .and_then(|c| c.value().username().cloned())
                .unwrap_or_default();

            //SEND ACCEPT
            self.send(PacketCode::Deattach { username: sharer_uname }).await;

            //TELL THE SHARER WHO STOPPED WATCHING
            server::notify(sharer_id, PacketCode::Deattached
            {
                username: self.username.clone(),
            }).await;
        } else
        {
            log::warn!("Deattach refused (not attached): {}", self.peer_addr);

            //NOT ATTACHED
            self.send(PacketCode::InvalidUsage).await;
        }

        PacketFlow::Continue
    }

    //MUTE ATTACHED SCREEN
    async fn mute_screen(&mut self) -> PacketFlow
    {
        //TOGGLE MUTE IF ATTACHED (DROP THE GUARD)
        let muted = server::CONNECTIONS.get_mut(&self.peer_addr)
            .filter(|conn| conn.attached_screen().is_some())
            .map(|mut conn| conn.toggle_mute_screen());

        match muted
        {
            Some(muted) =>
            {
                self.send(PacketCode::MuteScreen { muted }).await;
            },

            None =>
            {
                //NOT ATTACHED
                log::warn!("Screenshare mute refused (not attached): {}", self.peer_addr);
                self.send(PacketCode::InvalidUsage).await;
            },
        }

        PacketFlow::Continue
    }

    //LIST FILES
    async fn files(&mut self) -> PacketFlow
    {
        //GET ALL UPLOADS
        let mut users = Vec::new();
        for entry in server::AVAILABLE_FILES.iter()
        {
            //GET VALUES
            let username = entry.key();
            let uploads = entry.value();

            if !uploads.is_empty() //DO NOT ADD USERS WITH NO UPLOADS
            {
                //GET ID TO THE USERNAME
                let Some(id) = server::CONNECTIONS.iter()
                    .find(|c| c.username().is_some_and(|name| name == username))
                    .and_then(|c| c.id().copied()) else { continue }; //ALREADY GONE

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

        log::debug!("Sending file list ({} users): {}", users.len(), self.peer_addr);

        //SEND LIST BACK TO CLIENT
        self.send(PacketCode::Files { users }).await;
        PacketFlow::Continue
    }

    //AN OLDER PAGE OF THE HISTORY
    async fn history(&mut self, before: u64) -> PacketFlow
    {
        server::send_history(&self.streams.1, &self.keys, Some(before)).await;
        PacketFlow::Continue
    }

    //ONE OF THE HISTORY'S PICTURES
    async fn image_data(&mut self, hash: [u8; 32]) -> PacketFlow
    {
        //ONE REQUEST PER IMAGE_REQUEST_DELAY
        let wait = server::CONNECTIONS.get(&self.peer_addr)
            .and_then(|conn| conn.last_image().map(|last| consts::IMAGE_REQUEST_DELAY
                .saturating_sub(last.elapsed())))
            .unwrap_or_default(); //GUARD DROPPED HERE - NEVER HELD ACROSS THE SLEEP

        if !wait.is_zero()
        {
            log::debug!("Image fetch held for {}ms: {}", wait.as_millis(), self.peer_addr);
            time::sleep(wait).await;
        }

        if let Some(mut conn) = server::CONNECTIONS.get_mut(&self.peer_addr)
            && let Some(last) = conn.last_image_mut() { *last = Instant::now(); }

        //A PICTURE OR A CLIP
        let image = match config::messages::stored(&hash) || config::messages::voice(&hash).is_some()
        {
            true => file::read_image(&hash).await,
            false => None,
        };

        if let Some(data) = image
        {
            log::info!("Image fetch served ({} bytes): {}", data.len(), self.peer_addr);
            self.send(PacketCode::ImageData { hash, data }).await;
        } else
        {
            log::warn!("Image fetch refused (nothing names it): {}", self.peer_addr);

            //AN EMPTY ANSWER SO THE CAPTION STOPS WAITING
            self.send(PacketCode::ImageData
            {
                hash,
                data: Vec::new(),
            }).await;
        }

        PacketFlow::Continue
    }

    //LIST SCREENSHARES
    async fn screens(&mut self) -> PacketFlow
    {
        let mut users = Vec::new();

        //ITERATE OVER server::CONNECTIONS, CREATE LIST OF USERS
        for entry in server::CONNECTIONS.iter()
        {
            let conn = entry.value();

            let Some(username) = conn.username().cloned() else { continue; };
            let Some(id) = conn.id().copied() else { continue; };
            if conn.screen_stream().is_none() { continue; }

            users.push(UserScreen
            {
                username,
                id,
            });
        }

        log::debug!("Sending screenshare list ({} users): {}", users.len(), self.peer_addr);

        //SEND LIST BACK TO CLIENT
        self.send(PacketCode::Screens { users }).await;
        PacketFlow::Continue
    }

    //PRIVATE MESSAGE (OR REPLY)
    async fn private_message(&mut self, recipient: Option<usize>, text: String) -> PacketFlow
    {
        //SILENCE MUTED USERS
        if self.muted()
        {
            self.send(PacketCode::Muted).await;
            return PacketFlow::Continue;
        }

        //GET RECIPIENT
        let recipient_id = match recipient
        {
            Some(id) => id,

            //REPLY
            None =>
            {
                if let Some(recipient_id) = server::CONNECTIONS.get(&self.peer_addr).and_then(|c| c.last_pm())
                {
                    recipient_id
                } else
                {
                    self.send(PacketCode::InvalidUsage).await;
                    return PacketFlow::Continue;
                }
            },
        };

        //FIND RECIPIENT BY ID
        let recipient_addr = server::CONNECTIONS.iter()
            .find(|entry| entry.value().id() == Some(&recipient_id))
            .map(|entry| *entry.key());

        if let Some(recipient_addr) = recipient_addr
        {
            log::info!("Private message ({} chars): {} -> {}",
                text.chars().count(), self.peer_addr, recipient_addr);

            //GET COLORS
            let colors = config::users::colors(&self.username);

            //SEND TO RECIPIENT (IF NOT SELF-MESSAGE)
            if recipient_id != self.id
            {
                let recipient_data = if let Some(mut recipient) =
                    server::CONNECTIONS.get_mut(&recipient_addr)
                {
                    //SAVE ID TO RECIPIENT'S last_pm
                    recipient.set_last_pm(self.id);

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
                        username: self.username.clone(),
                        id: self.id,
                        colors: colors.clone(),
                    }, recipient_keys.as_ref()).await;
                }
            }

            //SEND CONFIRMATION BACK TO SENDER
            let Some(recipient_uname) = server::CONNECTIONS.get(&recipient_addr)
                .and_then(|e| e.username().cloned()) else { return PacketFlow::Continue }; //RECIPIENT LEFT MID-SEND

            //GET TARGET USERNAME COLOR
            let recipient_color = config::users::colors(&recipient_uname).username_color;

            self.send(PacketCode::PrivateMessageBack
            {
                text,
                id: recipient_id,
                username: recipient_uname,
                colors: MessageColors //ADJUST COLORS TO USE TARGET UNAME COLOR
                {
                    username_color: recipient_color,
                    message_color: colors.message_color,
                },
            }).await;
        } else
        {
            log::warn!("Private message refused (no such recipient): {}", self.peer_addr);

            //INVALID PM FORMAT
            self.send(PacketCode::InvalidUsage).await;
        }

        PacketFlow::Continue
    }

    //MESSAGE HEART REACTION
    async fn heart(&mut self, message_id: u64) -> PacketFlow
    {
        //SILENCE MUTED USERS
        if self.muted()
        {
            log::debug!("Message dropped (muted): {}", self.peer_addr);

            self.send(PacketCode::Muted).await;
            return PacketFlow::Continue;
        }

        //TOGGLE A HEART
        let Some(hearts) = config::messages::heart(message_id, &self.username) else
        {
            log::warn!("Heart refused (invalid message): {}", self.peer_addr);

            //SEND InvalidUsage CODE
            self.send(PacketCode::InvalidUsage).await;
            return PacketFlow::Continue;
        };

        //FORWARD TO ALL USERS
        server::send_to_all(PacketCode::Heart { message_id, hearts });
        PacketFlow::Continue
    }

    //TOGGLE A MESSAGE'S PIN
    async fn pin(&mut self, message_id: u64) -> PacketFlow
    {
        //VERIFY PERMISSIONS
        if self.role < Role::Moderator
        {
            log::warn!("Pin refused (permissions): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
            return PacketFlow::Continue;
        }

        //NO SUCH MESSAGE, OR TOO MANY PINS
        let Some(pinned) = config::messages::pin(message_id) else
        {
            log::warn!("Pin refused (no such message, or pin limit): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
            return PacketFlow::Continue;
        };

        log::info!("Message {}: {}", if pinned { "pinned" } else { "unpinned" }, self.peer_addr);

        //EVERY CLIENT HOLDS A LOBBY PANE
        server::send_to_all(PacketCode::Pinned { message_id, pinned });
        PacketFlow::Continue
    }

    //LIST PINNED MESSAGES
    async fn pins(&mut self) -> PacketFlow
    {
        self.send(PacketCode::Pins { messages: config::messages::pinned() }).await;
        PacketFlow::Continue
    }

    //MUTE USER
    async fn server_mute(&mut self, id: usize) -> PacketFlow
    {
        //VERIFY PERMISSIONS
        if self.role < Role::Moderator
        {
            log::warn!("Refused (permissions): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
            return PacketFlow::Continue;
        }

        //FIND TARGET USER
        let muted = if let Some(mut conn) = server::CONNECTIONS.iter_mut()
            .find(|entry| entry.value().id() == Some(&id)) && conn.role() < Some(&self.role)
        {
            //TOGGLE MUTE
            conn.toggle_mute();

            Some((*conn.peer_addr(), *conn.muted()))
        } else { None }; //USER NOT FOUND

        match muted
        {
            //THE GUARD IS DROPPED BEFORE THIS
            Some((target, muted)) => log::info!("{} by {}: {target}",
                if muted { "Mute" } else { "Unmute" }, self.peer_addr),

            None =>
            {
                log::warn!("Mute refused (no such user, or a peer): {}", self.peer_addr);

                self.send(PacketCode::InvalidUsage).await;
            }
        }

        PacketFlow::Continue
    }

    //KICK USER
    async fn server_kick(&mut self, id: usize) -> PacketFlow
    {
        //VERIFY PERMISSIONS
        if self.role < Role::Moderator
        {
            log::warn!("Refused (permissions): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
            return PacketFlow::Continue;
        }

        //FIND TARGET USER
        let target = server::CONNECTIONS.iter()
            .find(|entry| entry.value().id() == Some(&id))
            .map(|entry| (*entry.key(), entry.role().cloned()));

        if let Some((addr, Some(trole))) = target && trole < self.role
        {
            log::info!("Kick by {}: {addr}", self.peer_addr);

            //KICK
            server::remove_connection(&addr, true, Some("kick")).await;
        } else //USER NOT FOUND
        {
            log::warn!("Kick refused (no such user, or a peer): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
        }

        PacketFlow::Continue
    }

    //BAN USER
    async fn server_ban(&mut self, id: usize) -> PacketFlow
    {
        //VERIFY PERMISSIONS
        if self.role < Role::Owner
        {
            log::warn!("Refused (permissions): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
            return PacketFlow::Continue;
        }

        //FIND TARGET ACCOUNT
        if id != self.id && let Some(target) = config::users::username(id)
        {
            let session = server::session_of(id);

            log::info!("Ban by {}: {}", self.peer_addr,
                session.map_or("offline user".to_string(), |addr| addr.to_string()));

            config::bans::ban(&target);

            if let Some(addr) = session
            {
                server::remove_connection(&addr, true, Some("ban")).await;
            }

            //NOTIFY ALL USERS ABOUT THE BAN
            server::send_to_all(PacketCode::Banned { username: target });

            server::send_bans(&self.streams.1, &self.keys).await;
        } else //USER NOT FOUND
        {
            log::warn!("Ban refused (no such user): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
        }

        PacketFlow::Continue
    }

    //BAN USER'S IP
    async fn server_ban_ip(&mut self, uid: usize) -> PacketFlow
    {
        //VERIFY PERMISSIONS
        if self.role < Role::Owner || self.id == uid
        {
            log::warn!("Refused (permissions): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
            return PacketFlow::Continue;
        }

        //FIND TARGET USER
        let target = server::CONNECTIONS.iter()
            .find(|entry| entry.value().id() == Some(&uid))
            .map(|entry| *entry.key());

        if let Some(addr) = target
        {
            log::info!("IP ban by {}: {}", self.peer_addr, addr.ip());

            config::bans::ban_ip(&addr.ip());
            server::remove_connections(&addr.ip(), true, Some("ip ban")).await;
        } else //USER NOT FOUND
        {
            log::warn!("IP ban refused (no such user): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
        }

        PacketFlow::Continue
    }

    //PASSWORD CHANGE
    async fn account_passwd(&mut self, old_password: String, new_password: String) -> PacketFlow
    {
        //ZEROIZE PASSWORDS
        let (old_password, new_password) = (Zeroizing::new(old_password), Zeroizing::new(new_password));

        //VERIFY BOTH PASSWORDS
        let valid = if server::validate_password(&new_password) &&
            server::verify_password(&self.username, old_password).await
        {
            //HASH PASSWORD
            let hash = task::spawn_blocking(move || password::hash_password(&new_password))
                .await.expect("Hashing password failed");

            //SAVE PASSWORD
            config::users::set_password(&self.username, &hash);

            //LOG
            log::info!("Password changed: {}", self.peer_addr);

            true
        } else { false };

        //REPLY TO USER
        self.send(PacketCode::AccountPasswd { valid }).await;
        PacketFlow::Continue
    }

    //ACCOUNT DELETION
    async fn account_delete(&mut self, password: String) -> PacketFlow
    {
        let valid = server::verify_password(&self.username, Zeroizing::new(password)).await;

        //REPLY TO USER
        self.send(PacketCode::AccountDelete { valid }).await;

        if !valid { return PacketFlow::Continue; }

        //DELETE THE ACCOUNT AND ITS AVATAR
        config::users::remove(&self.username);
        config::messages::sweep_images();

        log::info!("Account deleted: {}", self.peer_addr);

        PacketFlow::Disconnect
        {
            grace: true,
            reason: Some("account deleted"),
        }
    }

    //A /color OR /ucolor
    async fn color(&mut self, username_color: bool, color: u8) -> PacketFlow
    {
        log::info!("Color set: {}", self.peer_addr);

        config::users::set_color(&self.username, username_color, color);

        self.send(PacketCode::Color).await;
        PacketFlow::Continue
    }

    //CHECK THE PROFILES FEATURE
    async fn profiles_disabled(&mut self) -> bool
    {
        if config::read_config::<bool>("profiles") { return false; }

        log::warn!("Refused (profiles disabled): {}", self.peer_addr);
        self.send(PacketCode::InvalidFeature).await;

        true
    }

    //A PROFILE, ASKED FOR
    async fn profile_request(&mut self, target: Option<usize>) -> PacketFlow
    {
        //CHECK DISABLED FEATURE
        if self.profiles_disabled().await { return PacketFlow::Continue; }

        //WHOSE PROFILE IS BEING READ
        let target = match target
        {
            Some(id) => match config::users::username(id)
            {
                Some(found) => found,
                None =>
                {
                    log::warn!("Profile refused (no such user): {}", self.peer_addr);

                    self.send(PacketCode::InvalidUsage).await;
                    return PacketFlow::Continue;
                },
            },

            None => self.username.clone(),
        };

        log::info!("Profile read ({}): {}", if target == self.username { "own" } else { "peer" }, self.peer_addr);

        //THE WHOLE PROFILE COMES BACK
        self.send_profile(target, false).await;
        PacketFlow::Continue
    }

    //A PROFILE, WRITTEN
    async fn profile_save(&mut self, profile: UserProfile) -> PacketFlow
    {
        //CHECK DISABLED FEATURE
        if self.profiles_disabled().await { return PacketFlow::Continue; }

        //FIELD LIMITS
        let bio_limit = config::read_config::<usize>("max_profile_bio");
        let field_limit = config::read_config::<usize>("max_profile_field");

        let mut stored = UserProfile::default();
        let mut refused = false;

        //EVERY FIELD IS CLEANED AND CHECKED, AND ONE BAD ONE REFUSES THE LOT
        for (key, value) in UserProfile::KEYS.iter().zip(profile.fields())
        {
            let value = server::clean(value);
            let length = value.chars().count();
            let limit = if *key == "bio" { bio_limit } else { field_limit };

            //CHECK MAX FIELD LENGTH
            if length > limit
            {
                log::warn!("Profile refused ({key}, {length} chars, limit {limit}): {}", self.peer_addr);
                refused = true;
                break;
            }

            //ONLY A LINK A CLIENT MAY OPEN IS STORED AS A WEBSITE
            if *key == "website" && !value.is_empty() && !misc::is_web_url(&value)
            {
                log::warn!("Profile refused ({key}, not a link): {}", self.peer_addr);
                refused = true;
                break;
            }

            if let Some(field) = stored.field_mut(key) { *field = value; }
        }

        //THE ONLINE STATUS IS A TOGGLE, NOT A TYPED FIELD
        stored.online_status = profile.online_status;

        if refused
        {
            self.send(PacketCode::InvalidUsage).await;
        } else
        {
            let length: usize = stored.fields().iter().map(|field| field.chars().count()).sum();
            let was_online = config::users::online_status(&self.username);

            log::info!("Profile saved ({length} chars): {}", self.peer_addr);
            config::users::set_profile(&self.username, &stored);

            //A CHANGE OF VISIBILITY IS A JOIN OR A LEAVE TO EVERYBODY
            if stored.online_status != was_online
            {
                let device = server::CONNECTIONS.get(&self.peer_addr).and_then(|conn| conn.device().clone());

                if stored.online_status
                {
                    server::send_to_all(PacketCode::Join
                    {
                        username: self.username.clone(),
                        username_color: config::users::colors(&self.username).username_color,
                        id: self.id,
                        device,
                        role: self.role,
                    });
                } else
                {
                    server::send_to_all(PacketCode::Leave
                    {
                        username: self.username.clone(),
                        id: self.id,
                        registered: config::users::contains(&self.username),
                    });
                }
            }
        }

        //THE WHOLE PROFILE COMES BACK, SO A REFUSED DESCRIPTION SNAPS BACK
        self.send_profile(self.username.clone(), true).await;
        PacketFlow::Continue
    }

    //SEND A WHOLE PROFILE
    async fn send_profile(&mut self, target: String, save: bool)
    {
        let own = target == self.username;

        self.send(PacketCode::Profile
        {
            profile: config::users::profile(&target),
            username: target,
            own,
            save,
        }).await;
    }

    //THE PROFILE PICTURE, SET OR DROPPED
    async fn avatar(&mut self, hash: Option<[u8; 32]>) -> PacketFlow
    {
        //CHECK DISABLED FEATURE
        if self.profiles_disabled().await { return PacketFlow::Continue; }

        //SILENCE MUTED USERS
        if self.muted()
        {
            self.send(PacketCode::Muted).await;
            return PacketFlow::Continue;
        }

        match hash
        {
            //A PICTURE THE SERVER ALREADY KEEPS COSTS NO UPLOAD
            Some(hash) if config::messages::stored(&hash) =>
            {
                //THE CEILING IS THE CEILING EITHER WAY - A CHAT PICTURE IS ALLOWED TO BE BIGGER
                let size = file::image_size(&hash).await.unwrap_or_default();

                if size > consts::MAX_AVATAR_SIZE as u64
                {
                    log::warn!("Avatar refused ({size} bytes over the {} ceiling): {}",
                        consts::MAX_AVATAR_SIZE, self.peer_addr);

                    self.send(PacketCode::InvalidUsage).await;
                    return PacketFlow::Continue;
                }

                //A CHAT PICTURE IS NOT CUT, SO IT HAS TO BE A SQUARE ALREADY
                if !file::read_image(&hash).await.is_some_and(|image| misc::is_avatar(&image))
                {
                    log::warn!("Avatar refused (not a square of at most {}px): {}",
                        consts::AVATAR_DIMENSION, self.peer_addr);

                    self.send(PacketCode::InvalidUsage).await;
                    return PacketFlow::Continue;
                }

                config::users::set_avatar(&self.username, Some(&hash));
                config::messages::sweep_images();

                log::info!("Avatar set (already stored): {}", self.peer_addr);

                self.send(PacketCode::ImageDuplicate { hash }).await;
            },

            //ANYTHING ELSE IS AN UPLOAD, AND THE PROFILE IS WRITTEN WHEN IT LANDS
            Some(hash) =>
            {
                //PREVENT TOKEN SPAM
                let active_count = file::ACTIVE_FILESHARES.iter().filter(|u| u.client_id == self.id).count();
                if active_count >= config::read_config::<usize>("max_client_parallel_uploads")
                {
                    log::warn!("Upload refused ({active_count} already running): {}", self.peer_addr);

                    self.send(PacketCode::UploadLimit).await;
                    return PacketFlow::Continue;
                }

                let uid = rand::random::<u64>();
                let token = server::open_connection(self.id, ConnectionType::Avatar { uid });

                log::info!("Upload request (avatar): {}", self.peer_addr);

                self.send(PacketCode::Image { hash, token, uid }).await;
                return PacketFlow::Continue;
            },

            //DROPPED, AND SO IS THE FILE IF NOTHING ELSE NAMES IT
            None =>
            {
                config::users::set_avatar(&self.username, None);
                config::messages::sweep_images();

                log::info!("Avatar dropped: {}", self.peer_addr);
            },
        }

        //THE WHOLE PROFILE COMES BACK, LIKE A SAVE'S
        let packet = PacketCode::Profile
        {
            profile: config::users::profile(&self.username),
            username: self.username.clone(),
            own: true,
            save: true,
        };

        self.send(packet).await;
        PacketFlow::Continue
    }

    //THE SERVER'S PICTURE
    async fn server_icon_request(&mut self) -> PacketFlow
    {
        self.send(PacketCode::ServerIcon
        {
            hash: config::server_icon(),
            save: false,
        }).await;

        PacketFlow::Continue
    }

    //THE SERVER'S PICTURE, SET OR DROPPED
    async fn server_icon(&mut self, hash: Option<[u8; 32]>) -> PacketFlow
    {
        //VERIFY PERMISSIONS
        if self.role < Role::Owner
        {
            log::warn!("Refused (permissions): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
            return PacketFlow::Continue;
        }

        match hash
        {
            //A PICTURE THE SERVER ALREADY KEEPS COSTS NO UPLOAD
            Some(hash) if config::messages::stored(&hash) =>
            {
                let size = file::image_size(&hash).await.unwrap_or_default();

                if size > consts::MAX_AVATAR_SIZE as u64
                {
                    log::warn!("Server icon refused ({size} bytes over the {} ceiling): {}",
                        consts::MAX_AVATAR_SIZE, self.peer_addr);

                    self.send(PacketCode::InvalidUsage).await;
                    return PacketFlow::Continue;
                }

                //CUT LIKE AN AVATAR
                if !file::read_image(&hash).await.is_some_and(|image| misc::is_avatar(&image))
                {
                    log::warn!("Server icon refused (not a square of at most {}px): {}",
                        consts::AVATAR_DIMENSION, self.peer_addr);

                    self.send(PacketCode::InvalidUsage).await;
                    return PacketFlow::Continue;
                }

                config::set_server_icon(Some(&hash));
                config::messages::sweep_images();

                log::info!("Server icon set (already stored): {}", self.peer_addr);

                self.send(PacketCode::ImageDuplicate { hash }).await;
            },

            //ANYTHING ELSE IS AN UPLOAD, AND THE ICON IS WRITTEN WHEN IT LANDS
            Some(hash) =>
            {
                //PREVENT TOKEN SPAM
                let active_count = file::ACTIVE_FILESHARES.iter().filter(|u| u.client_id == self.id).count();
                if active_count >= config::read_config::<usize>("max_client_parallel_uploads")
                {
                    log::warn!("Upload refused ({active_count} already running): {}", self.peer_addr);

                    self.send(PacketCode::UploadLimit).await;
                    return PacketFlow::Continue;
                }

                let uid = rand::random::<u64>();
                let token = server::open_connection(self.id, ConnectionType::Icon { uid });

                log::info!("Upload request (icon): {}", self.peer_addr);

                self.send(PacketCode::Image { hash, token, uid }).await;
                return PacketFlow::Continue;
            },

            //DROPPED, AND SO IS THE FILE IF NOTHING ELSE NAMES IT
            None =>
            {
                config::set_server_icon(None);
                config::messages::sweep_images();

                log::info!("Server icon dropped: {}", self.peer_addr);
            },
        }

        server::icon_changed(self.id).await;
        PacketFlow::Continue
    }

    //BAN LIST
    async fn server_bans(&mut self) -> PacketFlow
    {
        //VERIFY PERMISSIONS
        if self.role < Role::Owner
        {
            log::warn!("Refused (permissions): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
            return PacketFlow::Continue;
        }

        server::send_bans(&self.streams.1, &self.keys).await;
        PacketFlow::Continue
    }

    //LIFT A USERNAME BAN
    async fn server_pardon(&mut self, ban: usize) -> PacketFlow
    {
        //VERIFY PERMISSIONS
        if self.role < Role::Owner
        {
            log::warn!("Refused (permissions): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
            return PacketFlow::Continue;
        }

        if config::bans::pardon(ban)
        {
            log::info!("Pardon by {}", self.peer_addr);

            server::send_bans(&self.streams.1, &self.keys).await;
        } else
        {
            log::warn!("Pardon refused (no such ban): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
        }

        PacketFlow::Continue
    }

    //LIFT AN IP BAN
    async fn server_pardon_ip(&mut self, ban: usize) -> PacketFlow
    {
        //VERIFY PERMISSIONS
        if self.role < Role::Owner
        {
            log::warn!("Refused (permissions): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
            return PacketFlow::Continue;
        }

        if config::bans::pardon_ip(ban)
        {
            log::info!("IP pardon by {}", self.peer_addr);

            server::send_bans(&self.streams.1, &self.keys).await;
        } else
        {
            log::warn!("IP pardon refused (no such ban): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
        }

        PacketFlow::Continue
    }

    //SAY AS SERVER
    async fn server_say(&mut self, message: String) -> PacketFlow
    {
        //VERIFY PERMISSIONS
        if self.role < Role::Owner
        {
            log::warn!("Refused (permissions): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
            return PacketFlow::Continue;
        }

        log::info!("Server announcement ({} chars) by {}", message.chars().count(), self.peer_addr);

        //SEND BACK TO ALL CLIENTS ACROSS ALL CHANNELS
        server::send_to_all(PacketCode::ServerSay { message });
        PacketFlow::Continue
    }

    //SET A USER'S ROLE
    async fn server_role(&mut self, id: usize, new_role: Role) -> PacketFlow
    {
        //VERIFY PERMISSIONS
        if self.role < Role::Owner || new_role > self.role
        {
            log::warn!("Refused (permissions): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
            return PacketFlow::Continue;
        }

        //FIND TARGET ACCOUNT
        let target = if id != self.id
        {
            config::users::username(id)
                .and_then(|username| Some((config::users::role(&username)?, username)))
        } else { None };

        //CHECK FOR VALID TARGET
        let Some((target_role, target_username)) = target else
        {
            self.send(PacketCode::InvalidUsage).await;
            return PacketFlow::Continue;
        };

        //A PEER OR A SUPERIOR CANNOT BE AFFECTED
        if target_role >= self.role || target_role == new_role
        {
            self.send(PacketCode::InvalidUsage).await;
            return PacketFlow::Continue;
        }

        log::info!("Role change by {}: {} is now {new_role} (was {target_role})",
            self.peer_addr,
            server::session_of(id).map_or("offline user".to_string(), |addr| addr.to_string()));

        //STORE
        config::users::set_role(&target_username, new_role);

        //APPLY TO THEIR LIVE SESSION
        for mut entry in server::CONNECTIONS.iter_mut()
            .filter(|entry| entry.id() == Some(&id) && entry.role().is_some())
        {
            entry.set_role(new_role);
        }

        //TELL EVERYONE
        server::send_to_all(PacketCode::ServerRole
        {
            username: target_username,
            role: new_role,
        });

        PacketFlow::Continue
    }

    //SERVER CONFIGURATION
    async fn server_settings(&mut self, settings: Option<Vec<ServerSetting>>) -> PacketFlow
    {
        //VERIFY PERMISSIONS
        if self.role < Role::Owner
        {
            log::warn!("Refused (permissions): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
            return PacketFlow::Continue;
        }

        //STORE THE ROWS OF A SAVE
        let save = if let Some(settings) = &settings
        {
            let accepted = config::settings::write(settings);

            log::info!("Server settings saved by {}: {accepted}/{} rows accepted",
                self.peer_addr, settings.len());

            true
        } else
        {
            log::info!("Server settings read by {}", self.peer_addr);

            false
        };

        self.send(PacketCode::ServerSettings
        {
            settings: config::settings::all(),
            save,
        }).await;

        PacketFlow::Continue
    }

    //SERVER RESTART
    async fn server_restart(&mut self) -> PacketFlow
    {
        //VERIFY PERMISSIONS
        if self.role < Role::Owner
        {
            log::warn!("Refused (permissions): {}", self.peer_addr);

            self.send(PacketCode::InvalidUsage).await;
            return PacketFlow::Continue;
        }

        log::info!("Restart requested by {}", self.peer_addr);

        //IN ITS OWN TASK - disconnect_all ABORTS THIS ONE
        tokio::spawn(async
        {
            //SEND EVERYONE OFF FIRST
            server::disconnect_all().await;

            misc::restart();
        });

        PacketFlow::Continue
    }

    //KEEPALIVE
    async fn keepalive(&mut self) -> PacketFlow
    {
        //SET TO ALIVE
        if let Some(mut conn) = server::CONNECTIONS.get_mut(&self.peer_addr)
        {
            conn.set_alive(true);
        }

        PacketFlow::Continue
    }
}
