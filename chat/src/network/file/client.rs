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

//! # File Transfer — Client Side
//!
//! Runs approved uploads and downloads, each on its own auxiliary socket, and reports their
//! progress to the UI as [`ClientEvent`]s.

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
    t,
    misc,
    cache,
    config,
    options,
    crypto as chat_crypto,
    network::
    {
        self,
        EncryptionMode,
        client::
        {
            self,
            ClientEvent,
            handshake,
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

/// Uploads a file the server approved.
///
/// The path is taken from [`ACTIVE_UPLOADS`](client::ACTIVE_UPLOADS), where the request that
/// asked for the upload parked it under its hash. A cut avatar or recorded clip is removed from
/// the temp directory once sent.
///
/// # Parameters
/// - `token`: The transfer token from the approval.
/// - `uid`: The upload id from the approval.
/// - `file_hash`: SHA-256 of the file, which names it in `ACTIVE_UPLOADS`.
/// - `tx`: The UI event channel.
/// - `kind`: What the upload is for; decides what is reported.
///
/// # Panics
/// If the socket cannot be opened, the hash was never requested, or the file cannot be read.
pub async fn upload(token: [u8; 32], uid: u64, file_hash: [u8; 32], tx: Sender<ClientEvent>, kind: UploadKind)
{
    //INIT FILE CONNECTION
    let (_read_stream, mut write_stream) = handshake::connect(options::get_server_address()).await.expect("File connection failed");

    //SEND TOKEN
    write_stream.write_all(&token).await.unwrap();

    //GET FILE PATH
    let path = client::ACTIVE_UPLOADS.lock().unwrap().remove(&file_hash).unwrap(); //(CRASHES IF SERVER REQUESTS FILE THAT ISN'T FOR UPLOAD)
    let filename = path.clone().file_name().and_then(|n| n.to_str()
        .map(|s| s.to_string())).unwrap_or_else(|| t!("transfer.unknown_file").to_owned()); //GET FILENAME FOR CONSOLE LOG

    let size = fs::metadata(&path).await.unwrap().len();

    //LOG
    match kind
    {
        UploadKind::File => tx.send(ClientEvent::Upload { uid, filename: filename.clone(), size }).await.unwrap(),
        UploadKind::Voice => {},
        _ => tx.send(ClientEvent::Image { uid, filename: filename.clone(), size }).await.unwrap(),
    }

    //LOCAL SEQ COUNTER
    let mut seq = 0usize;

    //INIT REX STREAM
    let mut rex_stream = chat_crypto::init_rex_stream(options::get_keys().as_ref().unwrap(), &token).unwrap();

    //SEND FIRST PACKET (METADATA)
    let _ = network::send_tcp(&mut write_stream, FilePacket
    {
        uid,
        code: FilePacketCode::Metadata
        {
            size,
            filename: filename.clone(),
            hash: file_hash,
        },
        seq: 0,
    }, EncryptionMode::Stream(&mut rex_stream), Some(&mut seq)).await;

    //UPLOAD
    let progress = tx.clone();

    file::send_file(path.clone(), write_stream, uid, &mut rex_stream, Some(&mut seq), |sent|
    {
        progress.try_send(ClientEvent::TransferProgress { uid, done: sent }).ok(); //A DROPPED TICK COSTS NOTHING
    }).await;

    misc::drop_upload_temp(&path);

    //DONE
    tx.send(ClientEvent::UploadDone { uid, filename }).await.unwrap();
}

//COPY AN ATTACHMENT FROM THE VERIFIED DISK CACHE
/// Downloads a cached attachment in chunks, returning false on a cache miss.
/// Output errors are reported as download failures rather than retried over the network.
pub async fn download_cached(hash: [u8; 32], filename: &str, tx: Sender<ClientEvent>) -> bool
{
    //REDUCE THE NAME TO ITS LAST COMPONENT
    let filename = Path::new(filename).file_name().and_then(|name| name.to_str())
        .unwrap_or("unnamed_file").to_owned();
    let directory = config::read_config::<String>("download_directory")
        .replace("{HOME}", dirs::home_dir().expect("Could not determine home directory")
            .to_str().expect("Invalid home directory"));
    let uid = rand::random();

    //CREATE THE OUTPUT DIRECTORY BEFORE COPYING
    let result = match fs::create_dir_all(&directory).await
    {
        Ok(()) => cache::copy(&hash, &Path::new(&directory).join(&filename)).await,
        Err(error) => Some(Err(error)),
    };

    let Some(result) = result else { return false };
    let size = result.as_ref().copied().unwrap_or_default();

    //REPORT THE LOCAL TRANSFER THROUGH THE SAME UI EVENTS
    tx.send(ClientEvent::Download { uid, filename: filename.clone(), size }).await.ok();

    tx.send(match result
    {
        Ok(_) => ClientEvent::Downloaded { uid, filename },
        Err(_) => ClientEvent::DownloadFailed { uid, filename },
    }).await.ok();

    true
}

/// Downloads a file the server approved into the `download_directory`.
///
/// The file name is reduced to its last component, so a server cannot write outside the
/// download directory. The download is reported as
/// [`Downloaded`](ClientEvent::Downloaded) or [`DownloadFailed`](ClientEvent::DownloadFailed)
/// by whether its SHA-256 matches the metadata.
///
/// # Parameters
/// - `token`: The transfer token from the approval.
/// - `tx`: The UI event channel.
///
/// # Panics
/// If the socket cannot be opened, or the download directory or file cannot be created.
pub async fn download(token: [u8; 32], tx: Sender<ClientEvent>)
{
    //INIT FILE CONNECTION
    let (mut read_stream, mut write_stream) = handshake::connect(options::get_server_address()).await.expect("File connection failed");

    //SEND TOKEN
    write_stream.write_all(&token).await.unwrap();

    //CREATE STREAM PAIR
    let mut streams = (&mut read_stream, Arc::new(Mutex::new(write_stream)));

    //LOCAL SEQ COUNTER
    let mut seq = 0usize;

    //INIT REX STREAM
    let mut rex_stream = chat_crypto::init_rex_stream(options::get_keys().as_ref().unwrap(), &token).unwrap();

    //RECEIVE FIRST PACKET (METADATA)
    let (uid, size, filename, hash) = match file::receive_file(&mut streams, &mut rex_stream, &mut seq).await
    {
        Some((uid, FilePacketCode::Metadata { size, filename, hash })) => (uid, size, filename, hash),
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
    tx.send(ClientEvent::Download { uid, filename: filename.clone(), size }).await.unwrap();

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

            //REPORT PROGRESS
            tx.try_send(ClientEvent::TransferProgress { uid, done: current_size }).ok();

            //CHECK IF DOWNLOADING FINISHED
            if current_size == size
            {
                let final_hash: [u8; 32] = hasher.clone().finalize().into();

                //FLUSH TO DISK
                file.flush().await.ok();

                //CHECK HASHES
                tx.send(if hash == final_hash
                {
                    ClientEvent::Downloaded { uid, filename }
                } else
                {
                    ClientEvent::DownloadFailed { uid, filename }
                }).await.unwrap();

                return;
            }
        }
    }
}
