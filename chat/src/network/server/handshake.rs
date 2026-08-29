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
    time::Duration,
    net::{ IpAddr, SocketAddr },
    sync::
    {
        LazyLock,
        atomic::{ AtomicUsize, Ordering },
    },
};

use tokio::net::tcp::OwnedWriteHalf;

use dashmap::DashMap;

use crate::
{
    config,
    options,
    misc,
    crypto::kex,
    consts::
    {
        self,
        SharedKeys,
        Streams,
    },
    network::
    {
        self,
        codes::PacketCode,
    },
};

//STRUCTS
pub struct HandshakeSlot //RESERVATION HELD BY A SOCKET THAT HAS NOT IDENTIFIED ITSELF YET (SEE HANDSHAKE BUDGET)
{
    ip: IpAddr, //PEER THE SLOT WAS TAKEN FOR
}

//HANDSHAKE BUDGET
fn max_handshakes() -> usize
{
    (config::read_config::<usize>("max_clients") + config::read_config::<usize>("max_unauth_clients")) * consts::MAX_HANDSHAKES_PER_IP
}
static HANDSHAKES: AtomicUsize = AtomicUsize::new(0);
static HANDSHAKES_PER_IP: LazyLock<DashMap<IpAddr, usize>> = LazyLock::new(|| DashMap::new());

//IMPLEMENTATIONS
impl HandshakeSlot
{
    //TAKE A SLOT FOR A FRESHLY ACCEPTED SOCKET, None IF THE BUDGET IS FULL
    pub fn reserve(ip: IpAddr) -> Option<Self>
    {
        if HANDSHAKES.load(Ordering::Relaxed) >= max_handshakes() { return None; }

        //PER-IP SO ONE PEER CANNOT TAKE THE WHOLE BUDGET
        {
            let mut slots = HANDSHAKES_PER_IP.entry(ip).or_insert(0);
            if *slots >= consts::MAX_HANDSHAKES_PER_IP { return None; }

            *slots += 1;
        }

        HANDSHAKES.fetch_add(1, Ordering::Relaxed);

        Some(Self { ip })
    }
}

impl Drop for HandshakeSlot
{
    //RELEASE THE SLOT, WHICHEVER WAY THE HANDSHAKE ENDED
    fn drop(&mut self)
    {
        HANDSHAKES.fetch_sub(1, Ordering::Relaxed);

        //DROP THE GUARD BEFORE REMOVING - BOTH TOUCH THE SAME SHARD
        let empty = if let Some(mut slots) = HANDSHAKES_PER_IP.get_mut(&self.ip)
        {
            *slots = slots.saturating_sub(1);
            *slots == 0
        } else { false };

        if empty { HANDSHAKES_PER_IP.remove_if(&self.ip, |_, slots| *slots == 0); }
    }
}

//PRIVATE
async fn untrusted_read<F>(streams: &mut Streams<'_>, is_match: F, keys: Option<&SharedKeys>) -> Option<PacketCode>
where
    F: Fn(&PacketCode) -> bool
{
    let mut invalid_packets = 0; //INVALID KEY EXCHANGE PACKETS COUNTER

    //WAIT FOR KeyExchange
    let message = loop
    {
        //READ MESSAGE (WITH TIMEOUT FOR ZOMBIE CONNECTIONS)
        let received = match tokio::time::timeout(Duration::from_millis(2000), network::receive(streams, keys, None)).await
        {
            Ok(Some(r)) => r,
            _ => return None
        };

        if is_match(&received) { break received; }

        //CHECK INVALID PACKETS COUNTER
        if invalid_packets == 3 { return None; }
        invalid_packets += 1; //INCREMENT
    };

    Some(message)
}

pub(super) async fn key_exchange //KEY EXCHANGE FOR SERVER-SIDE
(
    streams: &mut Streams<'_>,
    peer_addr: &SocketAddr,
    nonce: &[u8; 32],
    keys: &mut SharedKeys,
    rekey_trigger: Option<&SharedKeys>,
)
{
    //SIGN A FRESH EPHEMERAL PAIR WITH THE STATIC IDENTITY
    let (ephemeral, offer) = kex::create_offer(nonce);

    //ATOMIC SEND
    {
        let mut write = streams.1.lock().await;

        //TRIGGER REKEY
        let keys = if let Some(current_keys) = rekey_trigger
        {
            network::send(&mut write, PacketCode::Rekey, Some(current_keys)).await;

            //ENCRYPT PUBKEYS
            Some(current_keys)
        } else { None }; //OBFUSCATE PUBKEYS

        //SEND SIGNED OFFER TO CLIENT
        network::send(&mut write, PacketCode::KeyExchangeOffer { offer }, keys).await;
    }

    //READ FROM UNTRUSTED CLIENT
    let message = match untrusted_read(streams, |code| matches!(code, PacketCode::KeyExchangeReply { .. }), rekey_trigger).await
    {
        Some(r) => r,
        None => return
    };

    //DERIVE SHARED KEYS - THE PACKET SCHEMA ALREADY PROVED BOTH HALVES ARE KEYS, SO NOTHING CAN FAIL HERE
    let PacketCode::KeyExchangeReply { reply } = message else { unreachable!("what"); };

    //DECAPSULATE PQ
    let pq_secret = kex::decapsulate_pq(&ephemeral, &reply.pq);

    //DERIVE KEYS - THIS CONSUMES THE EPHEMERAL SECRET
    let new_keys = kex::derive_shared_secret(ephemeral.into_ecc(), &reply.eph_ecc, pq_secret);

    //UPDATE CLIENT KEYS
    super::update_client_keys(peer_addr, &new_keys);
    *keys = new_keys;
}

pub(super) async fn send_welcome_packet(write_stream: &mut OwnedWriteHalf, keys: &SharedKeys) //send welcome packet you idiot
{
    //SEND
    network::send(write_stream, PacketCode::Welcome
    {
        min_pass: config::read_config::<u64>("min_password_length"),
        max_uname: config::read_config::<u64>("max_username_length"),
        min_uname: config::read_config::<u64>("min_username_length"),
        server_name: config::read_config::<String>("server_name"),
        server_uname: options::get_server_username(),
        git_hash: env!("WHY2_GIT_HASH").to_owned(),
    }, Some(keys)).await;
}

pub(super) async fn ask_version(streams: &mut Streams<'_>, keys: &SharedKeys) -> Option<String> //ASK CLIENT FOR VERSION
{
    //ASK FOR VERSION
    network::send(&mut *streams.1.lock().await,
        PacketCode::Version { version: Some(misc::get_version().to_string()) }, Some(keys)).await;

    //READ FROM UNTRUSTED CLIENT
    let read = untrusted_read(streams, |code| matches!(code, PacketCode::Version { .. }), Some(keys)).await?;

    if let PacketCode::Version { version } = read
    {
        return version;
    } { unreachable!("what"); }
}
