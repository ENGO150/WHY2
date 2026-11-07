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

//! # Client Handshake
//!
//! Dialing a server and the client's half of the key exchange, including the trust-on-first-use
//! check of the server identity. The protocol itself is described in [`crate::crypto::kex`].

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
    t,
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
/// The outcome of a key exchange.
#[derive(PartialEq)]
pub enum Handshake
{
    /// Keys are agreed; the session can start.
    Ready,     //KEYS AGREED - THE SESSION CAN START
    /// The user just pinned this server's key; the client disconnects and dials again so the session
    /// starts under a trusted identity.
    Reconnect, //THE USER JUST PINNED THIS KEY
    /// The exchange was refused; the client is done.
    Failed,    //REFUSED - THE CLIENT IS DONE
}

//FUNCTIONS
//PUBLIC
/// Performs the client's half of a key exchange, for a new session or a rekey.
///
/// The offer's signature is verified against the nonce this client sent (its obfuscation key),
/// and the identity is checked against the pinned one ([`keys::check`]). An unknown or changed
/// identity is put to the user through [`ClientEvent::TofuPrompt`]; a refusal ends the
/// session. With `WHY2_SKIP_TOFU` set at build time the check is skipped and reported as
/// [`ClientEvent::TofuSkip`].
///
/// On success the new keys are written to `keys`, to [`options`], and the server fingerprint is
/// set, which scopes the image cache.
///
/// # Parameters
/// - `streams`: The main connection.
/// - `keys`: Receives the new session keys.
/// - `tx`: The UI event channel.
/// - `exchange_keys`: The current keys during a rekey, `None` for a new session.
///
/// # Returns
/// The [`Handshake`] outcome.
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
            tx.send(ClientEvent::Quit { graceful: false }).await.ok();

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
        tx.send(ClientEvent::HandshakeFailed { reason: t!("login.unsigned_keys").to_owned() }).await.ok();

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
        None => tx.send(ClientEvent::TofuSkip { hash }).await.unwrap(),

        Some(TofuCode::Valid) => {},

        Some(status) =>
        {
            //ASK THE USER IN THE TUI
            let (reply, answer) = oneshot::channel();

            tx.send(ClientEvent::TofuPrompt
            {
                request: TofuRequest
                {
                    host: host.clone(),
                    hash: hash.clone(),
                    mismatch: matches!(status, TofuCode::Mismatch),
                    pinned: keys::pinned(&host),
                    reply,
                },
            }).await.unwrap();

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

/// Replaces the main connection with a fresh dial to the same server and resets both sequence
/// counters.
///
/// # Returns
/// `true` if the dial succeeded.
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

/// Dials a server, through SOCKS5 if enabled, within [`CONNECT_TIMEOUT`](consts::CONNECT_TIMEOUT).
///
/// The socket has `TCP_NODELAY` set and is split into halves.
///
/// # Parameters
/// - `connecting_addr`: The server address, `host:port`.
///
/// # Returns
/// - `Ok((read, write))` on success.
/// - `Err(Error)` if the dial failed or timed out.
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
        .unwrap_or_else(|_| Err(Error::new(ErrorKind::TimedOut, t!("login.timed_out"))))
        .and_then(|s|
        {
            //SET TCP_NODELAY
            s.set_nodelay(true)?;
            Ok(s.into_split())
        })
}
