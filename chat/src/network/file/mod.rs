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

//! # File Transfer
//!
//! Uploads and downloads on auxiliary TCP sockets, shared by fileshares, chat pictures,
//! avatars, the server icon and voice messages.
//!
//! # Protocol
//! A transfer is approved over the main connection, which mints a one-time token
//! ([`PacketCode::Upload`](crate::network::codes::PacketCode::Upload) and friends). The client
//! then opens a new socket, presents the token, and the file flows as a sequence of
//! [`FilePacket`]s — one [`Metadata`](FilePacketCode::Metadata) and then
//! [`UPLOAD_CHUNK_SIZE`](consts::UPLOAD_CHUNK_SIZE) chunks of [`Data`](FilePacketCode::Data) —
//! protected by a [`RexPacketStream`] keyed by the session keys and the token.
//!
//! The auxiliary socket has no packet-rate limit: a packet there is payload, not a request, and
//! the costs are bounded by size limits and by the main-connection request that minted the
//! token.
//!
//! # At Rest
//! On the server, a persistent upload (see [`UploadKind::persistent`]) is sealed into
//! `server_images/` and read back in the same chunks it was written in: the at-rest
//! `RexStream` is finalized per chunk, so a reader that breaks the file anywhere else falls off
//! the keystream.

//MODULES
#[cfg(feature = "client_base")]
pub mod client;

#[cfg(feature = "server")]
pub mod server;

use std::path::PathBuf;

use tokio::
{
    fs::File,
    io::AsyncReadExt,
    net::tcp::OwnedWriteHalf,
};

use wincode::{ SchemaRead, SchemaWrite };

#[cfg(feature = "server")]
use why2::stream::RexStream;

use crate::
{
    consts::{ self, Streams },
    crypto::RexPacketStream,
    network::
    {
        self,
        EncryptionMode,
        SequencedPacket,
    },
};

#[cfg(feature = "server")]
use crate::
{
    crypto,
    network::server as chat_server,
};

//ENUMS
//WHAT AN UPLOAD IS FOR - A FILESHARE, A CHAT PICTURE, A PROFILE PICTURE, THE SERVER'S OR A CLIP
/// What an upload is for.
#[derive(Clone, Copy, PartialEq)]
pub enum UploadKind
{
    /// A fileshare, kept in a temp directory for the session.
    File,
    /// A chat picture.
    Image,
    /// A profile picture.
    Avatar,
    /// The server icon.
    Icon,
    /// A voice message.
    Voice,
}

/// The payload of one file packet.
#[derive(SchemaWrite, SchemaRead, Clone)]
pub enum FilePacketCode
{
    //DATA
    /// One chunk of the file.
    Data { data: Vec<u8> },

    //FILE METADATA
    /// The file's description, sent before its data.
    Metadata
    {
        /// Size in bytes.
        size: u64,        //FILE SIZE
        /// The file's name.
        filename: String, //FILENAME
        /// SHA-256 of the whole file.
        hash: [u8; 32],   //FILE HASH
    },
}

//STRUCTS
/// One packet on a file transfer socket.
#[derive(SchemaWrite, SchemaRead, Clone)]
pub struct FilePacket //FILE CHUNK
{
    /// Id of the upload this packet belongs to.
    pub uid: u64,             //UPLOAD UID
    /// The payload.
    pub code: FilePacketCode, //CODE
    /// Sequence number, strictly increasing.
    pub seq: usize,           //SEQUENCE NUMBER
}

//IMPLEMENTATIONS
impl UploadKind
{
    //A PICTURE IS NAMED AFTER ITS CONTENT AND SEALED UNDER ITS HASH
    /// Returns whether the upload is persistent: named after its content, sealed under its hash and
    /// kept in `server_images/`. Only a fileshare is not.
    pub fn persistent(&self) -> bool { !matches!(self, Self::File) }

    /// Returns the kind's name, for logging.
    pub fn name(&self) -> &'static str
    {
        match self
        {
            Self::File => "file",
            Self::Image => "image",
            Self::Avatar => "avatar",
            Self::Icon => "icon",
            Self::Voice => "voice",
        }
    }
}

impl SequencedPacket for FilePacket
{
    fn seq(&self) -> usize { self.seq }
    fn set_seq(&mut self, seq: usize) { self.seq = seq; }
}

//FUNCTIONS
/// Sends a file as a stream of [`Data`](FilePacketCode::Data) packets.
///
/// On the server the file on disk is sealed, so each chunk is decrypted with `disk_stream`
/// before it is sent; the chunking matches how it was written.
///
/// # Parameters
/// - `path`: The file to send.
/// - `write_stream`: The transfer socket's write half; dropped (closing it) when done.
/// - `uid`: The upload id stamped on every packet.
/// - `rex_stream`: The transfer's packet stream.
/// - `seq`: A local sequence counter.
/// - `disk_stream`: The file's at-rest stream (server only).
/// - `progress`: Called with the total bytes sent after each chunk.
///
/// # Panics
/// If the file cannot be opened, or the at-rest stream fails.
pub async fn send_file //CHUNK FILE AND SEND TO STREAM
(
    path: PathBuf,
    mut write_stream: OwnedWriteHalf,
    uid: u64,
    rex_stream: &mut RexPacketStream,
    mut seq: Option<&mut usize>,
    #[cfg(feature = "server")] disk_stream: &mut RexStream,
    mut progress: impl FnMut(u64), //BYTES SENT SO FAR
)
{
    let mut file = File::open(path).await.expect("Cannot open file for upload");
    let mut buffer = vec![0; consts::UPLOAD_CHUNK_SIZE];
    let mut sent = 0u64;

    //LOOP READING
    loop
    {
        match file.read(&mut buffer).await
        {
            Ok(0) => break, //EOF
            Ok(bytes) =>
            {
                let plaintext =
                {
                    //DECRYPT FILE
                    #[cfg(feature = "server")]
                    {
                        let input_i64 = crypto::bytes_to_i64(&buffer[..bytes]);
                        let mut decrypted_i64 = disk_stream.update(&input_i64).expect("Disk stream decryption failed");
                        decrypted_i64.extend(disk_stream.finalize().expect("Disk stream finalize failed"));
                        let mut out = crypto::i64_to_bytes(&decrypted_i64);
                        out.truncate(bytes);
                        out
                    }

                    #[cfg(feature = "client_base")]
                    {
                        buffer[..bytes].to_vec()
                    }
                };

                //SEND FILE CHUNK
                let _ = network::send_tcp(&mut write_stream, FilePacket
                {
                    uid,
                    code: FilePacketCode::Data { data: plaintext },
                    seq: 0,
                }, EncryptionMode::Stream(rex_stream), seq.as_deref_mut()).await;

                //REPORT PROGRESS
                sent += bytes as u64;
                progress(sent);
            },
            Err(_) => {}, //TODO: Implement
        }
    }
}

/// Receives one file packet.
///
/// # Parameters
/// - `streams`: The transfer socket's halves.
/// - `rex_stream`: The transfer's packet stream.
/// - `seq`: The transfer's sequence counter.
///
/// # Returns
/// - `Some((uid, code))` for a valid packet.
/// - `None` if the read failed, the packet did not verify or decode, or its sequence number did
///   not increase. On the server an undecodable packet removes the connection.
pub async fn receive_file
(
    streams: &mut Streams<'_>,
    rex_stream: &mut RexPacketStream,
    seq: &mut usize
) -> Option<(u64, FilePacketCode)>
{
    let read = network::read_tcp
    (
        streams,
        EncryptionMode::Stream(rex_stream),
        #[cfg(feature = "server")] true,
    ).await?;

    //DESERIALIZE AND RETURN
    match wincode::config::deserialize::<FilePacket, _>(&read.data, consts::PACKET_CONFIG)
    {
        Ok(packet) =>
        {
            //VERIFY SEQUENCE NUMBER (CLIENT)
            if packet.seq > *seq || *seq == 0 //VALID
            {
                *seq = packet.seq;
            } else { return None; }

            Some((packet.uid, packet.code))
        },
        Err(_) =>
        {
            //FORCEFULLY DISCONNECT CLIENT ON INVALID PACKET
            #[cfg(feature = "server")]
            chat_server::remove_connection(&read.peer_addr, false, Some("packet")).await;

            None
        }
    }
}
