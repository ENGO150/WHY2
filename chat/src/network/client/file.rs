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
    thread,
    sync::mpsc::Sender,
};

use crate::
{
    options,
    network::
    {
        self,
        client::{ self, ClientEvent },
        FilePayload,
        MessageCode,
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

    //SPAWN UPLOAD THREAD
    thread::spawn(move || network::send_file(path, stream,
        payload.uid, MessageCode::Upload, options::get_keys().as_ref()));

    tx.send(ClientEvent::Upload(filename)).unwrap();
}
