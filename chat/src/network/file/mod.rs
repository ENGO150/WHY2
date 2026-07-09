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

use crate::
{
    network::
    {
        self,
        EncryptionMode,
        SequencedPacket,
    },
    consts::
    {
        self,
        SharedKeys,
        Streams,
    },
};

#[cfg(feature = "server")]
use crate::network::server as chat_server;

//STRUCTS
#[derive(SchemaWrite, SchemaRead, Clone, Default)]
pub struct FilePacket //FILE CHUNK
{
    pub uid: u64,                 //UPLOAD UID
    pub data: Option<Vec<u8>>,    //BINARY DATA
    pub size: Option<u64>,        //FILE SIZE
    pub filename: Option<String>, //FILE NAME
    pub hash: Option<[u8; 32]>,   //FILE HASH
    pub nonce: Option<Vec<i64>>,  //STREAM ENCRYPTION NONCE
    pub seq: usize,               //SEQUENCE NUMBER
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
    keys: Option<&SharedKeys>,
    mut seq: Option<&mut usize>,
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
                //SEND FILE CHUNK
                network::send_tcp(&mut stream, FilePacket
                {
                    uid,
                    data: Some(buffer[..bytes].to_vec()),
                    ..Default::default()
                }, EncryptionMode::OneShot(keys), seq.as_deref_mut());
            },
            Err(_) => {}, //TODO: Implement
        }
    }
}

pub fn receive_file
(
    streams: &mut Streams,
    keys: Option<&SharedKeys>,
    seq: &mut usize
) -> Option<FilePacket>
{
    let read = network::read_tcp
    (
        streams,
        EncryptionMode::OneShot(keys),
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

            Some(packet)
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
