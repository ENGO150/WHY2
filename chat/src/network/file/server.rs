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
    io::Write,
    ffi::OsStr,
    net::TcpStream,
    sync::LazyLock,
    fs::{ self, File },
    path::Path,
};

use dashmap::DashMap;

use sha2::{ Sha256, Digest };

use crate::
{
    config,
    misc,
    consts::{ self, Streams },
    network::
    {
        self,
        file::{ self, FilePacket },
        server::{ self, AvailableFile },
        EncryptionMode,
        MessagePacket,
        MessageCode,
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

            //REMOVE JUNK FILE
            if ACTIVE_FILESHARES.remove(&self.uid).is_some() && let Some(uname) = conn.username()
            {
                let temp_dir = misc::get_upload_dir(uname);
                let junk_file = temp_dir.join(self.uid.to_string());
                let _ = fs::remove_file(&junk_file);
                log::error!("Upload failed: {}", conn.peer_addr());
            }
        }
    }
}

//PUBLIC
pub struct ActiveFileshare //ACTIVE FILE UPLOAD
{
    pub file: File,                                  //TARGET FILE (SERVER-SIDE)
    pub size: u64,                                   //EXPECTED FILE SIZE
    pub current_size: u64,                           //CURRENT SIZE
    pub hash: [u8; 32],                              //SHA256 HASH OF FINAL FILE
    pub hasher: Sha256,                              //HASHER
    pub filename: String,                            //FILENAME
    #[cfg(feature = "server")] pub client_id: usize, //ID OF SENDER
}

//LISTS
pub static ACTIVE_FILESHARES: LazyLock<DashMap<u64, ActiveFileshare>> = LazyLock::new(|| DashMap::new()); //LIST FOR ACTIVE FILE UPLOADS

pub fn download(id: usize, streams: &mut Streams, uid: u64)
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
                c.add_file_stream(uid, streams.0.try_clone().unwrap());

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

    //WAIT FOR FIRST PACKET (METADATA)
    let metadata_packet = match file::receive_file(streams, Some(&keys), &mut seq)
    {
        Some(p) => p,
        None => return
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

    if !valid && let Some(size) = metadata_packet.size &&
        let Some(hash) = metadata_packet.hash &&
        let Some(filename) = metadata_packet.filename &&
        size / 1_048_576 <= config::read_config::<u64>("max_upload_size")
    {
        //CREATE TEMP UPLOAD DIRECTORY
        let temp_dir = misc::get_upload_dir(&username);
        fs::create_dir_all(&temp_dir).expect("Creating upload temp directory failed");

        //ADD ACTIVE UPLOAD (ALSO CREATE THE FILE)
        ACTIVE_FILESHARES.insert(uid, ActiveFileshare
        {
            file: File::create_new(temp_dir.join(uid.to_string())).expect("Creating upload file failed"),
            size,
            current_size: 0,
            hash,
            hasher: Sha256::new(),
            filename,
            client_id: id,
        });

        valid = true;
    }

    if !valid
    {
        //LOG FILE REJECT
        log::info!("Upload rejected: {peer_addr}");
        return;
    }

    //LOOP READING CHUNKS
    loop
    {
        //READ
        let read = match file::receive_file(streams, Some(&keys), &mut seq)
        {
            Some(r) => r,
            None => return
        };

        if let Some(chunk_data) = read.data
        {
            if let Some(mut active) = ACTIVE_FILESHARES.get_mut(&read.uid) && active.client_id == id
            {
                if chunk_data.len() <= consts::UPLOAD_CHUNK_SIZE && //CHECK PACKET SIZE
                    active.file.write_all(&chunk_data).is_ok() //WRITE
                {
                    //UPDATE SIZE
                    active.current_size += chunk_data.len() as u64;

                    if active.current_size > active.size { return; }

                    //UPDATE HASHER
                    active.hasher.update(&chunk_data);

                    //CHECK SIZE
                    if active.current_size == active.size //UPLOAD DONE
                    {
                        let valid: bool;

                        //GET FILE PATH
                        let temp_dir = misc::get_upload_dir(&username);
                        let current_path = temp_dir.join(read.uid.to_string());
                        let mut new_filename = None;
                        let mut final_path = None;
                        let mut insert = false;
                        let final_hash: [u8; 32] = active.hasher.clone().finalize().into();

                        //CHECK HASHES
                        if active.hash == final_hash
                        {
                            //GET NEW FILE PATH
                            let filename = Path::new(&active.filename) //PREVENT FROM PATH TRAVERSAL
                                .file_name()
                                .unwrap_or(OsStr::new("unnamed_file"));
                            let new_path = temp_dir.join(filename);

                            //RENAME FILE
                            insert = !new_path.is_file();
                            valid = fs::rename(&current_path, &new_path).is_ok();

                            //SET NEW FILE VARIABLES
                            new_filename = Some(filename.to_os_string());
                            final_path = Some(new_path);
                        } else { valid = false; }

                        if !valid { return; }

                        //LOG FILE UPLOAD
                        log::info!("Upload done: {peer_addr}");

                        let filename = new_filename.and_then(|f| f.into_string().ok()).unwrap_or("unnamed_file".to_string());

                        //ANNOUNCE FILE UPLOAD
                        server::send_to_all(MessagePacket
                        {
                            text: Some(filename.clone()),
                            username: Some(username.clone()),
                            code: Some(MessageCode::Uploaded),
                            ..Default::default()
                        });

                        if insert
                        {
                            //ADD FILE TO AVAILABLE FILES
                            server::AVAILABLE_FILES.get_mut(username.as_str()).unwrap().push(AvailableFile
                            {
                                hash: final_hash,
                                path: final_path.unwrap(),
                                filename,
                                size: active.current_size,
                            });
                        }

                        //REMOVE ACTIVE UPLOAD
                        drop(active);
                        ACTIVE_FILESHARES.remove(&read.uid);
                        return;
                    }
                }
            }
        }
    }
}

pub fn upload(id: usize, mut stream: TcpStream, file: AvailableFile, uid: u64)
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
                c.add_file_stream(uid, stream.try_clone().unwrap());

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

    //SEND FIRST PACKET (METADATA)
    let mut seq = 0usize;
    network::send_tcp(&mut stream, FilePacket
    {
        uid,
        size: Some(file.size),
        filename: Some(file.filename.clone()),
        hash: Some(file.hash),
        ..Default::default()
    }, EncryptionMode::OneShot(Some(&keys)), Some(&mut seq));

    //START UPLOAD
    file::send_file(file.path, stream, uid, Some(&keys), Some(&mut seq));

    //LOG END
    log::info!("Download done: {peer_addr}");
}
