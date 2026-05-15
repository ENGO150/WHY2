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
    fs,
    io::Write,
    ffi::OsStr,
    net::TcpStream,
    path::{ Path, PathBuf },
};

use sha2::Digest;

use crate::
{
    misc,
    consts::{ self, Streams },
    network::
    {
        self,
        server::{ self, AvailableFile },
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
            if network::ACTIVE_FILESHARES.remove(&self.uid).is_some() && let Some(uname) = conn.username()
            {
                let temp_dir = misc::get_upload_dir(uname);
                let junk_file = temp_dir.join(self.uid.to_string());
                let _ = fs::remove_file(&junk_file);
            }
        }
    }
}

//PUBLIC
pub fn download(id: usize, streams: &mut Streams, uid: u64)
{
    //GET SOCKET ADDR
    let peer_addr = match streams.0.peer_addr()
    {
        Ok(s) => s,
        Err(_) => return
    };

    //GET CLIENT INFO
    let (keys, username) =
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

                (keys, username)
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

    //LOOP READING
    loop
    {
        //READ
        let read = match network::receive(streams, Some(&keys), Some(&mut seq))
        {
            Some(r) => r,
            None => return
        };

        if let Some(file) = read.file //CHECK FOR FILE PAYLOAD
        {
            if let Some(mut active) = network::ACTIVE_FILESHARES.get_mut(&file.uid) &&
                let Some(chunk_data) = file.data && active.client_id == id
            {
                if chunk_data.len() <= consts::UPLOAD_CHUNK_SIZE && //CHECK PACKET SIZE
                    active.file.write_all(&chunk_data).is_ok() //WRITE
                {
                    //UPDATE SIZE
                    active.current_size += chunk_data.len() as u64;

                    if active.current_size > active.size
                    {
                        //REMOVE JUNK FILE
                        let temp_dir = misc::get_upload_dir(&username);
                        let _ = fs::remove_file(temp_dir.join(file.uid.to_string()));

                        drop(active);
                        network::ACTIVE_FILESHARES.remove(&file.uid);

                        log::error!("Upload overflow: {peer_addr}");
                        return; //DISCONNECT FILE CHANNEL
                    }

                    //UPDATE HASHER
                    active.hasher.update(&chunk_data);

                    //CHECK SIZE
                    if active.current_size == active.size //UPLOAD DONE
                    {
                        let delete: bool;

                        //GET FILE PATH
                        let temp_dir = misc::get_upload_dir(&username);
                        let current_path = temp_dir.join(file.uid.to_string());
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
                            delete = fs::rename(&current_path, &new_path).is_err();

                            //SET NEW FILE VARIABLES
                            new_filename = Some(filename);
                            final_path = Some(new_path);
                        } else { delete = true; }

                        if delete
                        {
                            //REMOVE JUNK FILE
                            let _ = fs::remove_file(&current_path);

                            //LOG FILE UPLOAD
                            log::error!("Upload failed: {peer_addr}");
                        } else
                        {
                            //LOG FILE UPLOAD
                            log::info!("Upload done: {peer_addr}");

                            let filename = new_filename.and_then(|f| f.to_str()).unwrap_or("unnamed_file").to_owned();

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
                        }

                        //REMOVE ACTIVE UPLOAD
                        drop(active);
                        network::ACTIVE_FILESHARES.remove(&file.uid);
                        return;
                    }
                }
            }
        }
    }
}

pub fn upload(id: usize, stream: TcpStream, path: PathBuf, uid: u64)
{
    //GET SOCKET ADDR
    let peer_addr = match stream.peer_addr()
    {
        Ok(s) => s,
        Err(_) => return
    };

    //GET CLIENT INFO
    let keys =
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

                keys
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

    //START UPLOAD
    network::send_file(path, stream, uid, MessageCode::Download, Some(&keys));

    //LOG END
    log::info!("Download done: {peer_addr}");
}
