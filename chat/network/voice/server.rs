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
    sync::LazyLock,
    net::{ UdpSocket, SocketAddr },
};

use dashmap::DashMap;

use crate::chat::
{
    options::SharedKeys,
    network::
    {
        server,
        voice::{ self, VoicePacket },
    },
};

pub struct Connection
{
    addr: SocketAddr,  //ADDRESS OF CONNECTION
    id: usize,         //ID ON TEXT CHAT
    seq: usize,        //SEQUENCE NUMBER
    server_seq: usize, //SERVER SEQUENCE NUMBER
}

//LISTS
pub static CONNECTIONS: LazyLock<DashMap<usize, Option<Connection>>> = LazyLock::new(|| DashMap::new()); //LIST FOR EACH CLIENT CONNECTION

//IMPLEMENTATIONS
impl Connection
{
    //GET PEER ADDRESS
    pub fn peer_addr(&self) -> &SocketAddr
    {
        &self.addr
    }

    //GET SEQ
    pub fn seq(&self) -> &usize
    {
        &self.seq
    }

    //GET SEQ AS MUTABLE
    pub fn seq_mut(&mut self) -> &mut usize
    {
        &mut self.seq
    }

    //GET SERVER SEQ
    pub fn server_seq(&self) -> &usize
    {
        &self.server_seq
    }

    //GET SERVER SEQ AS MUTABLE
    pub fn server_seq_mut(&mut self) -> &mut usize
    {
        &mut self.server_seq
    }
}

//FUNCTIONS
pub fn find_key(id: &usize) -> Option<SharedKeys>
{
    server::CONNECTIONS.iter()
        .find(|entry| entry.value().id() == Some(id))
        .map(|c| c.keys().unwrap().clone())
}

pub fn find_channel(id: &usize) -> Option<Option<String>>
{
    server::CONNECTIONS.iter()
        .find(|entry| entry.value().id() == Some(id))
        .map(|c| c.channel().clone())
}

pub fn listen_client_voice(socket: UdpSocket)
{
    //LOOP RECEIVING
    loop
    {
        let (received, addr) = voice::receive(&socket).unwrap();

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
            if let Some(conn) = conn.as_mut()
            {
                //IGNORE NON-MATCHING ADDRESS
                if conn.addr != addr { continue; }

                //VERIFY SEQ
                if received.seq <= conn.seq { continue; } //IGNORE INVALID SEQs
                *conn.seq_mut() = received.seq;
            } else //NOT FOUND, ADD ADDRESS
            {
                *conn = Some(Connection
                {
                    addr: addr,
                    id: id,
                    seq: 0,
                    server_seq: 0,
                });

                log::info!("New voice connection: {}", addr);
            }
        } else { continue; } //IGNORE UNRECOGNIZED CONNECTIONS

        //FIND SENDER'S CHANNEL
        let sender_channel = find_channel(&id);

        //COLLECT ALL ADDRESSES
        let mut addresses: Vec<(SocketAddr, SharedKeys, usize)> = Vec::new();
        for connection in CONNECTIONS.iter()
        {
            if let Some(conn) = connection.value()
            {
                //DO NOT SEND BACK TO SENDER (LOOPBACK)
                if conn.addr != addr
                {
                    //SEND ONLY TO SAME CHANNEL
                    if sender_channel != find_channel(&conn.id) { continue; }

                    //FIND CONNECTION KEYS
                    if let Some(keys) = find_key(&conn.id)
                    {
                        addresses.push((conn.addr, keys, conn.id));
                    }
                }
            }
        }

        //SEND TO ALL
        for (addr, keys, recipient_id) in addresses.iter()
        {
            voice::send(&socket, VoicePacket
            {
                voice: received.voice.clone(),
                id: Some(id),
                ..Default::default()
            }, addr, recipient_id, keys).unwrap();
        }
    }
}
