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

//! # Packet and At-Rest Cryptography
//!
//! Glue between the raw [`why2`] primitives and the chat protocol. Nothing outside this module
//! touches a cipher directly.
//!
//! # Overview
//! Three kinds of data are protected here, each with its own construction:
//!
//! 1. **Packets on the main connection** — [`encrypt_packet`] / [`decrypt_packet`]: one-shot
//!    REX encryption under the session keys, authenticated with
//!    [`AuthenticatedData`] (encrypt-then-MAC).
//! 2. **Packets on auxiliary sockets** (files, screen shares) — [`RexPacketStream`]: a
//!    [`RexStream`] wrapped in encrypt-then-MAC, with a packet counter bound into every tag so
//!    a replayed, dropped or reordered frame fails to verify instead of decrypting to garbage.
//! 3. **Files at rest** (the server's picture store, the client's cache, the history) —
//!    per-file keys derived from a store's master key and a trailing tag from [`disk_tag`].
//!
//! # Encryption Migration
//! The previous release (why2-chat 2.4.0, core 1.5.2/1.5.3) drew the `shift_rows` shifts from a
//! different round key, and therefore used a different CTR keystream. The HMAC and file layout
//! did not change, so a file from that release still verifies but decrypts to garbage. `legacy`
//! freezes the previous keystream; `apply_file_cipher_legacy` and `decrypt_packet_legacy` read
//! such files once so the history, picture store and cache can be rewritten with the current
//! encryption. Older releases are not migrated. The migration code is removed with the next
//! version bump.
//!
//! # Key Derivation
//! Every derived key is HKDF-SHA256 under a distinct label, so no two uses ever share a
//! keystream:
//!
//! - A session key of the wrong size is stretched to the grid's key size.
//! - A stream's nonce and MAC key are derived from its transfer token;
//!   the reverse direction of the same
//!   socket hashes the token under its own label ([`init_reverse_stream`]).
//! - A file's key, nonce and MAC key are derived from a per-purpose master key salted with the
//!   file's content hash (`image_keys`, `cache_keys`).
//!
//! Writing $\mathrm{HKDF}(s, k, l)$ for HKDF-SHA256 with salt $s$, input key material $k$ and
//! label $l$, $\Vert$ for concatenation, $K$ and $K\_m$ for the session's grid key and HMAC key,
//! and $\tau$ for a transfer token:
//!
//! $$ K^{\prime} = \mathrm{HKDF}(\varnothing,\ K,\ \texttt{WHY2-DERIVED-KEY-}W\texttt{x}H) $$
//!
//! $$ N\_\tau = \mathrm{HKDF}(\varnothing,\ \tau,\ \texttt{WHY2-STREAM-NONCE}) \qquad K\_\tau = \mathrm{HKDF}(\tau,\ K\_m,\ \texttt{WHY2-STREAM-MAC}) $$
//!
//! $$ \tau^{\prime} = \mathrm{SHA256}(\texttt{WHY2-REVERSE-STREAM} \Vert \tau) $$
//!
//! and for a file named by its hash $h$, in a store with master key $M$ (the server's image
//! key, or the client's cache key), salted with $s = h$ on the server and
//! $s = \mathrm{fp} \Vert h$ in the client's cache:
//!
//! $$ K\_f = \mathrm{HKDF}(s,\ M,\ \texttt{WHY2-IMAGE-KEY}) \qquad N\_f = \mathrm{HKDF}(s,\ M,\ \texttt{WHY2-IMAGE-NONCE}) \qquad K\_{f,m} = \mathrm{HKDF}(s,\ M,\ \texttt{WHY2-DISK-MAC}) $$

//MODULES
#[cfg(feature = "server")]
pub mod password;
pub mod kex;

#[cfg(feature = "chat")]
pub(crate) mod legacy;

use why2::
{
    encrypter,
    decrypter,
    grid::Grid,
    auth::{ AuthenticatedData, AuthenticatedPacket },
};

use zeroize::Zeroizing;

use hkdf::Hkdf;
use sha2::{ Sha256, Digest };

use hmac::
{
    Mac,
    Hmac,
    KeyInit,
};

use why2::
{
    consts,
    stream::RexStream,
};

use crate::consts::
{
    self as consts_chat,
    DiskKeys,
    SharedKeys,
};

//STRUCTS
/// An authenticated stream cipher: encrypt-then-MAC over a [`RexStream`].
///
/// Each [`seal`](Self::seal)ed packet is framed as `[32-byte tag][ciphertext]`, where the tag is
/// HMAC-SHA256 over `counter || len || ciphertext` under a stream-specific key. Binding the
/// counter is what makes this a stream rather than a bag of individually valid packets: the
/// receiver's counter must match the sender's, so replay and reordering are rejected.
///
/// For the $i$-th packet in either direction, with ciphertext $C\_i$ of $\ell\_i$ bytes and the
/// stream's MAC key $K\_\tau$ (see the module documentation):
///
/// $$ T\_i = \mathrm{HMAC}\_{K\_\tau}\left(\mathrm{be64}(i) \Vert \mathrm{be64}(\ell\_i) \Vert C\_i\right) $$
///
/// and the packet is $T\_i \Vert C\_i$.
///
/// # Cryptographic Safety
/// Both ends must process packets in the same order, and a failed [`open`](Self::open) means the
/// peer is not who it was; the caller ends the transfer.
pub struct RexPacketStream //AUTHENTICATED STREAM CIPHER (ENCRYPT-THEN-MAC OVER A RexStream)
{
    stream: RexStream,             //CTR MODE STREAM
    mac_key: Zeroizing<[u8; 32]>,  //HMAC KEY (STREAM SPECIFIC)
    counter: u64,                  //PACKET COUNTER (BOUND INTO EVERY TAG)
}

//IMPLEMENTATIONS
impl RexPacketStream
{
    //PRIVATE
    /// Primes an HMAC over `counter || len || ciphertext` at the current counter.
    fn mac(&self, ciphertext: &[u8]) -> Hmac<Sha256> //PRIME MAC OVER counter || len || ciphertext
    {
        let mut mac = <Hmac<Sha256>>::new_from_slice(self.mac_key.as_ref()).expect("Invalid MAC key");

        mac.update(&self.counter.to_be_bytes());
        mac.update(&(ciphertext.len() as u64).to_be_bytes());
        mac.update(ciphertext);

        mac
    }

    //PUBLIC
    /// Encrypts and authenticates one packet, advancing the counter.
    ///
    /// # Parameters
    /// - `plaintext`: The packet bytes to protect.
    ///
    /// # Returns
    /// The framed packet, `[tag][ciphertext]`; the ciphertext is the length of `plaintext`.
    ///
    /// # Panics
    /// If the underlying [`RexStream`] fails, which only happens on invalid grid dimensions.
    pub fn seal(&mut self, plaintext: &[u8]) -> Vec<u8> //ENCRYPT AND AUTHENTICATE ([TAG][CIPHERTEXT])
    {
        //CONVERT PLAINTEXT TO i64
        let input_i64 = Zeroizing::new(bytes_to_i64(plaintext));

        //ENCRYPT
        let mut encrypted_i64 = Zeroizing::new(self.stream.update(&input_i64).expect("Stream encryption failed"));

        //FLUSH
        encrypted_i64.extend(self.stream.finalize().expect("Stream finalize failed"));

        //CONVERT ENCRYPTED BYTES BACK TO u8
        let mut ciphertext = i64_to_bytes(&encrypted_i64);

        //TRUNCATE PADDING
        ciphertext.truncate(plaintext.len());

        //AUTHENTICATE
        let tag = self.mac(&ciphertext).finalize().into_bytes();
        self.counter += 1;

        //[TAG][CIPHERTEXT]
        let mut output = Vec::with_capacity(tag.len() + ciphertext.len());
        output.extend_from_slice(&tag);
        output.append(&mut ciphertext);

        output
    }

    /// Verifies and decrypts one packet, advancing the counter.
    ///
    /// The tag is verified in constant time **before** the stream is advanced, so a forged packet
    /// leaves the stream state untouched.
    ///
    /// # Parameters
    /// - `data`: A framed packet, `[tag][ciphertext]`.
    ///
    /// # Returns
    /// - `Some(plaintext)` if the tag verifies at the current counter.
    /// - `None` if the packet is too short, forged, replayed or out of order.
    pub fn open(&mut self, data: &[u8]) -> Option<Zeroizing<Vec<u8>>> //VERIFY AND DECRYPT
    {
        //SPLIT OFF THE TAG
        if data.len() < 32 { return None; }
        let (tag, ciphertext) = data.split_at(32);

        //VERIFY BEFORE DECRYPTING (CONSTANT TIME)
        self.mac(ciphertext).verify_slice(tag).ok()?;
        self.counter += 1;

        //CONVERT u8 TO i64
        let input_i64 = Zeroizing::new(bytes_to_i64(ciphertext));

        //DECRYPT
        let mut decrypted_i64 = Zeroizing::new(self.stream.update(&input_i64).ok()?);

        //FLUSH
        decrypted_i64.extend(self.stream.finalize().ok()?);

        //CONVERT BACK TO u8
        let mut output = Zeroizing::new(i64_to_bytes(&decrypted_i64));

        //TRUNCATE PADDING
        output.truncate(ciphertext.len());

        Some(output)
    }
}

//FUNCTIONS
//PRIVATE
/// Stretches a session key to the `W`×`H` grid's key size with HKDF.
///
/// # Type Parameters
/// - `W`, `H`: The target grid dimensions.
///
/// # Returns
/// A key of `W * H * 2` cells.
fn get_correct_key<const W: usize, const H: usize>(key: &Zeroizing<Vec<i64>>) -> Zeroizing<Vec<i64>> //DERIVE VALID KEYDIM USING HKDF
{
    //CONVERT KEY TO BYTES
    let mut key_bytes = Zeroizing::new(Vec::with_capacity(key.len() * 8));
    for val in key.iter()
    {
        key_bytes.extend_from_slice(&val.to_be_bytes());
    }

    //INIT HKDF
    let hkdf = Hkdf::<Sha256>::new(None, &key_bytes);

    let required_len = W * H * 2;
    let needed_bytes = required_len * 8;
    let mut output_bytes = Zeroizing::new(vec![0u8; needed_bytes]);

    //EXPAND
    hkdf.expand(format!("WHY2-DERIVED-KEY-{W}x{H}").as_bytes(), &mut output_bytes).expect("Key derivation failed");

    //CONVERT BACK TO i64
    let mut derived_key = Vec::with_capacity(required_len);
    for chunk in output_bytes.chunks_exact(8)
    {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(chunk);
        derived_key.push(i64::from_be_bytes(buf));
    }

    Zeroizing::new(derived_key)
}

/// Returns the grid key a packet was encrypted with, stretching it if the caller's key has the
/// wrong size.
fn packet_key<const W: usize, const H: usize>(keys: &SharedKeys) -> Option<Grid<W, H>>
{
    if keys.0.len() == W * H * 2
    {
        Grid::from_key(&keys.0).ok()
    } else
    {
        let derived = get_correct_key::<W, H>(&keys.0);
        Grid::from_key(&derived).ok()
    }
}

/// Derives a stream nonce grid from a transfer context (its token).
fn derive_stream_nonce(context: &[u8]) -> Zeroizing<Vec<i64>>
{
    let hkdf = Hkdf::<Sha256>::new(None, context);

    let mut okm = Zeroizing::new(vec![0u8; consts::DEFAULT_GRID_WIDTH * consts::DEFAULT_GRID_HEIGHT * 8]);
    hkdf.expand(b"WHY2-STREAM-NONCE", &mut okm).expect("HKDF expand failed");

    Zeroizing::new(okm.chunks_exact(8).map(|c| i64::from_be_bytes(c.try_into().unwrap())).collect())
}

/// Derives a stream-specific MAC key from the session HMAC key, salted with the transfer context.
fn derive_stream_mac_key(hmac_key: &[u8], context: &[u8]) -> Zeroizing<[u8; 32]> //STREAM SPECIFIC MAC KEY
{
    let hkdf = Hkdf::<Sha256>::new(Some(context), hmac_key);

    let mut okm = Zeroizing::new([0u8; 32]);
    hkdf.expand(b"WHY2-STREAM-MAC", okm.as_mut()).expect("HKDF expand failed");

    okm
}

//CRATE PUBLIC
/// Packs bytes into big-endian `i64` cells, zero-padding the last one.
pub(crate) fn bytes_to_i64(bytes: &[u8]) -> Vec<i64>
{
    bytes.chunks(8).map(|chunk|
    {
        let mut buf = [0u8; 8];
        buf[..chunk.len()].copy_from_slice(chunk);
        i64::from_be_bytes(buf)
    }).collect()
}

/// Unpacks big-endian `i64` cells into bytes.
pub(crate) fn i64_to_bytes(vals: &[i64]) -> Vec<u8>
{
    vals.iter().flat_map(|v| v.to_be_bytes()).collect()
}

//PUBLIC
/// Returns the SHA-256 digest of `data`.
pub fn sha256(data: &[u8]) -> [u8; 32] //SHA-256 OF A BYTE STRING
{
    //SHA256
    let mut hasher = Sha256::new();
    hasher.update(data);

    //FINALIZE
    hasher.finalize().into()
}

/// Encrypts and authenticates one packet under the session keys.
///
/// # Type Parameters
/// - `W`, `H`: The grid dimensions to encrypt with.
///
/// # Parameters
/// - `packet_bytes`: The serialized packet.
/// - `keys`: The session keys; a grid key of the wrong size is stretched with HKDF.
///
/// # Returns
/// The serialized authenticated packet: MAC, nonce, and ciphertext.
///
/// # Panics
/// If encryption fails or the HMAC key is not 32 bytes.
pub fn encrypt_packet<const W: usize, const H: usize>(packet_bytes: &[u8], keys: &SharedKeys) -> Vec<u8>
{
    //CONVERT packet_bytes to BINARY
    let input_i64 = Zeroizing::new(bytes_to_i64(&packet_bytes));

    //GET VALID KEY
    let key = if keys.0.len() == W * H * 2
    {
        &keys.0
    } else
    {
        &get_correct_key::<W, H>(&keys.0)
    };

    //ENCRYPT
    let encrypted_data = encrypter::encrypt::<W, H>(&input_i64, Some(key))
        .expect("Encrypting packet failed");

    //AUTHENTICATE
    AuthenticatedData::authenticate(encrypted_data, keys.1.as_slice().try_into().unwrap()).into()
}

/// Verifies and decrypts one packet under the session keys.
///
/// # Type Parameters
/// - `W`, `H`: The grid dimensions the packet was encrypted with.
///
/// # Parameters
/// - `decoded_packet`: The serialized [`AuthenticatedPacket`].
/// - `keys`: The session keys.
///
/// # Returns
/// - `Some(plaintext)` if the packet parses and its tag verifies.
/// - `None` otherwise. The plaintext is padded to whole grids; the caller's decoder ignores the
///   padding.
pub fn decrypt_packet<const W: usize, const H: usize>(decoded_packet: Vec<u8>, keys: &SharedKeys) -> Option<Zeroizing<Vec<u8>>>
{
    //DESERIALIZE
    let auth_packet: AuthenticatedPacket<W, H> = decoded_packet.as_slice().try_into().ok()?;

    //GET VALID KEY
    let key_grid = packet_key::<W, H>(keys)?;

    //VERIFY HMAC AND ATTACH THE KEY
    let encrypted_data = auth_packet.open(keys.1.as_slice().try_into().ok()?, key_grid)?;

    //DECRYPT
    let decrypted_packet = decrypter::decrypt(encrypted_data).ok()?;

    Some(Zeroizing::new(i64_to_bytes(&decrypted_packet.output)))
}

/// Verifies and decrypts one packet written by the previous release.
///
/// The packet layout and HMAC are unchanged; only the CTR keystream differs. This exists solely
/// so [`messages`](crate::config::messages) can read a history file from that release once and
/// re-save it under the current encryption.
#[cfg(feature = "server")]
pub(crate) fn decrypt_packet_legacy<const W: usize, const H: usize>(decoded_packet: Vec<u8>, keys: &SharedKeys) -> Option<Zeroizing<Vec<u8>>>
{
    //DESERIALIZE
    let auth_packet: AuthenticatedPacket<W, H> = decoded_packet.as_slice().try_into().ok()?;

    //GET VALID KEY
    let key_grid = packet_key::<W, H>(keys)?;

    //VERIFY HMAC AND ATTACH THE KEY
    let encrypted_data = auth_packet.open(keys.1.as_slice().try_into().ok()?, key_grid)?;

    //DECRYPT WITH THE FROZEN PREVIOUS-RELEASE KEYSTREAM
    let round_keys = why2::crypto::generate_round_keys(&encrypted_data.key).ok()?;
    let mut output = encrypted_data.output;

    legacy::apply_ctr(&mut output, &encrypted_data.nonce, &round_keys, 0);

    let decrypted = Zeroizing::new(output.iter().flat_map(Grid::to_flat).collect::<Vec<i64>>());

    Some(Zeroizing::new(i64_to_bytes(&decrypted)))
}

/// Opens the stream for the reverse direction of an auxiliary socket.
///
/// The token is hashed under its own label first, so the two directions never share a keystream.
///
/// # Parameters
/// - `keys`: The session keys.
/// - `token`: The transfer token the forward stream was opened with.
///
/// # Returns
/// - `Some(stream)` on success.
/// - `None` if the keys do not form a valid grid.
pub fn init_reverse_stream(keys: &SharedKeys, token: &[u8; 32]) -> Option<RexPacketStream> //THE SAME SOCKET, THE OTHER DIRECTION
{
    let mut hasher = Sha256::new();
    hasher.update(b"WHY2-REVERSE-STREAM");
    hasher.update(token);

    init_rex_stream(keys, &hasher.finalize().into())
}

/// Opens an authenticated packet stream for an auxiliary socket.
///
/// # Parameters
/// - `keys`: The session keys.
/// - `token`: The transfer token; it seeds the nonce and the stream's MAC key.
///
/// # Returns
/// - `Some(stream)` on success.
/// - `None` if the keys do not form a valid grid.
pub fn init_rex_stream(keys: &SharedKeys, token: &[u8; 32]) -> Option<RexPacketStream>
{
    let key_grid = Grid::from_key(&keys.0).ok()?;

    let derived = derive_stream_nonce(token);

    //RECONSRUCT NONCE GRID
    let nonce_grid = Grid::from_flat(&derived).ok()?;

    //INIT & RETURN
    Some(RexPacketStream
    {
        stream: RexStream::new(&key_grid, nonce_grid).ok()?,
        mac_key: derive_stream_mac_key(&keys.1, token),
        counter: 0,
    })
}

//AT-REST KEYS FOR ONE FILE
/// Derives the at-rest grid key, nonce and MAC key of one file.
///
/// # Parameters
/// - `salt`: What makes the keys unique to the file (its content hash, plus any scope).
/// - `ikm`: The master key of the store the file belongs to.
#[cfg(feature = "chat")]
fn disk_keys(salt: &[u8], ikm: &[u8]) -> DiskKeys
{
    let hkdf = Hkdf::<Sha256>::new(Some(salt), ikm);

    const KEY_LEN: usize = consts::DEFAULT_GRID_WIDTH * consts::DEFAULT_GRID_HEIGHT * 2;
    const NONCE_LEN: usize = consts::DEFAULT_GRID_WIDTH * consts::DEFAULT_GRID_HEIGHT;

    //GRID KEY, AT THE FULL KEYDIM
    let mut key_bytes = Zeroizing::new(vec![0u8; KEY_LEN * 8]);
    hkdf.expand(b"WHY2-IMAGE-KEY", &mut key_bytes).expect("HKDF expand failed");

    //NONCE, EXPANDED SEPARATELY
    let mut nonce_bytes = Zeroizing::new(vec![0u8; NONCE_LEN * 8]);
    hkdf.expand(b"WHY2-IMAGE-NONCE", &mut nonce_bytes).expect("HKDF expand failed");

    let mut mac_bytes = Zeroizing::new([0u8; 32]);
    hkdf.expand(b"WHY2-DISK-MAC", mac_bytes.as_mut()).expect("HKDF expand failed");

    let to_i64 = |bytes: &[u8]| bytes.chunks_exact(8)
        .map(|c| i64::from_be_bytes(c.try_into().unwrap()))
        .collect::<Vec<i64>>();

    (Zeroizing::new(to_i64(&key_bytes)), to_i64(&nonce_bytes), mac_bytes)
}

/// Returns the at-rest keys of one stored server picture or voice message.
///
/// # Parameters
/// - `hash`: The SHA-256 the file is named after.
#[cfg(feature = "server")]
pub fn image_keys(hash: &[u8; 32]) -> DiskKeys //AT-REST KEYS FOR ONE IMAGE
{
    disk_keys(hash, kex::image_key().as_ref())
}

/// Returns the at-rest keys of one cached picture.
///
/// Salting with the server fingerprint as well as the hash means two servers' copies of the
/// same picture share no keystream, and a file moved between scopes fails to verify.
///
/// # Parameters
/// - `fingerprint`: The fingerprint of the server the picture came from.
/// - `hash`: The SHA-256 the file is named after.
#[cfg(feature = "client_base")]
pub fn cache_keys(fingerprint: &str, hash: &[u8; 32]) -> DiskKeys
{
    let mut salt = Vec::with_capacity(fingerprint.len() + hash.len());

    salt.extend_from_slice(fingerprint.as_bytes());
    salt.extend_from_slice(hash);

    disk_keys(&salt, kex::cache_key().as_ref())
}

/// Opens an HMAC over a file's ciphertext, to be fed as the file is written.
#[cfg(feature = "chat")]
pub fn disk_mac(mac_key: &[u8; 32]) -> Hmac<Sha256> //OPEN A MAC OVER A FILE'S CIPHERTEXT
{
    <Hmac<Sha256>>::new_from_slice(mac_key).expect("Invalid MAC key")
}

/// Binds the ciphertext length into a file MAC and finishes it.
///
/// The length goes after the ciphertext, since the ciphertext is fed as it is written:
///
/// $$ T = \mathrm{HMAC}\_{K\_{f,m}}\left(C \Vert \mathrm{be64}(\ell)\right) $$
///
/// # Returns
/// The 32-byte tag appended to the file.
#[cfg(feature = "chat")]
pub fn disk_tag(mut mac: Hmac<Sha256>, length: u64) -> [u8; 32] //BIND THE LENGTH AND FINISH IT
{
    mac.update(&length.to_be_bytes());

    mac.finalize().into_bytes().into()
}

/// Computes the trailing tag of a whole ciphertext in one call.
#[cfg(feature = "chat")]
pub fn disk_seal(mac_key: &[u8; 32], ciphertext: &[u8]) -> [u8; 32]
{
    let mut mac = disk_mac(mac_key);
    mac.update(ciphertext);

    disk_tag(mac, ciphertext.len() as u64)
}

/// Verifies a sealed file's trailing tag.
///
/// # Parameters
/// - `mac_key`: The file's MAC key.
/// - `sealed`: The file, `ciphertext || tag`.
///
/// # Returns
/// - `Some(ciphertext)` if the tag verifies.
/// - `None` if the file is too short or has been tampered with.
#[cfg(feature = "chat")]
pub fn disk_open<'a>(mac_key: &[u8; 32], sealed: &'a [u8]) -> Option<&'a [u8]>
{
    let (ciphertext, tag) = sealed.split_at_checked(
        sealed.len().checked_sub(consts_chat::DISK_TAG_SIZE)?)?;

    let mut mac = disk_mac(mac_key);

    mac.update(ciphertext);
    mac.update(&(ciphertext.len() as u64).to_be_bytes());

    mac.verify_slice(tag).ok()?;

    Some(ciphertext)
}

/// Applies the current at-rest stream cipher to a whole file in fixed-size chunks.
///
/// The server's picture store was written one upload packet at a time, and its stream was
/// finalized after every chunk, which advances the block counter by an extra block for the
/// partial tail. Decryption must therefore break the file exactly where the upload did. The
/// client cache was written in one call, so it passes the whole file as a single chunk. CTR is
/// symmetric, so this both encrypts and decrypts.
///
/// # Parameters
/// - `key`: The file's grid key.
/// - `nonce`: The file's initial counter grid.
/// - `data`: The plaintext or ciphertext.
/// - `chunk_size`: The size of one write, in bytes.
#[cfg(feature = "chat")]
pub(crate) fn apply_file_cipher(key: &[i64], nonce: &[i64], data: &[u8], chunk_size: usize) -> Option<Vec<u8>>
{
    let mut stream: RexStream = RexStream::new(&Grid::from_key(key).ok()?, Grid::from_flat(nonce).ok()?).ok()?;
    let mut output = Vec::with_capacity(data.len());

    for chunk in data.chunks(chunk_size.max(1))
    {
        let mut transformed = stream.update(&bytes_to_i64(chunk)).ok()?;
        transformed.extend(stream.finalize().ok()?);

        let mut bytes = i64_to_bytes(&transformed);
        bytes.truncate(chunk.len());

        output.extend_from_slice(&bytes);
    }

    Some(output)
}

/// Applies the frozen previous-release at-rest stream cipher to a whole file.
///
/// This mirrors [`apply_file_cipher`] chunk for chunk, but with the previous release's round
/// shifts. It exists only for the one-time migration of that release's files.
#[cfg(feature = "chat")]
pub(crate) fn apply_file_cipher_legacy(key: &[i64], nonce: &[i64], data: &[u8], chunk_size: usize) -> Option<Vec<u8>>
{
    let key: Grid = Grid::from_key(key).ok()?;
    let nonce: Grid = Grid::from_flat(nonce).ok()?;
    let round_keys = why2::crypto::generate_round_keys(&key).ok()?;
    let grid_area = consts::DEFAULT_GRID_WIDTH * consts::DEFAULT_GRID_HEIGHT;

    let mut output = Vec::with_capacity(data.len());
    let mut counter = 0u64;

    for chunk in data.chunks(chunk_size.max(1))
    {
        //ONE GRID PER FULL BLOCK AND ONE FOR THE PARTIAL TAIL, AS update + finalize DID
        let mut grids: Vec<Grid> = bytes_to_i64(chunk).chunks(grid_area)
            .map(Grid::from_flat).collect::<Result<Vec<_>, _>>().ok()?;

        legacy::apply_ctr(&mut grids, &nonce, &round_keys, counter);
        counter = counter.wrapping_add(grids.len() as u64);

        let mut bytes = i64_to_bytes(&grids.iter().flat_map(Grid::to_flat).collect::<Vec<_>>());
        bytes.truncate(chunk.len());

        output.extend_from_slice(&bytes);
    }

    Some(output)
}

/// Returns the at-rest keys of the message history.
#[cfg(feature = "server")]
pub fn history_keys() -> SharedKeys //AT-REST KEYS FOR THE MESSAGE HISTORY
{
    let hkdf = Hkdf::<Sha256>::new(None, kex::history_key().as_ref());

    //GRID KEY, AT THE FULL KEYDIM
    const KEY_LEN: usize = consts::DEFAULT_GRID_WIDTH * consts::DEFAULT_GRID_HEIGHT * 2;

    let mut key_bytes = Zeroizing::new(vec![0u8; KEY_LEN * 8]);
    hkdf.expand(b"WHY2-HISTORY-KEY", &mut key_bytes).expect("HKDF expand failed");

    let key = key_bytes.chunks_exact(8).map(|c| i64::from_be_bytes(c.try_into().unwrap())).collect();

    //MAC KEY, EXPANDED SEPARATELY
    let mut mac_key = Zeroizing::new(vec![0u8; 32]);
    hkdf.expand(b"WHY2-HISTORY-MAC", &mut mac_key).expect("HKDF expand failed");

    (Zeroizing::new(key), mac_key)
}
