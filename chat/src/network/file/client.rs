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
    path::Path,
};

use tokio::
{
    io::AsyncWriteExt,
    fs::{ self, File },
    sync::
    {
        Mutex,
        mpsc::Sender,
    },
};

use sha2::{ Sha256, Digest };

use crate::
{
    config,
    options,
    crypto as chat_crypto,
    network::
    {
        self,
        EncryptionMode,
        client::{ self, ClientEvent },
        file::
        {
            self,
            FilePacket,
            FilePacketCode,
        },
    },
};

pub async fn upload(token: [u8; 32], uid: u64, file_hash: [u8; 32], tx: Sender<ClientEvent>)
{
    //INIT FILE CONNECTION
    let (_read_stream, mut write_stream) = client::connect(options::get_server_address()).await.expect("File connection failed");

    //SEND TOKEN
    write_stream.write_all(&token).await.unwrap();

    //GET FILE PATH
    let path = client::ACTIVE_UPLOADS.lock().unwrap().remove(&file_hash).unwrap(); //(CRASHES IF SERVER REQUESTS FILE THAT ISN'T FOR UPLOAD)
    let filename = path.clone().file_name().and_then(|n| n.to_str()
        .map(|s| s.to_string())).unwrap_or_else(|| String::from("Unknown")); //GET FILENAME FOR CONSOLE LOG

    let size = fs::metadata(&path).await.unwrap().len();

    //LOG
    tx.send(ClientEvent::Upload(filename.clone())).await.unwrap();
    tx.send(ClientEvent::Prompt).await.unwrap();

    //LOCAL SEQ COUNTER
    let mut seq = 0usize;

    //INIT REX STREAM
    let mut rex_stream = chat_crypto::init_rex_stream(options::get_keys().as_ref().unwrap(), &token).unwrap();

    //SEND FIRST PACKET (METADATA)
    network::send_tcp(&mut write_stream, FilePacket
    {
        uid,
        code: FilePacketCode::Metadata
        {
            size,
            filename,
            hash: file_hash,
        },
        seq: 0,
    }, EncryptionMode::Stream(&mut rex_stream), Some(&mut seq)).await;

    //UPLOAD
    file::send_file(path, write_stream, uid, &mut rex_stream, Some(&mut seq)).await;
}

pub async fn download(token: [u8; 32], tx: Sender<ClientEvent>)
{
    //INIT FILE CONNECTION
    let (mut read_stream, mut write_stream) = client::connect(options::get_server_address()).await.expect("File connection failed");

    //SEND TOKEN
    write_stream.write_all(&token).await.unwrap();

    //CREATE STREAM PAIR
    let mut streams = (&mut read_stream, Arc::new(Mutex::new(write_stream)));

    //LOCAL SEQ COUNTER
    let mut seq = 0usize;

    //INIT REX STREAM
    let mut rex_stream = chat_crypto::init_rex_stream(options::get_keys().as_ref().unwrap(), &token).unwrap();

    //RECEIVE FIRST PACKET (METADATA)
    let (size, filename, hash) = match file::receive_file(&mut streams, &mut rex_stream, &mut seq).await
    {
        Some((_, FilePacketCode::Metadata { size, filename, hash })) => (size, filename, hash),
        _ => return,
    };

    //NEW DOWNLOAD, GET NEW FILE
    let download_dir = config::read_config::<String>("download_directory")
        .replace("{HOME}", dirs::home_dir().expect("Could not determine home directory")
        .to_str().expect("Invalid home directory"));

    //GET SAFE FILENAME
    let filename = Path::new(&filename)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("unnamed_file")
        .to_string();

    //CREATE DOWNLOAD DIR
    fs::create_dir_all(&download_dir).await.expect("Creating download directory failed");

    //LOG
    tx.send(ClientEvent::Download(filename.clone())).await.unwrap();
    tx.send(ClientEvent::Prompt).await.unwrap();

    //INIT COUNTERS
    let mut current_size = 0u64;
    let mut file = File::create(Path::new(&download_dir).join(&filename)).await.expect("Creating download file failed");
    let mut hasher = Sha256::new();

    //LOOP READING
    loop
    {
        //READ
        let data = match file::receive_file(&mut streams, &mut rex_stream, &mut seq).await
        {
            Some((_, FilePacketCode::Data { data })) => data,
            _ => return
        };

        //WRITE
        if file.write_all(&data).await.is_ok()
        {
            //UPDATE SIZE
            current_size += data.len() as u64;

            //UPDATE HASHER
            hasher.update(&data);

            //CHECK IF DOWNLOADING FINISHED
            if current_size == size
            {
                let final_hash: [u8; 32] = hasher.clone().finalize().into();

                //FLUSH TO DISK
                file.flush().await.ok();

                //CHECK HASHES
                tx.send(if hash == final_hash
                {
                    ClientEvent::Downloaded(filename)
                } else
                {
                    ClientEvent::DownloadFailed(filename)
                }).await.unwrap();

                tx.send(ClientEvent::Prompt).await.unwrap();
                return;
            }
        }
    }
}
