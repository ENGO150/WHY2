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

//! # Voice Chat — Server Side
//!
//! One UDP socket serves every client on the server ([`listen_client_voice`]). A voice session
//! is opened over TCP ([`open_connection`]), which issues a token; the session's UDP address is
//! unknown until a [`Hello`](VoicePacketCode::Hello) carrying that token arrives and binds it.
//!
//! # Policing
//! Everything runs on the one shared loop, so nothing may wait there — a sleep would stall
//! every call on the server. A bound session therefore:
//!
//! - only moves to a new address on a fresh `Hello` or a port change on the same IP (a NAT
//!   rebinding); anything else from a foreign address is ignored;
//! - must raise its sequence number on every packet;
//! - is rate-limited by a token bucket at [`MAX_PACKET_RATE`](consts::MAX_PACKET_RATE) /
//!   [`MAX_PACKET_BURST`](consts::MAX_PACKET_BURST), where an overdrawn packet is **dropped**
//!   rather than delayed. This bounds amplification: each audio packet is fanned out to the
//!   whole channel.
//!
//! Audio from a muted user is dropped, and audio that is not a structurally valid Opus packet
//! is dropped before it is fanned out.

use std::
{
    mem,
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

/// A bound voice session.
pub struct Connection
{
    /// The client's UDP address.
    addr: SocketAddr,          //ADDRESS OF CONNECTION
    /// Last sequence number received.
    seq: usize,                //SEQUENCE NUMBER
    /// Last sequence number sent.
    server_seq: usize,         //SERVER SEQUENCE NUMBER
    /// Packets since the main connection's activity timer was last reset.
    packet_accumulator: usize, //PACKET ACCUMULATOR
    /// Tokens left in the packet-rate bucket.
    credit: f32,               //PACKET RATE TOKENS LEFT
    /// When the bucket was last topped up.
    refill: Instant,           //WHEN credit WAS LAST TOPPED UP
    /// Whether the last packet was already over the rate; only the first drop of a run is logged.
    throttled: bool,           //THE LAST PACKET WAS ALREADY OVER THE RATE
}

/// The current voice slot for one account.
pub struct VoiceSlot
{
    pub owner_addr: SocketAddr,
    pub username: String,
    pub channel: Option<String>,
    pub(super) token: [u8; 32],
    pub(super) connection: Option<Connection>,
}

struct Recipient
{
    id: usize,
    addr: SocketAddr,
    token: [u8; 32],
    keys: SharedKeys,
}

/// Voice slots by account ID.
pub(super) static CONNECTIONS: LazyLock<DashMap<usize, VoiceSlot>> = LazyLock::new(|| DashMap::new());

//IMPLEMENTATIONS
impl Connection
{
    //GET PEER ADDRESS
    /// Returns the client's UDP address.
    pub fn peer_addr(&self) -> &SocketAddr
    {
        &self.addr
    }

    //GET SEQ
    /// Returns the last sequence number received.
    pub fn seq(&self) -> &usize
    {
        &self.seq
    }

    //GET SERVER SEQ
    /// Returns the last sequence number sent.
    pub fn server_seq(&self) -> &usize
    {
        &self.server_seq
    }

    //GET SERVER SEQ AS MUTABLE
    /// Returns the last sequence number sent, for setting.
    pub fn server_seq_mut(&mut self) -> &mut usize
    {
        &mut self.server_seq
    }

    //PAY FOR ONE PACKET OUT OF THE RATE BUCKET
    /// Pays for one packet out of the rate bucket.
    ///
    /// The same bucket as the main connection's, at $r =$ [`MAX_PACKET_RATE`](consts::MAX_PACKET_RATE)
    /// and $b =$ [`MAX_PACKET_BURST`](consts::MAX_PACKET_BURST):
    ///
    /// $$ c \leftarrow \min(c + r\Delta t,\ b) - 1 $$
    ///
    /// with the packet accepted iff $c \ge 0$. A dropped packet still pays, so a flood keeps the
    /// bucket overdrawn for as long as it lasts.
    ///
    /// # Returns
    /// `true` if there was credit; `false` means the packet is dropped.
    fn take_credit(&mut self) -> bool
    {
        //TOP UP FOR THE ELAPSED TIME, THEN PAY
        self.credit = (self.credit + self.refill.elapsed().as_secs_f32() * consts::MAX_PACKET_RATE)
            .min(consts::MAX_PACKET_BURST) - 1.0;
        self.refill = Instant::now();

        self.credit >= 0.0
    }
}

impl Recipient
{
    fn from_slot(id: usize, slot: &VoiceSlot) -> Option<Self>
    {
        let addr = slot.connection.as_ref()?.addr;
        let keys = server::CONNECTIONS.get(&slot.owner_addr)?.keys()?.clone();
        Some(Self { id, addr, token: slot.token, keys })
    }

    async fn send(&self, socket: &UdpSocket, sender: usize, code: VoicePacketCode)
    {
        voice::send(socket, sender, code, &self.addr, &self.id, &self.token, &self.keys).await.ok();
    }
}

//HELPER FUNCTIONS
//PRIVATE
/// Reads an Opus frame length (one or two bytes).
///
/// # Returns
/// `(length, bytes the length took)`; `(0, 0)` if it is truncated.
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

/// Checks that a packet is structurally a valid Opus packet, by its TOC byte's framing code.
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

/// Compares two tokens in constant time.
fn tokens_match(a: &[u8; 32], b: &[u8; 32]) -> bool //COMPARE WITHOUT LEAKING WHERE THEY DIVERGE
{
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

//PUBLIC
/// Opens a voice session and returns its token and any displaced slot.
pub fn open_connection(id: usize, owner_addr: SocketAddr, username: String, channel: Option<String>) -> ([u8; 32], Option<VoiceSlot>)
{
    let mut token = [0u8; 32];
    SysRng.try_fill_bytes(&mut token).unwrap();

    let previous = CONNECTIONS.insert(id, VoiceSlot
    {
        owner_addr,
        username,
        channel,
        token,
        connection: None,
    });

    (token, previous)
}

/// Gets the slot token together with its owner's current keys.
pub(super) fn find_key(id: &usize) -> Option<([u8; 32], SharedKeys)>
{
    let slot = CONNECTIONS.get(id)?;
    let conn = server::CONNECTIONS.get(&slot.owner_addr)?;
    Some((slot.token, conn.keys()?.clone()))
}

/// Closes a voice slot if the requesting connection still owns it.
pub fn remove_connection(id: usize, owner_addr: SocketAddr, token: Option<&[u8; 32]>) -> Option<VoiceSlot>
{
    let (_, slot) = CONNECTIONS.remove_if(&id, |_, slot|
        slot.owner_addr == owner_addr && token.is_none_or(|token| tokens_match(token, &slot.token)))?;

    log::info!("Close voice connection ({}): {owner_addr}", if slot.connection.is_some() { "bound" } else { "never bound" });
    Some(slot)
}

/// Moves the owner's voice slot to another channel and returns the previous channel.
pub fn move_connection(id: usize, owner_addr: SocketAddr, channel: Option<String>) -> Option<Option<String>>
{
    let mut slot = CONNECTIONS.get_mut(&id)?;
    if slot.owner_addr != owner_addr { return None; }
    Some(mem::replace(&mut slot.channel, channel))
}

/// Builds a voice roster for a main connection's current channel.
pub fn roster(id: usize, channel: &Option<String>) -> Vec<(usize, String)>
{
    CONNECTIONS.iter().filter(|slot| *slot.key() != id && &slot.channel == channel)
        .map(|slot| (*slot.key(), slot.username.clone())).collect()
}

fn reset_last_activity(owner_addr: &SocketAddr)
{
    if let Some(mut conn) = server::CONNECTIONS.get_mut(owner_addr)
    {
        *conn.last_activity_mut() = Instant::now() -
            Duration::from_millis(config::read_config("min_message_delay"));
    }
}

fn recipients(sender: usize, channel: &Option<String>) -> Vec<Recipient>
{
    CONNECTIONS.iter().filter(|slot| *slot.key() != sender && &slot.channel == channel)
        .filter_map(|slot| Recipient::from_slot(*slot.key(), slot.value())).collect()
}

//MAIN FUNCTION
/// Serves voice for the whole server, forever.
///
/// Binds sessions on a valid `Hello`, polices every packet (see the module documentation), fans
/// audio and latency probes out to the sender's channel, and forwards probe answers to the
/// client that sent the probe.
///
/// # Parameters
/// - `socket`: The server's UDP socket.
pub async fn listen_client_voice(socket: UdpSocket)
{
    //LOOP RECEIVING
    loop
    {
        let (received, addr, slot_token) = voice::receive(&socket).await.unwrap();

        let (owner_addr, username, channel);

        //CHECK IF ID IS IN CONNECTIONS
        if let Some(mut entry) = CONNECTIONS.get_mut(&received.id)
        {
            //DROP PACKETS DECRYPTED BEFORE THE SLOT CHANGED OWNERS
            if !tokens_match(&slot_token, &entry.token) { continue; }
            owner_addr = entry.owner_addr;

            //ONLY A Hello WITH THE RIGHT TOKEN CLAIMS A SLOT
            let valid_hello = match received.code
            {
                VoicePacketCode::Hello { ref token } =>
                {
                    if !tokens_match(token, &entry.token) { continue; }
                    true
                },
                _ => false
            };

            //FOUND, CHECK ADDRESS
            if let Some(conn) = entry.connection.as_mut()
            {
                if conn.addr != addr
                {
                    //ANYTHING ELSE ONLY FOLLOWS A NAT SHIFT
                    if valid_hello || conn.addr.ip() == addr.ip()
                    {
                        log::debug!("Voice session moved ({}): main connection {owner_addr}",
                            if valid_hello { "hello" } else { "NAT port shift" });

                        conn.addr = addr;
                    } else
                    {
                        //IGNORE NON-MATCHING ADDRESS (SPOOFING)
                        log::warn!("Voice packet from an unexpected address ignored: main connection {owner_addr}");

                        continue;
                    }
                }

                //VERIFY SEQ
                if received.seq <= conn.seq { continue; } //IGNORE INVALID SEQs
                conn.seq = received.seq;

                //PACKET RATE
                if config::read_config("spam_protection") && !conn.take_credit()
                {
                    if !conn.throttled
                    {
                        conn.throttled = true;
                        log::warn!("Voice packets over the rate dropped: main connection {owner_addr}");
                    }

                    continue;
                }

                conn.throttled = false;

                //ACTIVITY TIMER
                conn.packet_accumulator += 1;
                if conn.packet_accumulator >= consts::ACTIVITY_TRESHOLD
                {
                    conn.packet_accumulator = 0; //RESET ACCUM
                    reset_last_activity(&owner_addr); //RESET ACTIVITY TIMER
                }
            } else //NOT BOUND YET, ADD ADDRESS
            {
                //WITHOUT THE TOKEN THERE IS NOTHING TO BIND TO
                if !valid_hello { continue; }

                entry.connection = Some(Connection
                {
                    addr: addr,
                    seq: received.seq,
                    server_seq: 0,
                    packet_accumulator: 0,
                    credit: consts::MAX_PACKET_BURST,
                    refill: Instant::now(),
                    throttled: false,
                });

                log::info!("New voice connection: {owner_addr}");
            }

            //SET USERNAME
            username = entry.username.clone();
            channel = entry.channel.clone();
        } else { continue; } //IGNORE UNRECOGNIZED CONNECTIONS

        match received.code
        {
            VoicePacketCode::Hello { .. } =>
            {
                let recipient = CONNECTIONS.get(&received.id)
                    .filter(|slot| tokens_match(&slot_token, &slot.token))
                    .and_then(|slot| Recipient::from_slot(received.id, slot.value()));
                if let Some(recipient) = recipient
                {
                    recipient.send(&socket, received.id, VoicePacketCode::HelloAck).await;
                }
            },

            VoicePacketCode::Audio { data, .. } =>
            {
                if server::CONNECTIONS.get(&owner_addr).is_none_or(|conn| *conn.muted()) { continue; }
                if !validate_opus_packet(&data)
                {
                    log::debug!("Malformed opus packet dropped: main connection {owner_addr}");
                    continue;
                }

                for recipient in recipients(received.id, &channel)
                {
                    recipient.send(&socket, received.id, VoicePacketCode::Audio
                    {
                        data: data.clone(),
                        username: Some(username.clone()),
                    }).await;
                }
            },

            VoicePacketCode::Ping { timestamp } =>
            {
                for recipient in recipients(received.id, &channel)
                {
                    recipient.send(&socket, received.id, VoicePacketCode::Ping { timestamp }).await;
                }
            },

            VoicePacketCode::Pong { target_id, timestamp } =>
            {
                let recipient = CONNECTIONS.get(&target_id)
                    .filter(|slot| slot.channel == channel)
                    .and_then(|slot| Recipient::from_slot(target_id, slot.value()));
                if let Some(recipient) = recipient
                {
                    recipient.send(&socket, received.id, VoicePacketCode::Pong { target_id, timestamp }).await;
                }
            },

            _ => {},
        }
    }
}
