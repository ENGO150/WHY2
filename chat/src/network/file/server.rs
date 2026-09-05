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
    io::AsyncWriteExt,
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
        codes::PacketCode,
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
        },
    },
};

//PRIVATE
//STRUCTS
struct FileTransferGuard
{
    id: usize,
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

            //REMOVE JUNK FILE - THE UPLOAD CARRIES ITS OWN PATH, SINCE AN IMAGE IS NOT BUILT IN THE TEMP DIR
            if let Some((_, active)) = ACTIVE_FILESHARES.remove(&self.uid)
            {
                let _ = std::fs::remove_file(&active.path);
                log::error!("Upload failed: {}", conn.peer_addr());
            }
        }
    }
}

//PUBLIC
pub struct ActiveFileshare //ACTIVE FILE UPLOAD
{
    pub file: Arc<Mutex<File>>, //TARGET FILE (SERVER-SIDE)
    pub size: u64,              //EXPECTED FILE SIZE
    pub current_size: u64,      //CURRENT SIZE
    pub hash: [u8; 32],         //SHA256 HASH OF FINAL FILE
    pub hasher: Sha256,         //HASHER
    pub filename: String,       //FILENAME
    pub client_id: usize,       //ID OF SENDER
    pub path: PathBuf,          //WHERE THE UPLOAD IS BEING BUILT
    pub image: Option<Vec<u8>>, //THE PLAINTEXT, KEPT ONLY FOR AN IMAGE - IT IS SENT ON WHEN IT IS WHOLE
    pub stream: RexStream,
}

//LISTS
pub static ACTIVE_FILESHARES: LazyLock<DashMap<u64, ActiveFileshare>> = LazyLock::new(|| DashMap::new()); //LIST FOR ACTIVE FILE UPLOADS

pub async fn download(token: [u8; 32], id: usize, streams: &mut Streams<'_>, uid: u64, task: AbortHandle, persistent: bool)
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

    let mut valid = false;

    //CHECK FOR CONCURRENT UPLOADS
    if ACTIVE_FILESHARES.iter().filter(|u| u.client_id == id).count() >=
        config::read_config("max_client_parallel_uploads")
    {
        //REJECT INSTEAD OF CONSUMING DATA
        log::warn!("Client reached max parallel uploads: {peer_addr}");
        return;
    }

    //AN IMAGE IS NOT ONLY STORED, IT IS PUSHED TO EVERY CLIENT IN THE CHANNEL AS ONE PACKET
    if persistent && size > consts::MAX_IMAGE_SIZE as u64
    {
        log::info!("Image rejected (too large): {peer_addr}");
        server::notify(id, PacketCode::InvalidUsage).await;
        return;
    }

    //CREATE KEY & NONCE FOR FILE ENCRYPTION ON DISK. A FILESHARE KEEPS ITS RANDOM PAIR IN
    //AVAILABLE_FILES AND DIES WITH THE PROCESS; AN IMAGE IS NOT IN THAT LIST AND OUTLIVES IT, SO ITS
    //PAIR IS DERIVED FROM THE SERVER'S IMAGE KEY AND THE HASH THE FILE IS NAMED AFTER INSTEAD
    let (disk_key, disk_nonce) = match persistent
    {
        true => crypto::image_keys(&hash),

        false =>
        (
            core_crypto::generate_key::
                <{ core_consts::DEFAULT_GRID_WIDTH }, { core_consts::DEFAULT_GRID_HEIGHT }>(),
            core_crypto::generate_nonce::
                <{ core_consts::DEFAULT_GRID_WIDTH }, { core_consts::DEFAULT_GRID_HEIGHT }>().unwrap().to_flat(),
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
        fs::create_dir_all(&target_dir).await.expect("Creating upload directory failed");

        //CREATE REXSTREAM FOR FILE ENCRYPTION ON DISK
        let disk_stream = RexStream::new(&Grid::from_key(&disk_key).unwrap(),
            Grid::from_flat(&disk_nonce).unwrap()).unwrap();

        //CREATE THE FILE - IT IS BUILT UNDER THE UID AND ONLY NAMED ONCE IT IS WHOLE AND VERIFIED
        let upload_path = target_dir.join(uid.to_string());
        let upload_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&upload_path)
            .await.expect("Creating upload file failed");

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
        });

        valid = true;
    }

    if !valid
    {
        //LOG FILE REJECT
        log::info!("Upload rejected: {peer_addr}");
        return;
    }

    //A FILESHARE IS WHATEVER THE UPLOADER SAYS IT IS; AN IMAGE IS NOT
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

            if !misc::is_image(&data)
            {
                log::info!("Image rejected (not an image): {peer_addr}");
                server::notify(id, PacketCode::InvalidUsage).await;
                return;
            }
        }

        //ENCRYPT CHUNK (NEVER HOLD THE UPLOAD ENTRY ACROSS AN AWAIT)
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

            //KEEP THE PLAINTEXT OF AN IMAGE - WHAT GOES TO DISK IS ENCRYPTED, AND WHAT GOES TO THE
            //CHANNEL IS THIS. READING IT BACK OFF THE DISK WOULD MEAN DECRYPTING WHAT WE JUST HELD
            if let Some(buffer) = active.image.as_mut() { buffer.extend_from_slice(&data); }

            //CHECK SIZE
            active.current_size == active.size
        };

        if !done { continue; } //UPLOAD STILL RUNNING

        //UPLOAD DONE, COLLECT FINAL STATE
        let (final_hash, expected_hash, upload_filename, final_size, image) =
        {
            let mut active = match ACTIVE_FILESHARES.get_mut(&uid)
            {
                Some(a) => a,
                None => return
            };

            let final_hash: [u8; 32] = active.hasher.clone().finalize().into();
            (final_hash, active.hash, active.filename.clone(), active.current_size, active.image.take())
        };

        //FLUSH TO DISK BEFORE RENAMING
        upload_file.lock().await.flush().await.ok();

        //CHECK HASHES
        if expected_hash != final_hash { return; }

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
        log::info!("Upload done: {peer_addr}");

        let filename = filename.into_string().unwrap_or("unnamed_file".to_string());

        //AN IMAGE IS SHOWN, NOT ANNOUNCED
        if persistent
        {
            if let Some(data) = image
            {
                let channel = server::CONNECTIONS.iter()
                    .find(|conn| conn.id() == Some(&id))
                    .and_then(|conn| conn.channel().clone());

                //KEEP IT, ON THE SAME TERMS AS A MESSAGE
                if channel.is_none() && config::read_config::<bool>("persistent_messages")
                {
                    config::messages::store_image(&username, &filename, &final_hash);
                }

                server::send_to_all(PacketCode::ImageDisplay
                {
                    username: username.clone(),
                    filename,
                    data,
                }, true, channel.as_deref());
            }
        } else
        {
            //ANNOUNCE FILE UPLOAD
            server::send_to_all(PacketCode::Uploaded
            {
                username: username.clone(),
                filename: filename.clone(),
            }, false, None);

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

    //START UPLOAD
    file::send_file(file.path, write_stream, uid, &mut rex_stream, Some(&mut seq), &mut disk_stream).await;

    //LOG END
    log::info!("Download done: {peer_addr}");
}

//READ ONE STORED IMAGE BACK OFF DISK. THE PAIR IT WAS SEALED WITH IS DERIVED FROM THE HASH THE FILE IS
//NAMED AFTER, SO NOTHING ABOUT IT HAS TO BE KEPT ANYWHERE - THE NAME IS THE KEY
pub async fn read_image(hash: &[u8; 32]) -> Option<Vec<u8>>
{
    let sealed = fs::read(misc::get_image_dir().join(misc::hex(hash))).await.ok()?;

    let (key, nonce) = crypto::image_keys(hash);

    let mut disk_stream: RexStream = RexStream::new(&Grid::from_key(&key).ok()?, Grid::from_flat(&nonce).ok()?).ok()?;

    let mut decrypted = disk_stream.update(&crypto::bytes_to_i64(&sealed)).ok()?;
    decrypted.extend(disk_stream.finalize().ok()?);

    let mut image = crypto::i64_to_bytes(&decrypted);
    image.truncate(sealed.len());

    Some(image)
}
