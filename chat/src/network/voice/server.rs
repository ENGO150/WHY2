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
    time::Instant,
    sync::LazyLock,
    net::{ UdpSocket, SocketAddr },
};

use dashmap::DashMap;

use crate::
{
    consts::SharedKeys,
    network::
    {
        server,
        voice::
        {
            self,
            consts,
            VoiceCode,
            VoicePacket,
        },
    },
};

pub struct Connection
{
    addr: SocketAddr,          //ADDRESS OF CONNECTION
    id: usize,                 //ID ON TEXT CHAT
    seq: usize,                //SEQUENCE NUMBER
    server_seq: usize,         //SERVER SEQUENCE NUMBER
    packet_accumulator: usize, //PACKET ACCUMULATOR
}

//LISTS
pub static CONNECTIONS: LazyLock<DashMap<usize, (Option<Connection>, String)>> = LazyLock::new(|| DashMap::new()); //LIST FOR EACH CLIENT CONNECTION

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

//HELPER FUNCTIONS
//PRIVATE
fn parse_opus_len(bytes: &[u8]) -> (usize, usize)
{
    if bytes.is_empty() { return (0, 0); }
    let b0 = bytes[0] as usize;
    if b0 < 252
    {
        (b0, 1)
    } else if bytes.len() >= 2
    {
        let b1 = bytes[1] as usize;
        (b1 * 4 + b0, 2)
    } else
    {
        (0, 0)
    }
}

fn validate_opus_packet(packet: Option<&[u8]>) -> bool
{
    let packet = match packet
    {
        Some(p) => p,
        None => return false
    };

    if packet.is_empty() { return false; }

    let toc = packet[0];
    let framing = toc & 0x03;

    match framing
    {
        0 => true,
        1 => (packet.len() - 1) % 2 == 0,
        2 =>
        {
            if packet.len() < 2 { return false; }
            let (len, count) = parse_opus_len(&packet[1..]);
            if count == 0 { return false; }
            packet.len() >= 1 + count + len
        },
        3 => {
            if packet.len() < 2 { return false; }
            let frame_count = packet[1] & 0x3F;
            frame_count > 0
        },
        _ => false
    }
}

//PUBLIC
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

pub fn remove_connection(id: &usize) //REMOVE CONNECTION
{
    if let Some((_, (conn, _))) = CONNECTIONS.remove(id) &&
        let Some(conn) = conn
    {
        log::info!("Close voice connection: {}", conn.peer_addr());
    }
}

pub fn reset_last_activity(id: &usize)
{
    if let Some(mut conn) = server::CONNECTIONS.iter_mut()
        .find(|entry| entry.value().id() == Some(id))
    {
        *conn.last_activity_mut() = Instant::now();
    }
}

//MAIN FUNCTION
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

        //CLIENT USERNAME
        let username: String;

        //CHECK IF ID IS IN CONNECTIONS
        if let Some(mut conn) = CONNECTIONS.get_mut(&id)
        {
            //FOUND, CHECK ADDRESS
            if let Some(conn) = conn.0.as_mut()
            {
                //NAT PORT SHIFTING
                if conn.addr != addr
                {
                    if conn.addr.ip() == addr.ip()
                    {
                        conn.addr = addr;
                    } else { continue; } //IGNORE NON-MATCHING ADDRESS (SPOOFING)
                }

                //VERIFY SEQ
                if received.seq <= conn.seq { continue; } //IGNORE INVALID SEQs
                conn.seq = received.seq;

                //ACTIVITY TIMER
                conn.packet_accumulator += 1;
                if conn.packet_accumulator >= consts::ACTIVITY_TRESHOLD
                {
                    conn.packet_accumulator = 0; //RESET ACCUM
                    reset_last_activity(&id); //RESET ACTIVITY TIMER
                }
            } else //NOT FOUND, ADD ADDRESS
            {
                conn.0 = Some(Connection
                {
                    addr: addr,
                    id: id,
                    seq: 0,
                    server_seq: 0,
                    packet_accumulator: 0,
                });

                log::info!("New voice connection: {}", addr);
            }

            //SET USERNAME
            username = conn.1.clone();
        } else { continue; } //IGNORE UNRECOGNIZED CONNECTIONS

        //VALIDATE PACKET
        if received.code.is_none() && !validate_opus_packet(received.voice.as_deref()) { continue; } //IGNORE INVALID

        //FORWARD PONG (UNICAST)
        if received.code == Some(VoiceCode::PONG) && let Some(ref target_id) = received.target_id
        {
            if let Some(ref keys) = find_key(&target_id)
            {
                //FIND ADDRESS OF RECIPIENT (DA PINGA)
                let addr = match CONNECTIONS.get(target_id)
                    .and_then(|entry| entry.0.as_ref().map(|conn| conn.addr))
                {
                    Some(a) => a,
                    None => continue
                };

                //FORWARD
                voice::send(&socket, VoicePacket
                {
                    code: Some(VoiceCode::PONG),
                    id: Some(id),
                    timestamp: received.timestamp,
                    ..Default::default()
                }, &addr, target_id, keys).unwrap();
            }

            continue;
        }

        //FIND SENDER'S CHANNEL
        let sender_channel = find_channel(&id);

        //COLLECT ALL ADDRESSES
        let mut addresses: Vec<(SocketAddr, SharedKeys, usize)> = Vec::new();
        for connection in CONNECTIONS.iter()
        {
            if let (Some(conn), _) = connection.value()
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
                username: Some(username.to_string()),
                code: received.code.clone(),
                id: Some(id),
                timestamp: received.timestamp,
                ..Default::default()
            }, addr, recipient_id, keys).unwrap();
        }
    }
}
