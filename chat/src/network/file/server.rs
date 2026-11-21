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

//! # File Transfer — Server Side
//!
//! Receives uploads and serves downloads on auxiliary sockets. Function names are from the
//! server's point of view: [`download`] takes a client's upload in, and [`upload`] sends a
//! shared file out.
//!
//! # Upload Pipeline
//! An upload is checked as early as possible and only named once it is verified:
//!
//! 1. The metadata is checked against the client's parallel-upload limit, the kind's size
//!    ceiling and `max_upload_size`.
//! 2. The first chunk of a voice upload must be a voice clip, and an avatar
//!    or icon must be a square no larger than [`AVATAR_DIMENSION`](consts::AVATAR_DIMENSION);
//!    nothing is decoded.
//! 3. Each chunk is encrypted with the file's at-rest [`RexStream`] and written to a temp name;
//!    for a persistent upload the HMAC is fed the ciphertext as written, and the tag is appended
//!    once the last chunk is in.
//! 4. The SHA-256 of the plaintext must match the metadata, and a voice clip must parse.
//! 5. Only then is the file renamed into place: under its hash for a persistent upload, under
//!    its sanitised name for a fileshare.
//!
//! Chat media is delivered to its audience and retained when that conversation allows it.
//! A public fileshare enters the sender’s files list. Avatar and icon uploads update their
//! respective settings.

use std::
{
    fs as sfs,
    ffi::OsStr,
    net::SocketAddr,
    sync::{ Arc, LazyLock },
    path::
    {
        Path,
        PathBuf,
    },
};

use tokio::
{
    sync::Mutex,
    io::AsyncWriteExt,
    net::tcp::OwnedWriteHalf,
    task::{ AbortHandle, Id },
    fs::
    {
        self,
        File,
        OpenOptions,
    },
};

use dashmap::DashMap;

use sha2::{ Sha256, Digest };

use hmac::
{
    Mac,
    Hmac,
};

use why2::
{
    grid::Grid,
    stream::RexStream,
    crypto as core_crypto,
    consts as core_consts,
};

use crate::
{
    config,
    crypto,
    misc,
    consts::{ self, Streams },
    network::
    {
        self,
        EncryptionMode,
        voice::message::{ self as voice_message, Clip },
        codes::
        {
            PacketCode,
            MessageMedia,
            StoredPrivateMessage,
            VoiceNote,
        },
        server::
        {
            self,
            connection::
            {
                AvailableFile,
                DownloadSource,
                TemporaryPrivateFile,
            },
        },
        file::
        {
            self,
            FilePacket,
            FilePacketCode,
            UploadKind,
        },
    },
};

//PRIVATE
//STRUCTS
/// Cleans up a transfer when its task ends, for whatever reason.
///
/// Removes the transfer's stream from the connection and deletes the partial file of an
/// unfinished upload.
struct FileTransferGuard
{
    /// The client's address.
    peer_addr: SocketAddr,
    /// The task of the main connection.
    owner_task: Id,
    /// The upload id.
    uid: u64,
}

//IMPLEMENTATIONS
impl Drop for FileTransferGuard
{
    fn drop(&mut self)
    {
        if let Some(conn) = server::CONNECTIONS.get(&self.peer_addr) && conn.task().id() == self.owner_task
        {
            //REMOVE FILE STREAM
            conn.remove_file_stream(self.uid);
        }
        if let Some((_, active)) = ACTIVE_FILESHARES.remove(&self.uid)
        {
            let _ = sfs::remove_file(&active.path);
        }
    }
}

//PUBLIC
/// The media and destination bound to an upload token.
pub struct UploadTarget
{
    pub kind: UploadKind,
    pub hash: [u8; 32],
    pub private: Option<PrivateDestination>,
}

/// The recipient and retention policy of a private upload.
pub struct PrivateDestination
{
    pub recipient: String,
    pub persistent: bool,
}

/// Checks whether a transient private upload can reach its recipient.
pub fn private_recipient_online(recipient: &str, sender: &str) -> bool
{
    server::private_live_allowed(sender, recipient) &&
        server::CONNECTIONS.iter().any(|conn|
            matches!(conn.value(), server::connection::Connection::Authenticated { username, .. } if username == recipient))
}

/// An upload in progress.
pub struct ActiveFileshare //ACTIVE FILE UPLOAD
{
    /// The file being written.
    pub file: Arc<Mutex<File>>, //TARGET FILE (SERVER-SIDE)
    /// Expected size, from the metadata.
    pub size: u64,              //EXPECTED FILE SIZE
    /// Bytes written so far.
    pub current_size: u64,      //CURRENT SIZE
    /// Expected SHA-256, from the metadata.
    pub hash: [u8; 32],         //SHA256 HASH OF FINAL FILE
    /// SHA-256 of the plaintext received so far.
    pub hasher: Sha256,         //HASHER
    /// The uploaded file's name.
    pub filename: String,       //FILENAME
    /// Peer address of the uploader.
    pub peer_addr: SocketAddr,
    /// The temp path the upload is being built at.
    pub path: PathBuf,          //WHERE THE UPLOAD IS BEING BUILT
    /// The plaintext, kept only for a persistent upload, which is broadcast or parsed afterwards.
    pub data: Option<Vec<u8>>, //THE PLAINTEXT, KEPT ONLY FOR AN ATTACHMENT OR A CLIP
    /// The at-rest stream the file is encrypted with.
    pub stream: RexStream,

    //THE DISK TAG BEING BUILT, FOR A PERSISTENT UPLOAD
    /// The at-rest tag being built, for a persistent upload only.
    pub mac: Option<Hmac<Sha256>>,
}

//LISTS
/// Every upload in progress, by upload id.
pub static ACTIVE_FILESHARES: LazyLock<DashMap<u64, ActiveFileshare>> = LazyLock::new(|| DashMap::new()); //LIST FOR ACTIVE FILE UPLOADS

/// Receives a client's upload on an auxiliary socket.
///
/// See the module documentation for the pipeline. Every rejection ends the transfer; a drop guard
/// removes the partial file.
///
/// # Parameters
/// - `token`: The transfer token the socket presented.
/// - `peer_addr`: The uploader's main connection address.
/// - `streams`: The transfer socket's halves.
/// - `uid`: The upload id.
/// - `task`: The abort handle of this transfer's task, registered on the connection.
/// - `target`: The upload metadata bound to the transfer token.
pub async fn download
(
    token: [u8; 32],
    peer_addr: SocketAddr,
    owner_task: Id,
    streams: &mut Streams<'_>,
    uid: u64,
    task: AbortHandle,
    target: UploadTarget,
)
{
    //GET CLIENT INFO
    let (keys, username, id, shared_files) =
    {
        //FIND THE MAIN CONNECTION
        let conn = server::CONNECTIONS.get(&peer_addr).filter(|conn| conn.task().id() == owner_task);
        match conn
        {
            Some(c) =>
            {
                let keys = match c.keys()
                {
                    Some(k) => k.clone(),
                    None => return
                };

                let username = match c.username()
                {
                    Some(u) => u.clone(),
                    None => return
                };

                let Some(id) = c.id().copied() else { return };
                let Some(shared_files) = c.shared_files().cloned() else { return };

                //ADD FILE STREAM
                c.add_file_stream(uid, task);

                (keys, username, id, shared_files)
            },
            None => return
        }
    };

    //DISCONNECT GUARD
    let _guard = FileTransferGuard
    {
        peer_addr,
        owner_task,
        uid,
    };

    //LOCAL SEQ
    let mut seq = 0usize;

    //INIT REX STREAM
    let mut rex_stream = crypto::init_rex_stream(&keys, &token).unwrap();

    //WAIT FOR FIRST PACKET (METADATA)
    let (size, hash, filename) = match file::receive_file(streams, &mut rex_stream, &mut seq).await
    {
        Some((packet_uid, FilePacketCode::Metadata { size, filename, hash })) if packet_uid == uid => (size, hash, filename),
        _ => return
    };

    if target.hash != hash { return; }
    let UploadTarget { kind, private, .. } = target;

    let sealed = kind.sealed();
    let temporary_file = kind == UploadKind::Attachment &&
        private.as_ref().is_some_and(|target| !target.persistent);

    log::info!("Upload started ({size} bytes, {}): {peer_addr}", kind.name());

    //CHECK FOR CONCURRENT UPLOADS
    if ACTIVE_FILESHARES.iter().filter(|u| u.peer_addr == peer_addr).count() >=
        config::read_config("max_client_parallel_uploads")
    {
        //REJECT INSTEAD OF CONSUMING DATA
        log::warn!("Client reached max parallel uploads: {peer_addr}");
        return;
    }

    //A PICTURE IS ALSO PUSHED TO SOMEBODY ELSE, AND A KEPT ONE IS SMALLER STILL
    let limits = config::upload_limits();
    let ceiling = match kind
    {
        UploadKind::Avatar | UploadKind::Icon => limits.avatar,
        UploadKind::Voice => limits.voice,
        UploadKind::Attachment => limits.attachment,
        UploadKind::File => limits.file,
    };

    if sealed && size > ceiling
    {
        log::warn!("Upload rejected ({}, {size} bytes over the {ceiling} ceiling): {peer_addr}", kind.name());
        server::notify(&peer_addr, PacketCode::InvalidUsage).await;
        return;
    }

    if size > limits.file
    {
        log::warn!("Upload rejected ({size} bytes over the {} byte limit): {peer_addr}", limits.file);
        return;
    }

    //CREATE KEY & NONCE FOR ENCRYPTION ON DISK
    let (disk_key, disk_nonce, disk_mac) = match sealed
    {
        true =>
        {
            let (key, nonce, mac_key) = crypto::attachment_keys(&hash);

            (key, nonce, Some(crypto::disk_mac(&mac_key)))
        },

        false =>
        (
            core_crypto::generate_key::
                <{ core_consts::DEFAULT_GRID_WIDTH }, { core_consts::DEFAULT_GRID_HEIGHT }>(),
            core_crypto::generate_nonce::
                <{ core_consts::DEFAULT_GRID_WIDTH }, { core_consts::DEFAULT_GRID_HEIGHT }>().unwrap().to_flat(),
            None,
        ),
    };

    //WHERE THE UPLOAD IS BUILT, AND WHERE IT STAYS
    let target_dir = match (&private, sealed)
    {
        (Some(_), _) => config::private_messages::media_dir().join("pending"),
        (None, true) => misc::get_attachment_dir(),
        (None, false) => shared_files.directory.clone(),
    };

    //CREATE UPLOAD DIRECTORY
    let created = if sealed
    {
        fs::create_dir_all(&target_dir).await
    } else
    {
        //THE BLOCKING OPERATION HOLDS THE DIRECTORY ALIVE EVEN IF THE UPLOAD IS ABORTED
        let shared_files = shared_files.clone();
        tokio::task::spawn_blocking(move || sfs::create_dir_all(&shared_files.directory))
            .await.expect("Upload directory task failed")
    };

    if let Err(e) = created
    {
        log::error!("Creating upload directory failed ({e}): {peer_addr}");
        return;
    }

    //CREATE REXSTREAM FOR FILE ENCRYPTION ON DISK
    let disk_stream = RexStream::new(&Grid::from_key(&disk_key).unwrap(),
        Grid::from_flat(&disk_nonce).unwrap()).unwrap();

    //CREATE THE FILE, NAMED ONLY ONCE IT IS VERIFIED
    let upload_path = target_dir.join(uid.to_string());
    let upload_file = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&upload_path)
        .await
    {
        Ok(f) => f,
        Err(e) =>
        {
            log::error!("Creating upload file failed ({e}): {peer_addr}");
            return;
        }
    };

    //ADD ACTIVE UPLOAD
    ACTIVE_FILESHARES.insert(uid, ActiveFileshare
    {
        file: Arc::new(Mutex::new(upload_file)),
        size,
        current_size: 0,
        hash,
        hasher: Sha256::new(),
        filename,
        peer_addr,
        path: upload_path,
        data: (sealed && !temporary_file).then(Vec::new),
        stream: disk_stream,
        mac: disk_mac,
    });

    //A FILESHARE IS WHAT THE UPLOADER SAYS IT IS
    let mut checked = matches!(kind, UploadKind::File | UploadKind::Attachment);

    //LOOP READING CHUNKS
    loop
    {
        //READ
        let (uid, data) = match file::receive_file(streams, &mut rex_stream, &mut seq).await
        {
            Some((packet_uid, FilePacketCode::Data { data })) if packet_uid == uid => (uid, data),
            _ => return
        };

        //CHECK THE HEADER FOR A VOICE CLIP, AVATAR OR ICON
        if !checked
        {
            checked = true;

            //CHECK FOR A CLIP
            if kind == UploadKind::Voice
            {
                if !voice_message::is_clip(&data)
                {
                    log::warn!("Voice message rejected (not a clip): {peer_addr}");
                    server::notify(&peer_addr, PacketCode::InvalidUsage).await;
                    return;
                }
            } else if !misc::is_image(&data)
            {
                log::warn!("Avatar or icon rejected (not an image): {peer_addr}");
                server::notify(&peer_addr, PacketCode::InvalidUsage).await;
                return;
            }

            //AN AVATAR IS A SQUARE, CUT BY THE CLIENT
            if matches!(kind, UploadKind::Avatar | UploadKind::Icon) && !misc::is_avatar(&data)
            {
                log::warn!("Upload rejected ({}, not a square of at most {}px): {peer_addr}", kind.name(), consts::AVATAR_DIMENSION);
                server::notify(&peer_addr, PacketCode::InvalidUsage).await;
                return;
            }
        }

        //ENCRYPT CHUNK (NO ENTRY HELD ACROSS AN AWAIT)
        let prepared =
        {
            match ACTIVE_FILESHARES.get_mut(&uid)
            {
                Some(mut active) if active.peer_addr == peer_addr &&
                    data.len() as u64 == (active.size - active.current_size).min(consts::UPLOAD_CHUNK_SIZE as u64) =>
                {
                    //ENCRYPT
                    let input_i64 = crypto::bytes_to_i64(&data);
                    let mut encrypted_i64 = active.stream.update(&input_i64).expect("Disk stream encryption failed");
                    encrypted_i64.extend(active.stream.finalize().expect("Disk stream finalize failed"));
                    let mut encrypted_bytes = crypto::i64_to_bytes(&encrypted_i64);
                    encrypted_bytes.truncate(data.len()); //REMOVE PADDING

                    Some((active.file.clone(), encrypted_bytes))
                },

                _ => None
            }
        };

        //WRITE
        let Some((upload_file, encrypted_bytes)) = prepared else { return };

        if upload_file.lock().await.write_all(&encrypted_bytes).await.is_err() { return; }

        //UPDATE UPLOAD STATE
        let done =
        {
            let mut active = match ACTIVE_FILESHARES.get_mut(&uid)
            {
                Some(a) => a,
                None => return
            };

            //UPDATE SIZE
            active.current_size += data.len() as u64;

            if active.current_size > active.size { return; }

            //UPDATE HASHER
            active.hasher.update(&data);

            //AND THE TAG, OVER WHAT THE DISK TOOK
            if let Some(mac) = active.mac.as_mut() { mac.update(&encrypted_bytes); }

            //KEEP THE ATTACHMENT'S PLAINTEXT FOR THE CHANNEL
            if let Some(buffer) = active.data.as_mut() { buffer.extend_from_slice(&data); }

            //CHECK SIZE
            active.current_size == active.size
        };

        if !done { continue; } //UPLOAD STILL RUNNING

        //UPLOAD DONE, COLLECT FINAL STATE
        let (final_hash, expected_hash, upload_filename, final_size, data, mac) =
        {
            let mut active = match ACTIVE_FILESHARES.get_mut(&uid)
            {
                Some(a) => a,
                None => return
            };

            let final_hash: [u8; 32] = active.hasher.clone().finalize().into();
            (final_hash, active.hash, active.filename.clone(), active.current_size, active.data.take(),
                active.mac.take())
        };

        //THE TAG GOES ON THE END
        if let Some(mac) = mac
        {
            let tag = crypto::disk_tag(mac, final_size);

            if upload_file.lock().await.write_all(&tag).await.is_err() { return; }
        }

        //FLUSH TO DISK BEFORE RENAMING
        if upload_file.lock().await.flush().await.is_err() { return; }

        //CHECK HASHES
        if expected_hash != final_hash
        {
            log::warn!("Upload rejected (hash mismatch): {peer_addr}");
            return;
        }

        //PARSE THE CLIP
        let voice = match kind
        {
            UploadKind::Voice => match data.as_deref().and_then(Clip::decode)
            {
                Some(clip) => Some(VoiceNote { hash: final_hash, duration: clip.duration() }),
                None =>
                {
                    log::warn!("Voice message rejected (malformed clip): {peer_addr}");
                    server::notify(&peer_addr, PacketCode::InvalidUsage).await;
                    return;
                },
            },

            _ => None,
        };

        //GET FILE PATHS
        let current_path = target_dir.join(uid.to_string());

        //GET NEW FILE PATH
        let filename = Path::new(&upload_filename) //PREVENT FROM PATH TRAVERSAL
            .file_name()
            .unwrap_or(OsStr::new("unnamed_file"))
            .to_os_string();

        if matches!(kind, UploadKind::Attachment | UploadKind::Voice)
        {
            let media = voice.map_or(MessageMedia::File(final_hash), MessageMedia::Voice);
            let filename: String = filename.to_string_lossy().chars()
                .filter(|character| !character.is_control()).take(255).collect();
            let filename = if filename.trim().is_empty() { "unnamed_file".to_owned() } else { filename };

            log::info!("Upload done ({final_size} bytes, {}): {peer_addr}", kind.name());
            match private
            {
                Some(target) =>
                {
                    let temporary = temporary_file.then(|| AvailableFile
                    {
                        persistent: true,
                        hash: final_hash,
                        path: current_path.with_extension("shared"),
                        filename: filename.clone(),
                        size: final_size,
                        key: disk_key,
                        nonce: disk_nonce,
                    });

                    publish_private(id, peer_addr, owner_task, &username, target, &filename, media,
                        &current_path, data, temporary).await;
                },

                None =>
                {
                    let channel = server::CONNECTIONS.get(&peer_addr)
                        .and_then(|conn| conn.channel().clone());
                    let kept = keep_public_media(&channel);
                    if kept && fs::rename(&current_path, target_dir.join(misc::hex(&final_hash))).await.is_err()
                    {
                        return;
                    }

                    publish_public(&username, filename, media, data, channel, kept);
                },
            }
            return;
        }

        //SHARED UPLOADS HAVE UNIQUE DISK PATHS EVEN WHEN THEIR DISPLAY NAMES MATCH
        let new_path = match sealed
        {
            true => target_dir.join(misc::hex(&final_hash)),
            false => current_path.with_extension("shared"),
        };

        //RENAME FILE
        if fs::rename(&current_path, &new_path).await.is_err() { return; }

        //LOG FILE UPLOAD
        log::info!("Upload done ({final_size} bytes, {}): {peer_addr}", kind.name());

        let filename = filename.into_string().unwrap_or("unnamed_file".to_string());

        //A PROFILE PICTURE IS NOBODY ELSE'S BUSINESS, AND THE ACCOUNT KEEPS IT
        if kind == UploadKind::Avatar
        {
            //THE PICTURE THIS ONE REPLACES MAY HAVE NOTHING LEFT NAMING IT
            config::users::set_avatar(&username, Some(&final_hash));
            config::messages::sweep_attachments();

            log::info!("Avatar set: {peer_addr}");

            //THE WHOLE PROFILE COMES BACK, LIKE A SAVE'S
            server::notify(&peer_addr, PacketCode::Profile
            {
                profile: config::users::profile(&username),
                username: username.clone(),
                own: true,
                save: true,
            }).await;
        } else if kind == UploadKind::Icon //THE SERVER'S PICTURE, KEPT LIKE AN AVATAR
        {
            config::set_server_icon(Some(&final_hash));
            config::messages::sweep_attachments();

            log::info!("Server icon set: {peer_addr}");

            server::icon_changed(&peer_addr).await;
        } else
        {
            {
                //PUBLISH ONLY TO THE CURRENT SHARED FILE LIST
                let Some(active) = server::AVAILABLE_FILES.get(&username) else { return; };
                if !Arc::ptr_eq(active.value(), &shared_files) { return; }

                shared_files.files.lock().unwrap().push(AvailableFile
                {
                    persistent: false,
                    hash: final_hash,
                    path: new_path,
                    filename: filename.clone(),
                    size: final_size,
                    key: disk_key.clone(),
                    nonce: disk_nonce.clone(),
                });
            }

            //ANNOUNCE FILE UPLOAD
            server::send_to_all(PacketCode::Uploaded
            {
                username: username.clone(),
                filename,
            });
        }

        //REMOVE ACTIVE UPLOAD
        ACTIVE_FILESHARES.remove(&uid);
        return;
    }
}

/// Determines whether public chat media belongs in lobby history.
pub(crate) fn keep_public_media(channel: &Option<String>) -> bool
{
    channel.is_none() && config::read_config::<bool>("persistent_messages")
}

/// Posts a fresh or already stored file or voice note to public chat.
pub(crate) fn publish_public
(
    username: &str,
    filename: String,
    media: MessageMedia,
    data: Option<Vec<u8>>,
    channel: Option<String>,
    kept: bool,
)
{
    let timestamp = config::messages::timestamp();
    let message_id = if kept { config::messages::store_media(username, &filename, &media, timestamp) }
        else { config::messages::next_id() };
    let username_color = config::users::colors(username).username_color;
    let username = username.to_owned();
    let channel = Some(channel);

    let packet = match media
    {
        MessageMedia::File(hash) => PacketCode::AttachmentDisplay
        {
            username,
            filename,
            message_id,
            hash,
            data,
            username_color,
            channel,
            timestamp,
        },

        MessageMedia::Voice(voice) => PacketCode::VoiceMessage
        {
            username,
            message_id,
            voice,
            data,
            username_color,
            channel,
            timestamp,
        },
    };
    server::send_to_all(packet);
}

async fn publish_private
(
    id: usize,
    peer_addr: SocketAddr,
    session: Id,
    sender: &str,
    target: PrivateDestination,
    filename: &str,
    media: MessageMedia,
    pending: &Path,
    data: Option<Vec<u8>>,
    temporary: Option<AvailableFile>,
)
{
    let sender_connected = server::CONNECTIONS.get(&peer_addr).is_some_and(|conn|
        conn.task().id() == session && !*conn.muted());
    if !sender_connected { return; }

    let recipient = &target.recipient;
    if target.persistent && !config::private_messages::media_persistence_enabled()
    {
        server::notify(&peer_addr, PacketCode::InvalidFeature).await;
        return;
    }
    if !target.persistent && !private_recipient_online(recipient, sender)
    {
        server::notify(&peer_addr, PacketCode::PrivateUploadUnavailable).await;
        return;
    }

    let timestamp = config::messages::timestamp();
    let text = if matches!(media, MessageMedia::File(_)) { filename } else { "" };
    let Some(delivery) = config::private_messages::store_media(sender, recipient, text, timestamp,
        media.clone(), pending, target.persistent) else
    {
        server::notify(&peer_addr, PacketCode::InvalidUsage).await;
        return;
    };

    let temporary_file = temporary.is_some();
    if let Some(file) = temporary
    {
        let stored = server::CONNECTIONS.get(&peer_addr).is_some_and(|conn|
        {
            if conn.task().id() != session || *conn.muted() { return false; }
            let Some(files) = conn.private_files() else { return false; };
            if sfs::rename(pending, &file.path).is_err() { return false; }

            files.lock().unwrap().push(TemporaryPrivateFile { participants: delivery.participants.clone(), file });
            true
        });

        if !stored
        {
            server::notify(&peer_addr, PacketCode::InvalidUsage).await;
            return;
        }
    }

    let colors = config::users::colors(sender);
    let sender_id = config::users::id(sender).unwrap_or(id);
    let recipient_id = config::users::id(recipient).unwrap_or_default();
    server::send_to_accounts(&delivery.participants, |account, message_id|
    {
        let sent = account == sender;
        PacketCode::PrivateMedia
        {
            message: StoredPrivateMessage
            {
                message_id,
                before: config::messages::cursor(),
                reply: None,
                hearts: Vec::new(),
                edited: false,
                username: if sent { recipient.to_string() } else { sender.to_string() },
                id: if sent { recipient_id } else { sender_id },
                sent,
                text: text.to_string(),
                media: Some(media.clone()),
                colors: colors.clone(),
                timestamp,
            },
            data: data.clone(),
            temporary_file,
            persistent: delivery.persistent,
        }
    });
}

/// Finds a live private file by the requesting account's local message id.
pub fn private_file(username: &str, message_id: u64, transfer: Option<(u64, AbortHandle)>) -> Option<(AvailableFile, SocketAddr, Id)>
{
    server::CONNECTIONS.iter().find_map(|owner|
    {
        let files = owner.private_files()?.lock().unwrap();
        let entry = files.iter().find(|entry| entry.participants.message_id(username) == Some(message_id))?;
        if let Some((uid, task)) = &transfer
        {
            owner.add_file_stream(*uid, task.clone());
        }

        Some((entry.file.clone(), *owner.key(), owner.task().id()))
    })
}

/// Sends a shared file to a client on an auxiliary socket.
///
/// # Parameters
/// - `token`: The transfer token the socket presented.
/// - `peer_addr`: The downloader's main connection address.
/// - `owner_task`: The task of the main connection when the token was consumed.
/// - `write_stream`: The transfer socket's write half.
/// - `source`: The file or the live PM whose access is checked again on this socket.
/// - `uid`: The transfer id.
/// - `task`: The abort handle of this transfer's task, registered on the connection.
pub async fn upload(token: [u8; 32], peer_addr: SocketAddr, owner_task: Id, mut write_stream: OwnedWriteHalf, source: DownloadSource, uid: u64, task: AbortHandle)
{
    //GET CLIENT INFO
    let (keys, username) =
    {
        //FIND THE MAIN CONNECTION
        let conn = server::CONNECTIONS.get(&peer_addr).filter(|conn| conn.task().id() == owner_task);

        match conn
        {
            Some(c) =>
            {
                let keys = match c.keys()
                {
                    Some(k) => k.clone(),
                    None => return
                };

                //ADD FILE STREAM
                let Some(username) = c.username().cloned() else { return };
                c.add_file_stream(uid, task.clone());

                (keys, username)
            },

            None => return
        }
    };

    //DISCONNECT GUARD
    let _guard = FileTransferGuard
    {
        peer_addr,
        owner_task,
        uid,
    };

    let (file, _owner_guard) = match source
    {
        DownloadSource::File(file) => (file, None),
        DownloadSource::PrivateMessage(message_id) =>
        {
            let Some((file, owner_addr, file_owner_task)) = private_file(&username, message_id, Some((uid, task))) else
            {
                server::notify(&peer_addr, PacketCode::AttachmentUnavailable { message_id }).await;
                return;
            };
            (file, (file_owner_task != owner_task).then(|| FileTransferGuard
            {
                peer_addr: owner_addr,
                owner_task: file_owner_task,
                uid,
            }))
        },
    };

    //INIT SEQ COUNTER
    let mut seq = 0usize;

    //INIT REX STREAM
    let mut rex_stream = crypto::init_rex_stream(&keys, &token).unwrap();

    //OPEN AND VERIFY THE SAME FILE THE TRANSFER WILL READ
    let mut source = match File::open(&file.path).await
    {
        Ok(source) => source,
        Err(error) =>
        {
            log::warn!("Download failed (cannot open file: {error}): {peer_addr}");
            return;
        },
    };

    if file.persistent
    {
        let (_, _, mac_key) = crypto::attachment_keys(&file.hash);

        if crypto::disk_verify(&mut source, &mac_key).await != Some(file.size)
        {
            log::warn!("Download failed (stored file verification): {peer_addr}");
            return;
        }
    }

    //SEND FIRST PACKET (METADATA)
    let _ = network::send_tcp(&mut write_stream, FilePacket
    {
        uid,
        code: FilePacketCode::Metadata
        {
            size: file.size,
            filename: file.filename.clone(),
            hash: file.hash,
        },
        seq: 0,
    }, EncryptionMode::Stream(&mut rex_stream), Some(&mut seq)).await;

    //INIT DISK REX STREAM
    let mut disk_stream = RexStream::new(&Grid::from_key(&file.key).unwrap(), Grid::from_flat(&file.nonce).unwrap()).unwrap();

    log::info!("Download started ({} bytes): {peer_addr}", file.size);

    //START UPLOAD
    file::send_file(source, write_stream, uid, &mut rex_stream, Some(&mut seq), &mut disk_stream, file.size, |_| {}).await;

    //LOG END
    log::info!("Download done: {peer_addr}");
}

//THE PLAINTEXT SIZE OF A STORED ATTACHMENT, TAG TAKEN OFF
/// Returns the plaintext size of a stored attachment or clip, without reading it.
pub async fn attachment_size(hash: &[u8; 32]) -> Option<u64>
{
    sealed_size(&misc::get_attachment_dir().join(misc::hex(hash))).await
}

/// Returns the plaintext size of a sealed file.
pub async fn sealed_size(path: &Path) -> Option<u64>
{
    let sealed = fs::metadata(path).await.ok()?.len();

    Some(sealed.saturating_sub(consts::DISK_TAG_SIZE as u64))
}

//READ ONE STORED ATTACHMENT BACK OFF DISK
/// Reads one stored attachment or clip back off disk, verifying it.
///
/// The file is decrypted in [`UPLOAD_CHUNK_SIZE`](consts::UPLOAD_CHUNK_SIZE) chunks, finalizing
/// the stream after each, to mirror how it was written.
///
/// # Returns
/// - `Some(bytes)` with the original file.
/// - `None` if it is missing or fails verification.
pub async fn read_attachment(hash: &[u8; 32]) -> Option<Vec<u8>>
{
    read_sealed_attachment(&misc::get_attachment_dir().join(misc::hex(hash)), hash).await
}

/// Reads a sealed file after its caller has authorized the path.
pub async fn read_sealed_attachment(path: &Path, hash: &[u8; 32]) -> Option<Vec<u8>>
{
    let sealed = match fs::read(path).await
    {
        Ok(bytes) => bytes,
        Err(error) =>
        {
            log::warn!("Stored attachment unreadable: {error}");
            return None;
        }
    };

    let (key, nonce, mac_key) = crypto::attachment_keys(hash);

    let Some(ciphertext) = crypto::disk_open(&mac_key, &sealed) else
    {
        log::error!("Stored attachment failed verification");
        return None;
    };

    let mut disk_stream: RexStream = RexStream::new(&Grid::from_key(&key).ok()?, Grid::from_flat(&nonce).ok()?).ok()?;

    //THE READ HAS TO MIRROR THE WRITE CHUNK FOR CHUNK
    let mut data = Vec::with_capacity(ciphertext.len());

    for chunk in ciphertext.chunks(consts::UPLOAD_CHUNK_SIZE)
    {
        let mut decrypted = disk_stream.update(&crypto::bytes_to_i64(chunk)).ok()?;
        decrypted.extend(disk_stream.finalize().ok()?);

        let mut bytes = crypto::i64_to_bytes(&decrypted);
        bytes.truncate(chunk.len());

        data.extend_from_slice(&bytes);
    }

    Some(data)
}
