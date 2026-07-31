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

//MODULES
#[cfg(feature = "client_base")]
pub mod client;

#[cfg(feature = "server")]
pub mod server;

use std::
{
    fs::File,
    io::Read,
    path::PathBuf,
    net::TcpStream,
};

use wincode::{ SchemaRead, SchemaWrite };

use why2::stream::RexStream;

use crate::
{
    consts::{ self, Streams },
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
#[derive(SchemaWrite, SchemaRead, Clone)]
pub enum FilePacketCode
{
    //DATA
    Data { data: Vec<u8> },

    //FILE METADATA
    Metadata
    {
        size: u64,        //FILE SIZE
        filename: String, //FILENAME
        hash: [u8; 32],   //FILE HASH
    },
}

//STRUCTS
#[derive(SchemaWrite, SchemaRead, Clone)]
pub struct FilePacket //FILE CHUNK
{
    pub uid: u64,             //UPLOAD UID
    pub code: FilePacketCode, //CODE
    pub seq: usize,           //SEQUENCE NUMBER
}

//IMPLEMENTATIONS
impl SequencedPacket for FilePacket
{
    fn seq(&self) -> usize { self.seq }
    fn set_seq(&mut self, seq: usize) { self.seq = seq; }
}

//FUNCTIONS
pub fn send_file //CHUNK FILE AND SEND TO STREAM
(
    path: PathBuf,
    mut stream: TcpStream,
    uid: u64,
    rex_stream: &mut RexStream,
    mut seq: Option<&mut usize>,
    #[cfg(feature = "server")] disk_stream: &mut RexStream,
)
{
    let mut file = File::open(path).expect("Cannot open file for upload");
    let mut buffer = vec![0; consts::UPLOAD_CHUNK_SIZE];

    //LOOP READING
    loop
    {
        match file.read(&mut buffer)
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
                network::send_tcp(&mut stream, FilePacket
                {
                    uid,
                    code: FilePacketCode::Data { data: plaintext },
                    seq: 0,
                }, EncryptionMode::Stream(rex_stream), seq.as_deref_mut());
            },
            Err(_) => {}, //TODO: Implement
        }
    }
}

pub fn receive_file
(
    streams: &mut Streams,
    rex_stream: &mut RexStream,
    seq: &mut usize
) -> Option<(u64, FilePacketCode)>
{
    let read = network::read_tcp
    (
        streams,
        EncryptionMode::Stream(rex_stream),
        #[cfg(feature = "server")] true,
    )?;

    //DESERIALIZE AND RETURN
    match wincode::deserialize::<FilePacket>(&read.data)
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
            chat_server::remove_connection(&read.peer_addr, false, Some("packet"));

            None
        }
    }
}
