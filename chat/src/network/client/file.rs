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

use crate::
{
    config,
    options,
    network::
    {
        self,
        client::{ self, ClientEvent },
        FilePayload,
        MessageCode,
        ActiveFileshare,
    },
};

pub fn upload(payload: FilePayload, tx: Sender<ClientEvent>)
{
    //INIT FILE CONNECTION
    let mut stream = client::connect(options::get_server_address()).expect("File connection failed");

    //SEND TOKEN
    stream.write_all(&payload.token.unwrap()).unwrap();

    //UPLOAD CONSTANTS
    let file_hash = payload.hash.unwrap();

    //GET FILE PATH
    let path = client::ACTIVE_UPLOADS.lock().unwrap().remove(&file_hash).unwrap(); //(CRASHES IF SERVER REQUESTS FILE THAT ISN'T FOR UPLOAD)
    let filename = path.clone().file_name().and_then(|n| n.to_str()
        .map(|s| s.to_string())).unwrap_or(String::from("Unknown")); //GET FILENAME FOR CONSOLE LOG

    //LOG
    tx.send(ClientEvent::Upload(filename)).unwrap();
    tx.send(ClientEvent::Prompt(options::INPUT_READ.lock().unwrap().iter().collect::<String>())).unwrap();

    //UPLOAD
    network::send_file(path, stream, payload.uid, MessageCode::Upload, options::get_keys().as_ref());
}

pub fn download(payload: FilePayload, tx: Sender<ClientEvent>)
{
    //INIT FILE CONNECTION
    let mut stream = client::connect(options::get_server_address()).expect("File connection failed");

    //SEND TOKEN
    stream.write_all(&payload.token.unwrap()).unwrap();

    //CREATE STREAM PAIR
    let write_stream = Arc::new(Mutex::new(stream.try_clone().expect("Failed cloning stream")));
    let mut streams = (&mut stream, write_stream);

    //NEW DOWNLOAD, GET NEW FILE
    let download_dir = config::read_config::<String>("download_directory")
        .replace("{HOME}", dirs::home_dir().expect("Could not determine home directory")
        .to_str().expect("Invalid home directory"));

    //GET SAFE FILENAME
    let filename = Path::new(&payload.filename.unwrap())
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("unnamed_file")
        .to_string();

    //CREATE DOWNLOAD DIR
    fs::create_dir_all(&download_dir).expect("Creating download directory failed");

    network::ACTIVE_FILESHARES.insert(payload.uid, ActiveFileshare
    {
        file: File::create(Path::new(&download_dir)
            .join(&filename)).expect("Creating download file failed"),
        size: payload.size.unwrap(),
        current_size: 0,
        hash: payload.hash.unwrap(),
        hasher: Sha256::new(),
        filename: filename.clone(),
    });

    //LOG
    tx.send(ClientEvent::Download(filename)).unwrap();
    tx.send(ClientEvent::Prompt(options::INPUT_READ.lock().unwrap().iter().collect::<String>())).unwrap();

    //INIT SEQ
    let mut seq = 0usize;

    //LOOP READING
    loop
    {
        //READ
        let read = match network::receive(&mut streams, options::get_keys().as_ref(), Some(&mut seq))
        {
            Some(r) => r,
            None => return
        };

        let file = read.file.unwrap();

        //CHECK IF UPLOAD IS ALREADY ACTIVE
        if let Some(mut active) = network::ACTIVE_FILESHARES.get_mut(&file.uid)
        {
            let chunk_data = file.data.unwrap();

            //WRITE
            if active.file.write_all(&chunk_data).is_ok()
            {
                //UPDATE SIZE
                active.current_size += chunk_data.len() as u64;

                //UPDATE HASHER
                active.hasher.update(&chunk_data);

                //CHECK IF DOWNLOADING FINISHED
                if active.current_size == active.size
                {
                    let filename = active.filename.clone();
                    let final_hash: [u8; 32] = active.hasher.clone().finalize().into();

                    //CHECK HASHES
                    tx.send(if active.hash == final_hash
                    {
                        ClientEvent::Downloaded(filename)
                    } else
                    {
                        ClientEvent::DownloadFailed(filename)
                    }).unwrap();
                    tx.send(ClientEvent::Prompt(options::INPUT_READ.lock().unwrap().iter().collect::<String>())).unwrap();

                    //REMOVE ACTIVE STREAM
                    drop(active);
                    network::ACTIVE_FILESHARES.remove(&file.uid);
                    return;
                }
            }
        }
    }
}
