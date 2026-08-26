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
    net::SocketAddr,
    time::{ Instant, Duration },
};

use tokio::net::UdpSocket;

use rand::
{
    TryRng,
    rngs::SysRng,
};

use dashmap::DashMap;

use crate::
{
    config,
    consts::SharedKeys,
    network::
    {
        server,
        voice::
        {
            self,
            consts,
            VoicePacketCode,
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
//CONNECTION (NONE UNTIL THE Hello ARRIVES), USERNAME, TOKEN THAT CLAIMS THE SLOT
pub static CONNECTIONS: LazyLock<DashMap<usize, (Option<Connection>, String, [u8; 32])>> = LazyLock::new(|| DashMap::new()); //LIST FOR EACH CLIENT CONNECTION

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

fn validate_opus_packet(packet: &[u8]) -> bool
{
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

fn tokens_match(a: &[u8; 32], b: &[u8; 32]) -> bool //COMPARE WITHOUT LEAKING WHERE THEY DIVERGE
{
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

//PUBLIC
pub fn open_connection(id: usize, username: String) -> [u8; 32] //OPEN A VOICE SLOT AND HAND OUT ITS TOKEN
{
    //GENERATE RANDOM TOKEN
    let mut token = [0u8; 32];
    SysRng.try_fill_bytes(&mut token).unwrap();

    //THE SLOT STAYS UNBOUND UNTIL A Hello CARRYING THIS TOKEN ARRIVES OVER UDP
    CONNECTIONS.insert(id, (None, username, token));

    token
}

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
    if let Some((_, (conn, _, _))) = CONNECTIONS.remove(id) &&
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
        *conn.last_activity_mut() = Instant::now() -
            Duration::from_millis(config::read_config("min_message_delay"));
    }
}

//MAIN FUNCTION
pub async fn listen_client_voice(socket: UdpSocket)
{
    //LOOP RECEIVING
    loop
    {
        let (received, addr) = voice::receive(&socket).await.unwrap();

        //CLIENT USERNAME
        let username: String;

        //CHECK IF ID IS IN CONNECTIONS
        if let Some(mut entry) = CONNECTIONS.get_mut(&received.id)
        {
            //A Hello IS THE ONLY PACKET THAT MAY CLAIM OR MOVE A SLOT, AND ONLY WITH THE RIGHT TOKEN
            let valid_hello = match received.code
            {
                VoicePacketCode::Hello { ref token } => tokens_match(token, &entry.2),
                _ => false
            };

            //FOUND, CHECK ADDRESS
            if let Some(conn) = entry.0.as_mut()
            {
                if conn.addr != addr
                {
                    //A Hello CARRYING THE TOKEN MAY MOVE THE SESSION ANYWHERE; ANYTHING ELSE ONLY FOLLOWS
                    //A NAT PORT SHIFT, WHICH KEEPS THE ADDRESS THE SESSION IS ALREADY ON
                    if valid_hello || conn.addr.ip() == addr.ip()
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
                    reset_last_activity(&received.id); //RESET ACTIVITY TIMER
                }
            } else //NOT BOUND YET, ADD ADDRESS
            {
                //WITHOUT THE TOKEN THERE IS NOTHING TO BIND TO: THE SLOT WAITS FOR THE CLIENT THAT WAS
                //HANDED IT OVER TCP, NOT FOR WHOEVER SPEAKS FIRST
                if !valid_hello { continue; }

                entry.0 = Some(Connection
                {
                    addr: addr,
                    id: received.id,
                    seq: received.seq,
                    server_seq: 0,
                    packet_accumulator: 0,
                });

                log::info!("New voice connection: {}", addr);
            }

            //SET USERNAME
            username = entry.1.clone();
        } else { continue; } //IGNORE UNRECOGNIZED CONNECTIONS

        //CODES
        match received.code
        {
            //HANDSHAKE - ANSWER EVERY Hello, INCLUDING THE REPEATS THAT CROSSED AN ACK ON THE WIRE
            VoicePacketCode::Hello { .. } =>
            {
                if let Some(ref keys) = find_key(&received.id)
                {
                    voice::send(&socket, received.id, VoicePacketCode::HelloAck, &addr, &received.id, keys).await.ok();
                }

                continue;
            },

            //AUDIO
            VoicePacketCode::Audio { data, .. } =>
            {
                //SILENCE MUTED USERS
                if *server::CONNECTIONS.iter().find(|c| c.id() == Some(&received.id)).unwrap().muted() { continue; }

                //VALIDATE PACKET IF IT CONTAINS AUDIO
                if !validate_opus_packet(&data) { continue; }

                //FIND SENDER'S CHANNEL
                let sender_channel = find_channel(&received.id);

                //COLLECT ALL ADDRESSES
                let mut addresses: Vec<(SocketAddr, SharedKeys, usize)> = Vec::new();
                for connection in CONNECTIONS.iter()
                {
                    if let (Some(conn), _, _) = connection.value()
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
                    voice::send(&socket, received.id, VoicePacketCode::Audio
                    {
                        data: data.clone(),
                        username: Some(username.clone()),
                    }, addr, recipient_id, keys).await.unwrap();
                }
            }

            //PING - BROADCAST TO ALL CLIENTS IN SAME CHANNEL
            VoicePacketCode::Ping { timestamp } =>
            {
                //FIND SENDER'S CHANNEL
                let sender_channel = find_channel(&received.id);

                //COLLECT ALL ADDRESSES
                let mut addresses: Vec<(SocketAddr, SharedKeys, usize)> = Vec::new();
                for connection in CONNECTIONS.iter()
                {
                    if let (Some(conn), _, _) = connection.value()
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
                    voice::send(&socket, received.id, VoicePacketCode::Ping
                    {
                        timestamp,
                    }, addr, recipient_id, keys).await.unwrap();
                }
            }

            //FORWARD PONG (UNICAST)
            VoicePacketCode::Pong { target_id, timestamp } =>
            {
                if let Some(ref keys) = find_key(&target_id)
                {
                    //FIND ADDRESS OF RECIPIENT (DA PINGA)
                    let addr = match CONNECTIONS.get(&target_id)
                        .and_then(|entry| entry.0.as_ref().map(|conn| conn.addr))
                    {
                        Some(a) => a,
                        None => continue
                    };

                    //FORWARD
                    voice::send(&socket, received.id, VoicePacketCode::Pong
                    {
                        target_id,
                        timestamp,
                    }, &addr, &target_id, keys).await.unwrap();
                }

                continue;
            },

            _ => {},
        }
    }
}
