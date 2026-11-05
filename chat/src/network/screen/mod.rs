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

use std::
{
    io,
    time::Duration,
};

#[cfg(target_os = "linux")]
use std::
{
    mem,
    os::fd::AsRawFd,
};

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
    Feedback { delay: u32, unsent: u32, shed: bool }, //VIEWER BACKLOG (MICROSECONDS; BYTES, OR THE VIEWER'S QUEUE DEPTH)
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

//STRUCTS
/// What one TCP socket reports about its queue and its delivery rate.
#[derive(Clone, Copy, Default)]
pub struct TcpStats //ONE SOCKET'S QUEUE AND DELIVERY FIGURES
{
    /// Bytes written to the socket that have not been sent yet.
    pub unsent: u64,          //BYTES NOT SENT YET
    /// Round-trip time above the path's best.
    pub inflated: Duration,   //RTT OVER THE PATH'S MINIMUM
    /// Bytes the peer has acknowledged, cumulative.
    pub acked: u64,           //BYTES ACKNOWLEDGED
    /// The kernel's delivery-rate estimate, in bytes per second.
    pub delivery_rate: u64,   //BYTES PER SECOND
    /// Retransmitted segments, cumulative.
    pub retrans: u32,         //RETRANSMITTED SEGMENTS
}

//FUNCTIONS
//UTILS
/// Returns a socket's queue and delivery figures.
///
/// # Returns
/// `None` where the platform has no `tcp_info` or the kernel reports too little of it.
#[cfg(target_os = "linux")]
pub fn tcp_stats(stream: &TcpStream) -> Option<TcpStats>
{
    //SAFETY: tcp_info IS PLAIN DATA
    let mut info: libc::tcp_info = unsafe { mem::zeroed() };
    let mut length = mem::size_of::<libc::tcp_info>() as libc::socklen_t;

    //SAFETY: LIVE SOCKET, info IS length BYTES
    let status = unsafe
    {
        libc::getsockopt(stream.as_raw_fd(), libc::IPPROTO_TCP, libc::TCP_INFO, (&raw mut info).cast(), &mut length)
    };

    //THE KERNEL MAY KNOW FEWER FIELDS THAN libc DOES
    let known = length as usize;

    if status != 0 || known < mem::offset_of!(libc::tcp_info, tcpi_notsent_bytes) + mem::size_of::<u32>() { return None; }

    let acked = if known >= mem::offset_of!(libc::tcp_info, tcpi_bytes_acked) + mem::size_of::<u64>() { info.tcpi_bytes_acked } else { 0 };
    let delivery_rate = if known >= mem::offset_of!(libc::tcp_info, tcpi_delivery_rate) + mem::size_of::<u64>() { info.tcpi_delivery_rate } else { 0 };

    Some(TcpStats
    {
        unsent: u64::from(info.tcpi_notsent_bytes),
        inflated: Duration::from_micros(u64::from(info.tcpi_rtt.saturating_sub(info.tcpi_min_rtt))),
        acked,
        delivery_rate,
        retrans: info.tcpi_total_retrans,
    })
}

#[cfg(not(target_os = "linux"))]
pub fn tcp_stats(_stream: &TcpStream) -> Option<TcpStats>
{
    None
}

/// Returns the unsent bytes and RTT inflation of a socket.
pub fn tcp_backlog(stream: &TcpStream) -> Option<(u64, Duration)> //UNSENT BYTES AND RTT OVER THE PATH'S BEST
{
    tcp_stats(stream).map(|stats| (stats.unsent, stats.inflated))
}

/// Bounds the kernel queue a screen socket may hide.
///
/// The budget is [`SOCKET_QUEUE_MS`](consts::SOCKET_QUEUE_MS) of `bitrate`, clamped to
/// [`SOCKET_BUFFER_MIN`](consts::SOCKET_BUFFER_MIN)..[`SOCKET_BUFFER_MAX`](consts::SOCKET_BUFFER_MAX).
/// Linux doubles what is requested, so half is asked for there.
pub fn cap_socket_buffers(stream: &TcpStream, bitrate: u32)
{
    let budget = (u64::from(bitrate) / 8 * consts::SOCKET_QUEUE_MS / 1000) as usize;
    let budget = budget.clamp(consts::SOCKET_BUFFER_MIN, consts::SOCKET_BUFFER_MAX);

    //LINUX DOUBLES BOTH BUFFERS
    #[cfg(target_os = "linux")]
    let request = (budget / 2).max(consts::SOCKET_BUFFER_MIN / 2);

    #[cfg(not(target_os = "linux"))]
    let request = budget;

    //CAP THE AUTOTUNED BUFFERS THAT HIDE BACKLOG
    let socket = socket2::SockRef::from(stream);

    //BEST EFFORT
    socket.set_send_buffer_size(request).ok();
    socket.set_recv_buffer_size(request).ok();

    //KEEP THE UNSENT QUEUE SHORT WHERE THE KERNEL OFFERS IT
    #[cfg(target_os = "linux")]
    {
        let value = request.min(i32::MAX as usize) as libc::c_int;

        //SAFETY: LIVE SOCKET, value IS ONE INCH
        unsafe
        {
            libc::setsockopt(stream.as_raw_fd(), libc::IPPROTO_TCP, libc::TCP_NOTSENT_LOWAT,
                (&raw const value).cast(), mem::size_of::<libc::c_int>() as libc::socklen_t);
        }
    }
}

/// Returns whether an access unit can be decoded on its own: it contains an IDR slice (NAL
/// type 5) or an SPS (type 7), which the encoders only emit in front of one.
pub fn is_keyframe(bitstream: &[u8]) -> bool //DOES THIS ACCESS UNIT STAND ON ITS OWN?
{
    //ONLY AN IDR (5), OR AN SPS (7) IN FRONT OF ONE
    let mut index = 0;
    while index + 3 < bitstream.len()
    {
        if bitstream[index..index + 3] != [0, 0, 1] { index += 1; continue; }

        if matches!(bitstream[index + 3] & 0x1f, 5 | 7) { return true; }

        index += 3;
    }

    false
}

/// Returns whether `WHY2_SCREEN_STATS` asked for the once-a-second diagnostic lines.
pub fn stats_enabled() -> bool
{
    static ENABLED: std::sync::LazyLock<bool> = std::sync::LazyLock::new(||
    {
        std::env::var(consts::STATS_VAR).is_ok_and(|value| value != "0")
    });

    *ENABLED
}

pub async fn send_frame //SEND frame TO stream
(
    write_stream: &mut OwnedWriteHalf,
    code: ScreenPacketCode,
    rex_stream: &mut RexPacketStream,
    seq: Option<&mut usize>, //LOCAL/GLOBAL SEQ COUNTER
) -> io::Result<()>
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
    ).await
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
