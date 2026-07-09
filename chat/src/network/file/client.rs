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
    path::Path,
    fs::{ self, File },
    sync::
    {
        Arc,
        Mutex,
        mpsc::Sender,
    },
};

use sha2::{ Sha256, Digest };

use why2::{ consts, crypto };

use crate::
{
    config,
    options,
    network::
    {
        self,
        file::{ self, FilePacket },
        client::{ self, ClientEvent },
    },
};

pub fn upload(token: [u8; 32], uid: u64, file_hash: [u8; 32], tx: Sender<ClientEvent>)
{
    //INIT FILE CONNECTION
    let mut stream = client::connect(options::get_server_address()).expect("File connection failed");

    //SEND TOKEN
    stream.write_all(&token).unwrap();

    //GET FILE PATH
    let path = client::ACTIVE_UPLOADS.lock().unwrap().remove(&file_hash).unwrap(); //(CRASHES IF SERVER REQUESTS FILE THAT ISN'T FOR UPLOAD)
    let filename = path.clone().file_name().and_then(|n| n.to_str()
        .map(|s| s.to_string())).unwrap_or_else(|| String::from("Unknown")); //GET FILENAME FOR CONSOLE LOG

    let size = path.metadata().unwrap().len();

    //LOG
    tx.send(ClientEvent::Upload(filename.clone())).unwrap();
    tx.send(ClientEvent::Prompt).unwrap();

    //LOCAL SEQ COUNTER
    let mut seq = 0usize;

    //GENERATE STREAM NONCE
    let nonce = crypto::generate_nonce::<{ consts::DEFAULT_GRID_WIDTH }, { consts::DEFAULT_GRID_HEIGHT }>().unwrap();
    let nonce_vec: Vec<i64> = nonce.clone().into_iter().collect();

    //SEND FIRST PACKET (METADATA)
    network::send_tcp(&mut stream, FilePacket
    {
        uid,
        size: Some(size),
        filename: Some(filename),
        hash: Some(file_hash),
        nonce: Some(nonce_vec),
        ..Default::default()
    }, options::get_keys().as_ref(), Some(&mut seq));

    //UPLOAD
    file::send_file(path, stream, uid, options::get_keys().as_ref(), Some(&mut seq));
}

pub fn download(token: [u8; 32], tx: Sender<ClientEvent>)
{
    //INIT FILE CONNECTION
    let mut stream = client::connect(options::get_server_address()).expect("File connection failed");

    //SEND TOKEN
    stream.write_all(&token).unwrap();

    //CREATE STREAM PAIR
    let write_stream = Arc::new(Mutex::new(stream.try_clone().expect("Failed cloning stream")));
    let mut streams = (&mut stream, write_stream);

    //LOCAL SEQ COUNTER
    let mut seq = 0usize;

    //RECEIVE FIRST PACKET (METADATA)
    let metadata_packet = match file::receive_file(&mut streams, options::get_keys().as_ref(), &mut seq)
    {
        Some(p) => p,
        None => return,
    };

    let size = metadata_packet.size.unwrap();
    let hash = metadata_packet.hash.unwrap();

    //NEW DOWNLOAD, GET NEW FILE
    let download_dir = config::read_config::<String>("download_directory")
        .replace("{HOME}", dirs::home_dir().expect("Could not determine home directory")
        .to_str().expect("Invalid home directory"));

    //GET SAFE FILENAME
    let filename = Path::new(&metadata_packet.filename.unwrap())
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("unnamed_file")
        .to_string();

    //CREATE DOWNLOAD DIR
    fs::create_dir_all(&download_dir).expect("Creating download directory failed");

    //LOG
    tx.send(ClientEvent::Download(filename.clone())).unwrap();
    tx.send(ClientEvent::Prompt).unwrap();

    //INIT COUNTERS
    let mut current_size = 0u64;
    let mut file = File::create(Path::new(&download_dir).join(&filename)).expect("Creating download file failed");
    let mut hasher = Sha256::new();

    //LOOP READING
    loop
    {
        //READ
        let read = match file::receive_file(&mut streams, options::get_keys().as_ref(), &mut seq)
        {
            Some(r) => r,
            None => return
        };

        let chunk_data = read.data.unwrap();

        //WRITE
        if file.write_all(&chunk_data).is_ok()
        {
            //UPDATE SIZE
            current_size += chunk_data.len() as u64;

            //UPDATE HASHER
            hasher.update(&chunk_data);

            //CHECK IF DOWNLOADING FINISHED
            if current_size == size
            {
                let final_hash: [u8; 32] = hasher.clone().finalize().into();

                //CHECK HASHES
                tx.send(if hash == final_hash
                {
                    ClientEvent::Downloaded(filename)
                } else
                {
                    ClientEvent::DownloadFailed(filename)
                }).unwrap();

                tx.send(ClientEvent::Prompt).unwrap();
                return;
            }
        }
    }
}
