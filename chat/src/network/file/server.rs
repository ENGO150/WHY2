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
//! 2. The first chunk of a persistent upload must be an image (or a voice clip), and an avatar
//!    or icon must be a square no larger than [`AVATAR_DIMENSION`](consts::AVATAR_DIMENSION);
//!    nothing is decoded.
//! 3. Each chunk is encrypted with the file's at-rest [`RexStream`] and written to a temp name;
//!    for a persistent upload the HMAC is fed the ciphertext as written, and the tag is appended
//!    once the last chunk is in.
//! 4. The SHA-256 of the plaintext must match the metadata, and a voice clip must parse.
//! 5. Only then is the file renamed into place: under its hash for a persistent upload, under
//!    its sanitised name for a fileshare.
//!
//! A persistent picture or clip is then stored in the history (lobby only, with
//! `persistent_messages` on) and broadcast whole; an avatar or icon is answered with the
//! updated profile or icon instead.
//!
//! # Encryption Migration
//! Files written by the previous release (core 1.5.2/1.5.3) still verify with the same HMAC key,
//! but decrypt to garbage with the current keystream. [`migrate_images`] runs once at startup and
//! rewrites every stored picture, avatar, server icon and voice message it identifies with the
//! current encryption. The file name is the SHA-256 of its plaintext, so that is what confirms an
//! old file. Files in formats this server no longer recognises are left untouched.

use std::
{
    sync::Arc,
    ffi::OsStr,
    path::
    {
        Path,
        PathBuf,
    },
    sync::LazyLock,
};

use tokio::
{
    sync::Mutex,
    task::AbortHandle,
    io::
    {
        AsyncReadExt,
        AsyncWriteExt,
    },
    net::tcp::OwnedWriteHalf,
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
            VoiceNote,
        },
        server::
        {
            self,
            connection::AvailableFile,
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
    /// The client's session id.
    id: usize,
    /// The upload id.
    uid: u64,
}

//IMPLEMENTATIONS
impl Drop for FileTransferGuard
{
    fn drop(&mut self)
    {
        if let Some(conn) = server::CONNECTIONS.iter().find(|c| c.id() == Some(&self.id))
        {
            //REMOVE FILE STREAM
            conn.remove_file_stream(self.uid);

            //REMOVE JUNK FILE
            if let Some((_, active)) = ACTIVE_FILESHARES.remove(&self.uid)
            {
                let _ = std::fs::remove_file(&active.path);

                log::warn!("Upload failed at {}/{} bytes: {}", active.current_size, active.size, conn.peer_addr());
            }
        }
    }
}

//PUBLIC
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
    /// Session id of the uploader.
    pub client_id: usize,       //ID OF SENDER
    /// The temp path the upload is being built at.
    pub path: PathBuf,          //WHERE THE UPLOAD IS BEING BUILT
    /// The plaintext, kept only for a persistent upload, which is broadcast or parsed afterwards.
    pub image: Option<Vec<u8>>, //THE PLAINTEXT, KEPT ONLY FOR AN IMAGE OR A CLIP
    /// The at-rest stream the file is encrypted with.
    pub stream: RexStream,

    //THE DISK TAG BEING BUILT, FOR AN IMAGE ONLY
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
/// - `id`: The uploader's session id.
/// - `streams`: The transfer socket's halves.
/// - `uid`: The upload id.
/// - `task`: The abort handle of this transfer's task, registered on the connection.
/// - `kind`: What the upload is for.
pub async fn download
(
    token: [u8; 32],
    id: usize,
    streams: &mut Streams<'_>,
    uid: u64,
    task: AbortHandle,
    kind: UploadKind,
)
{
    //GET CLIENT INFO
    let (keys, username, peer_addr) =
    {
        //FIND CONNECTION BY ID
        let conn = server::CONNECTIONS.iter()
            .find(|e| e.value().id() == Some(&id));

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

                //ADD FILE STREAM
                c.add_file_stream(uid, task);

                (keys, username, c.peer_addr().clone())
            },
            None => return
        }
    };

    //DISCONNECT GUARD
    let _guard = FileTransferGuard
    {
        id,
        uid,
    };

    //LOCAL SEQ
    let mut seq = 0usize;

    //INIT REX STREAM
    let mut rex_stream = crypto::init_rex_stream(&keys, &token).unwrap();

    //WAIT FOR FIRST PACKET (METADATA)
    let (size, hash, filename) = match file::receive_file(streams, &mut rex_stream, &mut seq).await
    {
        Some((_, FilePacketCode::Metadata { size, filename, hash })) => (size, hash, filename),
        _ => return
    };

    let persistent = kind.persistent();

    log::info!("Upload started ({size} bytes, {}): {peer_addr}", kind.name());

    let mut valid = false;

    //CHECK FOR CONCURRENT UPLOADS
    if ACTIVE_FILESHARES.iter().filter(|u| u.client_id == id).count() >=
        config::read_config("max_client_parallel_uploads")
    {
        //REJECT INSTEAD OF CONSUMING DATA
        log::warn!("Client reached max parallel uploads: {peer_addr}");
        return;
    }

    //A PICTURE IS ALSO PUSHED TO SOMEBODY ELSE, AND A KEPT ONE IS SMALLER STILL
    let ceiling = match kind
    {
        UploadKind::Avatar | UploadKind::Icon => consts::MAX_AVATAR_SIZE,
        UploadKind::Voice => consts::MAX_VOICE_MESSAGE_SIZE,
        _ => consts::MAX_IMAGE_SIZE,
    };

    if persistent && size > ceiling as u64
    {
        log::warn!("Upload rejected ({}, {size} bytes over the {ceiling} ceiling): {peer_addr}", kind.name());
        server::notify(id, PacketCode::InvalidUsage).await;
        return;
    }

    //CREATE KEY & NONCE FOR ENCRYPTION ON DISK
    let (disk_key, disk_nonce, disk_mac) = match persistent
    {
        true =>
        {
            let (key, nonce, mac_key) = crypto::image_keys(&hash);

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
    let target_dir = match persistent
    {
        true => misc::get_image_dir(),
        false => misc::get_upload_dir(&username),
    };

    if !valid && size / consts::MEGABYTE as u64 <= config::read_config::<u64>("max_upload_size")
    {
        //CREATE UPLOAD DIRECTORY
        if let Err(e) = fs::create_dir_all(&target_dir).await
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
            client_id: id,
            path: upload_path,
            image: persistent.then(|| Vec::with_capacity(size as usize)),
            stream: disk_stream,
            mac: disk_mac,
        });

        valid = true;
    }

    if !valid
    {
        //LOG FILE REJECT
        log::warn!("Upload rejected ({size} bytes over the {}MB limit): {peer_addr}",
            config::read_config::<u64>("max_upload_size"));
        return;
    }

    //A FILESHARE IS WHAT THE UPLOADER SAYS IT IS
    let mut checked = !persistent;

    //LOOP READING CHUNKS
    loop
    {
        //READ
        let (uid, data) = match file::receive_file(streams, &mut rex_stream, &mut seq).await
        {
            Some((uid, FilePacketCode::Data { data })) => (uid, data),
            _ => return
        };

        //CHECK FOR VALID IMAGE ON PERSISTENT UPLOADS
        if !checked
        {
            checked = true;

            //CHECK FOR A CLIP
            if kind == UploadKind::Voice
            {
                if !voice_message::is_clip(&data)
                {
                    log::warn!("Voice message rejected (not a clip): {peer_addr}");
                    server::notify(id, PacketCode::InvalidUsage).await;
                    return;
                }
            } else if !misc::is_image(&data)
            {
                log::warn!("Image rejected (not an image): {peer_addr}");
                server::notify(id, PacketCode::InvalidUsage).await;
                return;
            }

            //AN AVATAR IS A SQUARE, CUT BY THE CLIENT
            if matches!(kind, UploadKind::Avatar | UploadKind::Icon) && !misc::is_avatar(&data)
            {
                log::warn!("Upload rejected ({}, not a square of at most {}px): {peer_addr}", kind.name(), consts::AVATAR_DIMENSION);
                server::notify(id, PacketCode::InvalidUsage).await;
                return;
            }
        }

        //ENCRYPT CHUNK (NO ENTRY HELD ACROSS AN AWAIT)
        let prepared =
        {
            match ACTIVE_FILESHARES.get_mut(&uid)
            {
                Some(mut active) if active.client_id == id && data.len() <= consts::UPLOAD_CHUNK_SIZE =>
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
        let (upload_file, encrypted_bytes) = match prepared
        {
            Some(p) => p,
            None => continue
        };

        if upload_file.lock().await.write_all(&encrypted_bytes).await.is_err() { continue; }

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

            //KEEP AN IMAGE'S PLAINTEXT FOR THE CHANNEL
            if let Some(buffer) = active.image.as_mut() { buffer.extend_from_slice(&data); }

            //CHECK SIZE
            active.current_size == active.size
        };

        if !done { continue; } //UPLOAD STILL RUNNING

        //UPLOAD DONE, COLLECT FINAL STATE
        let (final_hash, expected_hash, upload_filename, final_size, image, mac) =
        {
            let mut active = match ACTIVE_FILESHARES.get_mut(&uid)
            {
                Some(a) => a,
                None => return
            };

            let final_hash: [u8; 32] = active.hasher.clone().finalize().into();
            (final_hash, active.hash, active.filename.clone(), active.current_size, active.image.take(),
                active.mac.take())
        };

        //THE TAG GOES ON THE END
        if let Some(mac) = mac
        {
            let tag = crypto::disk_tag(mac, final_size);

            if upload_file.lock().await.write_all(&tag).await.is_err() { return; }
        }

        //FLUSH TO DISK BEFORE RENAMING
        upload_file.lock().await.flush().await.ok();

        //CHECK HASHES
        if expected_hash != final_hash
        {
            log::warn!("Upload rejected (hash mismatch): {peer_addr}");
            return;
        }

        //PARSE THE CLIP
        let voice = match kind
        {
            UploadKind::Voice => match image.as_deref().and_then(Clip::decode)
            {
                Some(clip) => Some(VoiceNote { hash: final_hash, duration: clip.duration() }),
                None =>
                {
                    log::warn!("Voice message rejected (malformed clip): {peer_addr}");
                    server::notify(id, PacketCode::InvalidUsage).await;
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

        //AN IMAGE IS NAMED AFTER ITS CONTENT INSTEAD
        let new_path = match persistent
        {
            true => target_dir.join(misc::hex(&final_hash)),
            false => target_dir.join(&filename),
        };

        //RENAME FILE
        let insert = !fs::try_exists(&new_path).await.unwrap_or(false);
        if fs::rename(&current_path, &new_path).await.is_err() { return; }

        //LOG FILE UPLOAD
        log::info!("Upload done ({final_size} bytes, {}): {peer_addr}", kind.name());

        let filename = filename.into_string().unwrap_or("unnamed_file".to_string());

        //A PROFILE PICTURE IS NOBODY ELSE'S BUSINESS, AND THE ACCOUNT KEEPS IT
        if kind == UploadKind::Avatar
        {
            //THE PICTURE THIS ONE REPLACES MAY HAVE NOTHING LEFT NAMING IT
            config::users::set_avatar(&username, Some(&final_hash));
            config::messages::sweep_images();

            log::info!("Avatar set: {peer_addr}");

            //THE WHOLE PROFILE COMES BACK, LIKE A SAVE'S
            server::notify(id, PacketCode::Profile
            {
                profile: config::users::profile(&username),
                username: username.clone(),
                own: true,
                save: true,
            }).await;
        } else if kind == UploadKind::Icon //THE SERVER'S PICTURE, KEPT LIKE AN AVATAR
        {
            config::set_server_icon(Some(&final_hash));
            config::messages::sweep_images();

            log::info!("Server icon set: {peer_addr}");

            server::icon_changed(id).await;
        } else if let Some(voice) = voice //A VOICE MESSAGE
        {
            if let Some(data) = image
            {
                let channel = server::CONNECTIONS.iter()
                    .find(|conn| conn.id() == Some(&id))
                    .and_then(|conn| conn.channel().clone());

                let kept = channel.is_none() && config::read_config::<bool>("persistent_messages");

                let timestamp = config::messages::timestamp();
                let message_id = match kept
                {
                    true => config::messages::store_voice(&username, &voice, timestamp),
                    false => config::messages::next_id(),
                };

                //DELETE A CLIP NOTHING KEPT
                if !kept && insert { let _ = fs::remove_file(&new_path).await; }

                //A FRESH CLIP GOES OUT WHOLE
                server::send_to_all(PacketCode::VoiceMessage
                {
                    username: username.clone(),
                    message_id,
                    voice,
                    data: Some(data),
                    username_color: config::users::colors(&username).username_color,
                    channel: Some(channel.clone()),
                    timestamp,
                });
            }
        } else if persistent //AN IMAGE IS SHOWN, NOT ANNOUNCED
        {
            if let Some(data) = image
            {
                let channel = server::CONNECTIONS.iter()
                    .find(|conn| conn.id() == Some(&id))
                    .and_then(|conn| conn.channel().clone());

                //KEEP IT, ON THE SAME TERMS AS A MESSAGE
                let kept = channel.is_none() && config::read_config::<bool>("persistent_messages");

                let timestamp = config::messages::timestamp();
                let message_id = match kept
                {
                    true => config::messages::store_image(&username, &filename, &final_hash, timestamp),
                    false => config::messages::next_id(),
                };

                //DELETE A PICTURE NOTHING KEPT
                if !kept && insert { let _ = fs::remove_file(&new_path).await; }

                //A FRESH PICTURE GOES OUT WHOLE
                server::send_to_all(PacketCode::ImageDisplay
                {
                    username: username.clone(),
                    filename,
                    message_id,
                    hash: final_hash,
                    data: Some(data),
                    username_color: config::users::colors(&username).username_color,
                    channel: Some(channel.clone()),
                    timestamp,
                });
            }
        } else
        {
            //ANNOUNCE FILE UPLOAD
            server::send_to_all(PacketCode::Uploaded
            {
                username: username.clone(),
                filename: filename.clone(),
            });

            if insert
            {
                //ADD FILE TO AVAILABLE FILES
                server::AVAILABLE_FILES.get_mut(username.as_str()).unwrap().push(AvailableFile
                {
                    hash: final_hash,
                    path: new_path,
                    filename,
                    size: final_size,
                    key: disk_key.clone(),
                    nonce: disk_nonce.clone(),
                });
            }
        }

        //REMOVE ACTIVE UPLOAD
        ACTIVE_FILESHARES.remove(&uid);
        return;
    }
}

/// Sends a shared file to a client on an auxiliary socket.
///
/// # Parameters
/// - `token`: The transfer token the socket presented.
/// - `id`: The downloader's session id.
/// - `write_stream`: The transfer socket's write half.
/// - `file`: The shared file, with its at-rest key and nonce.
/// - `uid`: The transfer id.
/// - `task`: The abort handle of this transfer's task, registered on the connection.
pub async fn upload(token: [u8; 32], id: usize, mut write_stream: OwnedWriteHalf, file: AvailableFile, uid: u64, task: AbortHandle)
{
    //GET CLIENT INFO
    let (keys, peer_addr) =
    {
        //FIND CONNECTION BY ID
        let conn = server::CONNECTIONS.iter()
            .find(|e| e.value().id() == Some(&id));

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
                c.add_file_stream(uid, task);

                (keys, c.peer_addr().clone())
            },

            None => return
        }
    };

    //DISCONNECT GUARD
    let _guard = FileTransferGuard
    {
        id,
        uid,
    };

    //INIT SEQ COUNTER
    let mut seq = 0usize;

    //INIT REX STREAM
    let mut rex_stream = crypto::init_rex_stream(&keys, &token).unwrap();

    //SEND FIRST PACKET (METADATA)
    network::send_tcp(&mut write_stream, FilePacket
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
    file::send_file(file.path, write_stream, uid, &mut rex_stream, Some(&mut seq), &mut disk_stream, |_| {}).await;

    //LOG END
    log::info!("Download done: {peer_addr}");
}

//THE PLAINTEXT SIZE OF A STORED PICTURE, TAG TAKEN OFF
/// Returns the plaintext size of a stored picture or clip, without reading it.
pub async fn image_size(hash: &[u8; 32]) -> Option<u64>
{
    let sealed = fs::metadata(misc::get_image_dir().join(misc::hex(hash))).await.ok()?.len();

    Some(sealed.saturating_sub(consts::DISK_TAG_SIZE as u64))
}

//READ ONE STORED IMAGE BACK OFF DISK
/// Reads one stored picture or clip back off disk, verifying it.
///
/// The file is decrypted in [`UPLOAD_CHUNK_SIZE`](consts::UPLOAD_CHUNK_SIZE) chunks, finalizing
/// the stream after each, to mirror how it was written.
///
/// # Returns
/// - `Some(bytes)` with the original file.
/// - `None` if it is missing or fails verification.
pub async fn read_image(hash: &[u8; 32]) -> Option<Vec<u8>>
{
    let sealed = match fs::read(misc::get_image_dir().join(misc::hex(hash))).await
    {
        Ok(bytes) => bytes,
        Err(error) =>
        {
            log::warn!("Stored image unreadable: {error}");
            return None;
        }
    };

    let (key, nonce, mac_key) = crypto::image_keys(hash);

    let Some(ciphertext) = crypto::disk_open(&mac_key, &sealed) else
    {
        log::error!("Stored image failed verification, it is not the picture it is named after");
        return None;
    };

    //THE READ HAS TO MIRROR THE WRITE CHUNK FOR CHUNK
    crypto::apply_file_cipher(&key, &nonce, ciphertext, consts::UPLOAD_CHUNK_SIZE)
}

//REWRITE ONE STORED FILE FROM THE PREVIOUS-RELEASE KEYSTREAM
/// Number of leading bytes [`migrate_image`] reads to tell the two encryptions apart. Every
/// image and clip marker fits well inside it.
const MIGRATION_PROBE_SIZE: u64 = 64;

/// What the leading bytes of a stored file say about its encryption.
#[derive(Clone, Copy)]
enum StoredEncryption
{
    /// Only the current keystream yields a known image or clip marker.
    Current,
    /// Only the previous release's keystream does.
    Legacy,
    /// Both do, because a short marker matched by chance; the file has to be read whole.
    Ambiguous,
    /// Neither does: not a picture or clip this server wrote.
    Unknown,
}

/// Probes the leading bytes of a stored file with both keystreams.
///
/// A valid file starts with an image or clip marker, so the keystream that produces one is the
/// one it was written with. Short markers (`BM`, `#?`, `P1` and friends) can match by chance, so
/// both matching is reported as [`StoredEncryption::Ambiguous`] rather than guessed.
///
/// # Parameters
/// - `key`, `nonce`: The file's at-rest keys.
/// - `probe`: The first ciphertext bytes of the file.
fn probe_encryption(key: &[i64], nonce: &[i64], probe: &[u8]) -> StoredEncryption
{
    let marker = |plaintext: Option<Vec<u8>>|
        plaintext.is_some_and(|bytes| misc::is_image(&bytes) || voice_message::is_clip(&bytes));

    let current = marker(crypto::apply_file_cipher(key, nonce, probe, probe.len()));
    let legacy = marker(crypto::apply_file_cipher_legacy(key, nonce, probe, probe.len()));

    match (current, legacy)
    {
        (true, false) => StoredEncryption::Current,
        (false, true) => StoredEncryption::Legacy,
        (true, true) => StoredEncryption::Ambiguous,
        (false, false) => StoredEncryption::Unknown,
    }
}

/// Migrates one stored picture or voice message from the previous release, if it is old.
///
/// The file name is the SHA-256 of the plaintext, so a successful decryption can be checked
/// against it. That is what confirms a previous-release file before it is rewritten. A current
/// file is recognised more cheaply: its leading bytes decrypt to a known image or clip marker,
/// while the same bytes under the previous release's keystream are noise. Only a file that looks
/// old, or whose short marker matches under both keystreams, is read whole.
///
/// # Returns
/// `true` if the file was rewritten.
async fn migrate_image(path: &Path, hash: &[u8; 32]) -> bool //REWRITE ONE PREVIOUS-RELEASE FILE
{
    let (key, nonce, mac_key) = crypto::image_keys(hash);

    //READ ONLY THE MARKER FOR THE PROBE
    let mut probe = Vec::with_capacity(MIGRATION_PROBE_SIZE as usize);

    let read = match File::open(path).await
    {
        Ok(file) => file.take(MIGRATION_PROBE_SIZE).read_to_end(&mut probe).await,

        Err(error) =>
        {
            log::warn!("Stored file unreadable during migration ({error}): {}", path.display());
            return false;
        }
    };

    if let Err(error) = read
    {
        log::warn!("Stored file unreadable during migration ({error}): {}", path.display());
        return false;
    }

    let encryption = probe_encryption(&key, &nonce, &probe);

    //CURRENT (OR NOT OURS) - NOTHING TO DO
    if matches!(encryption, StoredEncryption::Current | StoredEncryption::Unknown) { return false; }

    //PREVIOUS RELEASE: READ THE WHOLE FILE, VERIFY IT AND REWRITE IT
    let sealed = match fs::read(path).await
    {
        Ok(bytes) => bytes,

        Err(error) =>
        {
            log::warn!("Stored file unreadable during migration ({error}): {}", path.display());
            return false;
        }
    };

    let Some(ciphertext) = crypto::disk_open(&mac_key, &sealed) else
    {
        log::warn!("Stored file failed verification during migration: {}", path.display());
        return false;
    };

    //A SHORT MARKER MAY HAVE MATCHED UNDER BOTH KEYSTREAMS - THE HASH SETTLES IT
    if matches!(encryption, StoredEncryption::Ambiguous)
        && crypto::apply_file_cipher(&key, &nonce, ciphertext, consts::UPLOAD_CHUNK_SIZE)
            .is_some_and(|plaintext| crypto::sha256(&plaintext) == *hash)
    {
        return false;
    }

    //PREVIOUS RELEASE ENCRYPTION
    let Some(plaintext) = crypto::apply_file_cipher_legacy(&key, &nonce, ciphertext, consts::UPLOAD_CHUNK_SIZE) else
    {
        log::warn!("Stored file could not be decrypted during migration: {}", path.display());
        return false;
    };

    if crypto::sha256(&plaintext) != *hash
    {
        log::warn!("Stored file is neither current nor from the previous release: {}", path.display());
        return false;
    }

    //SEAL IT AGAIN WITH THE CURRENT ENCRYPTION
    let Some(ciphertext) = crypto::apply_file_cipher(&key, &nonce, &plaintext, consts::UPLOAD_CHUNK_SIZE) else
    {
        log::warn!("Stored file could not be re-encrypted during migration: {}", path.display());
        return false;
    };

    let mut rewritten = ciphertext;
    let tag = crypto::disk_seal(&mac_key, &rewritten);
    rewritten.extend_from_slice(&tag);

    //REPLACE IT ONLY ONCE THE WHOLE FILE IS READY
    let temporary = path.with_extension("migrating");

    if let Err(error) = fs::write(&temporary, &rewritten).await
    {
        log::error!("Writing migrated file failed ({error}): {}", path.display());
        let _ = fs::remove_file(&temporary).await;
        return false;
    }

    if let Err(error) = fs::rename(&temporary, path).await
    {
        log::error!("Replacing migrated file failed ({error}): {}", path.display());
        let _ = fs::remove_file(&temporary).await;
        return false;
    }

    true
}

/// Rewrites every stored picture, avatar, server icon and voice message written by the previous
/// release with the current encryption.
///
/// Runs at startup, after the orphan sweep and before clients are accepted. A file that is
/// already current, or that cannot be identified, is left untouched.
pub async fn migrate_images() //MIGRATE THE PERSISTENT PICTURE STORE
{
    let Ok(mut entries) = fs::read_dir(misc::get_image_dir()).await else { return };

    let mut migrated = 0;

    while let Ok(Some(entry)) = entries.next_entry().await
    {
        if !entry.file_type().await.map(|kind| kind.is_file()).unwrap_or(false) { continue; }

        let path = entry.path();

        let Some(hash) = path.file_name()
            .and_then(OsStr::to_str)
            .and_then(misc::unhex)
        else { continue };

        if migrate_image(&path, &hash).await { migrated += 1; }
    }

    if migrated > 0
    {
        log::info!("Migrated {migrated} stored images and voice messages to the current encryption");
    }
}
