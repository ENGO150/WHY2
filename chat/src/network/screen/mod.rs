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
pub mod consts;

#[cfg(all(feature = "client_base"))]
pub mod client;

#[cfg(feature = "server")]
pub mod server;

use tokio::net::
{
    TcpStream,
    tcp::OwnedWriteHalf,
};

use wincode::{ SchemaRead, SchemaWrite };

use crate::
{
    consts::{ self as chat_consts, Streams },
    crypto::RexPacketStream,
    network::
    {
        self,
        EncryptionMode,
        SequencedPacket,
    },
};

//ENUMS
#[derive(SchemaWrite, SchemaRead, Clone)]
pub enum ScreenPacketCode
{
    Video { data: Vec<u8> }, //VIDEO DATA
    Audio { data: Vec<u8> }, //AUDIO DATA
}

//STRUCTS
#[derive(SchemaWrite, SchemaRead, Clone)]
pub struct ScreenPacket //SCREEN PACKET
{
    pub code: ScreenPacketCode, //CODE
    pub seq: usize,             //SEQUENTIAL NUMBER
}

//IMPLEMENTATIONS
impl SequencedPacket for ScreenPacket
{
    fn seq(&self) -> usize { self.seq }
    fn set_seq(&mut self, seq: usize) { self.seq = seq; }
}

//FUNCTIONS
//UTILS
pub fn cap_socket_buffers(stream: &TcpStream) //BOUND THE KERNEL QUEUE THIS SOCKET MAY HIDE
{
    //THE PIPELINE ALREADY KNOWS HOW TO SHED A BACKLOG - `FrameEncoder::dispatch` DROPS A FRAME THE
    //NETWORK CHANNEL CANNOT HOLD AND FORCES AN IDR SO THE NEXT ONE STANDS ALONE. IT ONLY EVER GETS
    //TO DO THAT ONCE A SEND ACTUALLY BLOCKS, THOUGH, AND WITH AUTOTUNED BUFFERS (4 MB OF SEND
    //QUEUE ON LINUX BY DEFAULT) IT NEVER DOES: THE KERNEL SWALLOWS MEGABYTES AT `H264_BITRATE`
    //BEFORE `write_all` STALLS, AND EVERY ONE OF THOSE BYTES IS LATENCY THE VIEWER PAYS - SECONDS
    //OF IT ON A LINK THAT CANNOT CARRY THE SHARE. CAPPING THE BUFFER IS WHAT TURNS "THE LINK IS
    //FULL" BACK INTO SOMETHING THE ENCODER CAN FEEL WHILE THE BACKLOG IS STILL ONE FRAME OLD.
    //
    //`SOCKET_BUFFER` IS THE TRADE: THE STANDING QUEUE COSTS ROUGHLY ONE BUFFER PER HOP AT THE
    //SHARE'S BITRATE (~250 ms EACH AT 4 Mbps), WHILE A RECEIVE BUFFER ALSO PINS THE WINDOW, SO
    //SIZING IT MUCH SMALLER WOULD CAP THROUGHPUT ON A HIGH-LATENCY PATH INSTEAD.
    let socket = socket2::SockRef::from(stream);

    //BEST EFFORT - A PLATFORM THAT REFUSES EITHER ONE IS BUFFERBLOATED, NOT BROKEN
    socket.set_send_buffer_size(consts::SOCKET_BUFFER).ok();
    socket.set_recv_buffer_size(consts::SOCKET_BUFFER).ok();
}

pub async fn send_frame //SEND frame TO stream
(
    write_stream: &mut OwnedWriteHalf,
    code: ScreenPacketCode,
    rex_stream: &mut RexPacketStream,
    seq: Option<&mut usize>, //LOCAL/GLOBAL SEQ COUNTER
)
{
    network::send_tcp
    (
        write_stream,
        ScreenPacket
        {
            code,
            seq: 0,
        },
        EncryptionMode::Stream(rex_stream),
        seq,
    ).await;
}

pub async fn receive_frame
(
    streams: &mut Streams<'_>,
    rex_stream: &mut RexPacketStream,
    seq: &mut usize //LOCAL/GLOBAL SEQ
) -> Option<ScreenPacketCode>
{
    let read = network::read_tcp
    (
        streams,
        EncryptionMode::Stream(rex_stream),
        #[cfg(feature = "server")] true,
    ).await?;

    //DESERIALIZE AND RETURN
    match wincode::config::deserialize::<ScreenPacket, _>(&read.data, chat_consts::PACKET_CONFIG)
    {
        Ok(packet) =>
        {
            //VERIFY SEQUENCE NUMBER (CLIENT)
            if packet.seq > *seq || *seq == 0 //VALID
            {
                *seq = packet.seq;
            } else { return None; }

            Some(packet.code)
        },

        _ => { None } //TODO: Implement
    }
}
