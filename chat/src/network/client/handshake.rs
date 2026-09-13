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
    io::{ Error, ErrorKind },
};

use tokio::
{
    time,
    sync::
    {
        oneshot,
        mpsc::Sender,
    },
    net::
    {
        TcpStream,
        tcp::{ OwnedReadHalf, OwnedWriteHalf },
    },
};

use tokio_socks::tcp::Socks5Stream;

use crate::
{
    crypto::kex,
    options,
    config::
    {
        self,
        keys::{ self, TofuCode },
    },
    consts::
    {
        self,
        Streams,
        SharedKeys,
    },
    network::
    {
        self,
        schema,
        codes::PacketCode,
        client::{ ClientEvent, TofuRequest },
    },
};

//ENUMS
#[derive(PartialEq)]
pub enum Handshake
{
    Ready,     //KEYS AGREED - THE SESSION CAN START
    Reconnect, //THE USER JUST PINNED THIS KEY
    Failed,    //REFUSED - THE CLIENT IS DONE
}

//FUNCTIONS
//PUBLIC
pub async fn key_exchange
(
    streams: &mut Streams<'_>,
    keys: &mut SharedKeys,
    tx: &Sender<ClientEvent>,
    exchange_keys: Option<&SharedKeys>,
) -> Handshake //KEY EXCHANGE FOR CLIENT-SIDE
{
    //WAIT FOR KeyExchangeOffer
    let offer = loop
    {
        //READ MESSAGE
        let Some(received) = network::receive(streams, exchange_keys, None).await else
        {
            //THE SERVER WENT AWAY MID-HANDSHAKE
            tx.send(ClientEvent::Quit).await.ok();

            return Handshake::Failed;
        };

        if let PacketCode::KeyExchangeOffer { offer } = received { break offer; }
    };

    //VERIFY PUBKEY VALIDITY (TOFU)
    let host = streams.0.peer_addr().unwrap().ip().to_string();
    let verdict = if env!("WHY2_SKIP_TOFU") == "false"
    {
        Some(keys::check(&host, &kex::public_bytes(&offer.static_ecc)))
    } else { None };

    //THE STATIC KEY SIGNS THE EPHEMERAL ONES
    if !kex::verify_offer(&options::get_obfuscation_key(), &offer.static_ecc, &offer.eph_ecc, &offer.pq, &offer.sig)
    {
        tx.send(ClientEvent::HandshakeFailed(String::from("Server identity did not sign its exchange keys."))).await.ok();

        return Handshake::Failed;
    }

    //GENERATE EPHEMERAL ECC KEYS
    let (sk, pk) = kex::generate_ephemeral_keys();

    //ENCAPSULATE PQ
    let (pq_ciphertext, pq_secret) = kex::encapsulate_pq(&offer.pq);

    //SEND PUBKEYS TO SERVER
    network::send(&mut *streams.1.lock().await, PacketCode::KeyExchangeReply
    {
        reply: Box::new(schema::Reply { eph_ecc: pk, pq: pq_ciphertext }),
    }, exchange_keys).await;

    //CALCULATE SHARED SECRET (HYBRID)
    *keys = kex::derive_shared_secret(sk, &offer.eph_ecc, pq_secret);

    //SET GLOBAL VARIABLES
    options::set_keys(keys.clone());

    //ACT ON THE TOFU VERDICT
    let hash = keys::hash(&kex::public_bytes(&offer.static_ecc));

    //SET SERVER FINGERPRINT
    options::set_fingerprint(&hash);

    match verdict
    {
        //VERIFICATION DISABLED AT BUILD TIME
        None => tx.send(ClientEvent::TofuSkip(hash)).await.unwrap(),

        Some(TofuCode::Valid) => {},

        Some(status) =>
        {
            //ASK THE USER IN THE TUI
            let (reply, answer) = oneshot::channel();

            tx.send(ClientEvent::TofuPrompt(TofuRequest
            {
                host: host.clone(),
                hash: hash.clone(),
                mismatch: matches!(status, TofuCode::Mismatch),
                pinned: keys::pinned(&host),
                reply,
            })).await.unwrap();

            //A DROPPED SENDER COUNTS AS A REFUSAL
            if !answer.await.unwrap_or(false)
            {
                //GRACEFULLY DISCONNECT FROM SERVER
                network::send(&mut *streams.1.lock().await, PacketCode::Disconnect, Some(keys)).await;

                //END THE SESSION
                tx.send(ClientEvent::TofuError).await.unwrap();

                //EXIT
                return Handshake::Failed;
            }

            //PIN THE KEY
            keys::save(&host, &hash);

            if exchange_keys.is_none()
            {
                //GRACEFULLY DISCONNECT FROM SERVER
                network::send(&mut *streams.1.lock().await, PacketCode::Disconnect, Some(keys)).await;

                return Handshake::Reconnect;
            }
        },
    }

    Handshake::Ready
}

pub async fn reconnect(streams: &mut Streams<'_>) -> bool
{
    let Ok((read_half, write_half)) = connect(options::get_server_address()).await else { return false };

    *streams.0 = read_half;
    *streams.1.lock().await = write_half;

    //A NEW CONNECTION COUNTS FROM ZERO ON BOTH SIDES
    options::set_seq(0);
    options::set_server_seq(0);

    true
}

pub async fn connect(connecting_addr: String) -> Result<(OwnedReadHalf, OwnedWriteHalf), Error> //CONNECT TO SERVER
{
    let dial = async
    {
        if !options::socks5_enabled() //NO SOCKS5
        {
            TcpStream::connect(connecting_addr).await
        } else //USE PROXY
        {
            let proxy_addr = config::read_config::<String>("socks5_addr");

            Socks5Stream::connect(proxy_addr.as_str(), connecting_addr.as_str()).await
                .map(|s| s.into_inner())
                .map_err(Error::other)
        }
    };

    time::timeout(Duration::from_millis(consts::CONNECT_TIMEOUT), dial).await
        .unwrap_or_else(|_| Err(Error::new(ErrorKind::TimedOut, "Connection timed out.")))
        .and_then(|s|
        {
            //SET TCP_NODELAY
            s.set_nodelay(true)?;
            Ok(s.into_split())
        })
}
