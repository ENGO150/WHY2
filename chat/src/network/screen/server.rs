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
    net::TcpStream,
    sync::{ Arc, Mutex },
};

use crate::
{
    consts::SharedKeys,
    network::
    {
        self,
        Streams,
        server::{ self, Connection },
    },
};

//PRIVATE
//STRUCTS
struct ScreenTransferGuard
{
    id: usize,
}

//IMPLEMENTATIONS
impl Drop for ScreenTransferGuard
{
    fn drop(&mut self)
    {
        if let Some(mut conn) = server::CONNECTIONS.iter_mut().find(|c| c.id() == Some(&self.id))
        {
            //REMOVE SCREEN STREAM
            conn.remove_screen_stream();
        }
    }
}

//PUBLIC
//FUNCTIONS
pub fn screen(id: usize, streams: &mut Streams)
{
    //GET CLIENT KEYS
    let keys =
    {
        //FIND CONNECTION BY ID
        let conn = server::CONNECTIONS.iter_mut()
            .find(|e| e.value().id() == Some(&id));

        match conn
        {
            Some(mut c) =>
            {
                let keys = match c.keys()
                {
                    Some(k) => k.clone(),
                    None => return
                };

                //ADD FILE STREAM
                c.set_screen_stream(Arc::new(Mutex::new(streams.0.try_clone().unwrap())));

                keys
            },
            None => return
        }
    };

    //DISCONNECT GUARD
    let _guard = ScreenTransferGuard { id };

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

        if read.screen.is_none() { continue; } //DO NOT FORWARD INVALID FRAMES

        //COLLECT ALL ATTACHED CLIENT STREAMS
        let entries: Vec<(Arc<TcpStream>, Option<SharedKeys>)> = server::CONNECTIONS.iter().filter_map(|entry|
        {
            match entry.value()
            {
                Connection::Authenticated { id: client_id, attached_screen, .. } =>
                {
                    //FILTER ATTACHED CLIENTS
                    if let Some(attached_screen) = attached_screen && attached_screen.target_id == id
                    {
                        //PREVENT FEEDBACK
                        if *client_id == id && read.screen.as_ref().is_some_and(|s| s.frame.is_none())
                        {
                            return None;
                        }

                        //FOUND, COLLECT
                        Some((attached_screen.stream.clone(), entry.value().keys().cloned()))
                    } else { None }
                },
                _ => None,
            }
        }).collect();

        //FORWARD PACKET
        for ref mut entry in entries
        {
            if let Ok(mut stream) = entry.0.try_clone()
            {
                network::send(&mut stream, read.clone(), entry.1.as_ref(), None);
            }
        }
    }
}
