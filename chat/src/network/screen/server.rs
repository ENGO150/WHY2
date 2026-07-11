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
    collections::HashMap,
    sync::{ Arc, Mutex },
};

use why2::
{
    consts,
    stream::RexStream,
};

use crate::
{
    crypto,
    network::
    {
        screen,
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
pub fn screen(token: [u8; 32], id: usize, streams: &mut Streams)
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

    //VIEWER MAPS
    let mut viewer_seqs = HashMap::<usize, usize>::new();
    let mut viewer_streams = HashMap::<usize, RexStream<{ consts::DEFAULT_GRID_WIDTH }, { consts::DEFAULT_GRID_HEIGHT }>>::new();

    //INIT REX STREAM
    let mut rex_stream = crypto::init_rex_stream::<{ consts::DEFAULT_GRID_WIDTH }, { consts::DEFAULT_GRID_HEIGHT }>
    (
        &keys,
        &token,
    ).unwrap();

    //LOOP READING
    loop
    {
        //READ
        let read = match screen::receive_frame(streams, &mut rex_stream, &mut seq)
        {
            Some(r) => r,
            None => return
        };

        //COLLECT ALL ATTACHED CLIENT STREAMS
        let entries: Vec<(usize, Arc<TcpStream>)> = server::CONNECTIONS.iter().filter_map(|entry|
        {
            match entry.value()
            {
                Connection::Authenticated { id: client_id, attached_screen, keys, .. } =>
                {
                    //FILTER ATTACHED CLIENTS
                    if let Some(attached_screen) = attached_screen && attached_screen.target_id == id
                    {
                        //CHECK FOR EXISTING REX STREAM
                        if !viewer_streams.contains_key(client_id)
                        {
                            //INIT REX STREAM
                            let rex_stream = crypto::init_rex_stream::
                                <{ consts::DEFAULT_GRID_WIDTH }, { consts::DEFAULT_GRID_HEIGHT }>
                            (
                                &keys,
                                &attached_screen.token,
                            ).unwrap();

                            viewer_streams.insert(*client_id, rex_stream);
                        }

                        //PREVENT FEEDBACK
                        if *client_id == id && read.frame.is_none()
                        {
                            return None;
                        }

                        //FOUND, COLLECT
                        Some((*client_id, attached_screen.stream.clone()))
                    } else { None }
                },
                _ => None,
            }
        }).collect();

        //FORWARD PACKET
        for (client_id, stream) in entries
        {
            if let Ok(mut cloned_stream) = stream.try_clone()
            {
                let viewer_seq = viewer_seqs.entry(client_id).or_insert(0);
                let viewer_stream = viewer_streams.get_mut(&client_id).unwrap();

                screen::send_frame(&mut cloned_stream, read.clone(), viewer_stream, Some(viewer_seq));
            }
        }
    }
}
