/*
This is part of WHY2
Copyright (C) 2022-2025 Václav Šmejkal

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
    sync::LazyLock,
    net::{ UdpSocket, SocketAddr },
};

use dashmap::DashMap;

use crate::chat::network::voice::{ self, VoicePacket };

pub struct Connection
{
    addr: SocketAddr,  //ADDRESS OF CONNECTION
    seq: usize,        //SEQUENCE NUMBER
    server_seq: usize, //SERVER-SIDE SEQUENCE NUMBER
}

//LISTS
pub static CONNECTIONS: LazyLock<DashMap<usize, Option<Connection>>> = LazyLock::new(|| DashMap::new()); //LIST FOR EACH CLIENT CONNECTION

pub fn listen_client_voice(socket: UdpSocket)
{
    //LOOP RECEIVING
    loop
    {
        let (received, addr) = voice::receive(&socket);

        //GET ID
        let id = match received.id
        {
            Some(id) => id,
            None => continue //IGNORE INVALID IDS
        };

        //CHECK IF ID IS IN CONNECTIONS
        if let Some(mut conn) = CONNECTIONS.get_mut(&id)
        {
            //FOUND, CHECK ADDRESS
            if let Some(conn_addr) = conn.value()
            {
                //IGNORE NON-MATCHING ADDRESS
                if conn_addr.addr != addr { continue; }
            } else //NOT FOUND, ADD ADDRESS
            {
                *conn = Some(Connection
                {
                    addr: addr,
                    seq: 0,
                    server_seq: 0,
                });
            }
        } else { continue; } //IGNORE UNRECOGNIZED CONNECTIONS

        //SEND TO ALL
        for connection in CONNECTIONS.iter()
        {
            if let Some(conn_addr) = connection.value()
            {
                if conn_addr.addr == addr { continue; } //DO NOT SEND BACK TO SENDER (LOOPBACK)

                voice::send(&socket, VoicePacket
                {
                    voice: received.voice.clone(),
                    id: None,
                    seq: 0,
                }, &conn_addr.addr).unwrap();
            }
        }
    }
}
