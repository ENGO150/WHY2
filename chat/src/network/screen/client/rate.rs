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
    time::{ Duration, Instant },
    sync::atomic::
    {
        AtomicU32,
        AtomicU64,
        Ordering,
    },
};

#[cfg(target_os = "linux")]
use std::
{
    mem,
    os::fd::AsRawFd,
};

#[cfg(target_os = "linux")]
use tokio::net::TcpStream;

use crate::network::screen::consts;

//SHARED WITH THE NETWORK TASK
static SENT: AtomicU64 = AtomicU64::new(0);         //BYTES WRITTEN THIS WINDOW
static BUSY: AtomicU64 = AtomicU64::new(0);         //MICROSECONDS SPENT WRITING
static BITRATE: AtomicU32 = AtomicU32::new(0);      //CURRENT TARGET
static DELAY: AtomicU64 = AtomicU64::new(u64::MAX); //LEAST QUEUEING SEEN, MICROSECONDS

//STRUCTS
pub struct RateControl //FITS THE BITRATE TO THE LINK
{
    window: Instant,
    held: Instant,    //NO GROWTH BEFORE THIS
    ceiling: f64,     //WHAT THE LINK CARRIED WHEN IT LAST FILLED
    congested: bool,  //INSIDE A BACKOFF
}

impl RateControl
{
    pub fn new() -> Self
    {
        Self { window: Instant::now(), held: Instant::now(), ceiling: f64::INFINITY, congested: false }
    }

    pub fn bitrate(&self) -> u32
    {
        BITRATE.load(Ordering::Relaxed)
    }

    pub fn update(&mut self) -> Option<u32> //NEW TARGET, IF IT MOVED
    {
        let elapsed = self.window.elapsed();
        if elapsed < consts::RATE_WINDOW { return None; }

        self.window = Instant::now();

        let sent = SENT.swap(0, Ordering::Relaxed) as f64 * 8.0;
        let busy = Duration::from_micros(BUSY.swap(0, Ordering::Relaxed)).as_secs_f64() / elapsed.as_secs_f64();
        let delay = match DELAY.swap(u64::MAX, Ordering::Relaxed) { u64::MAX => Duration::ZERO, micros => Duration::from_micros(micros) };

        let current = f64::from(self.bitrate());
        let throughput = sent / elapsed.as_secs_f64();

        let carried = throughput.min(current);

        //LINK FULL, OR A QUEUE BUILDING
        let next = if busy >= consts::RATE_SATURATED || delay >= consts::RATE_DELAY
        {
            //AT MOST WHAT WE ASKED FOR, AT MOST WHAT A FULL LINK DRAINED
            if !self.congested { self.ceiling = current; }
            if busy >= consts::RATE_SATURATED { self.ceiling = self.ceiling.min(throughput); }

            self.congested = true;
            self.held = Instant::now() + consts::RATE_HOLD;

            carried * consts::RATE_BACKOFF
        } else if busy <= consts::RATE_IDLE && delay < consts::RATE_DELAY / 2 && Instant::now() >= self.held && throughput >= current * consts::RATE_USED
        {
            self.congested = false;

            //BACK UP UNDER THE CEILING, THEN SLOWLY PAST IT
            if current < self.ceiling * consts::RATE_NEAR
            {
                let resume = if self.ceiling.is_finite() { self.ceiling * consts::RATE_BACKOFF } else { 0.0 };
                (current * consts::RATE_GROWTH).max(resume)
            } else
            {
                current * consts::RATE_CREEP
            }
        } else
        {
            current
        };

        let next = (next as u32).clamp(consts::MIN_BITRATE, consts::H264_BITRATE);
        if next == self.bitrate() { return None; }

        BITRATE.store(next, Ordering::Relaxed);

        Some(next)
    }
}

//FUNCTIONS
pub fn start() //A NEW SHARE
{
    SENT.store(0, Ordering::Relaxed);
    BUSY.store(0, Ordering::Relaxed);
    DELAY.store(u64::MAX, Ordering::Relaxed);
    BITRATE.store(consts::START_BITRATE, Ordering::Relaxed);
}

pub fn record(bytes: usize, busy: Duration) //ONE SOCKET WRITE
{
    SENT.fetch_add(bytes as u64, Ordering::Relaxed);
    BUSY.fetch_add(busy.as_micros() as u64, Ordering::Relaxed);
}

#[cfg(target_os = "linux")]
pub fn probe(stream: &TcpStream) //THE QUEUE A NEW FRAME JOINS
{
    //SAFETY: tcp_info IS PLAIN DATA
    let mut info: libc::tcp_info = unsafe { mem::zeroed() };
    let mut length = mem::size_of::<libc::tcp_info>() as libc::socklen_t;

    //SAFETY: LIVE SOCKET, info IS length BYTES
    let status = unsafe
    {
        libc::getsockopt(stream.as_raw_fd(), libc::IPPROTO_TCP, libc::TCP_INFO, (&raw mut info).cast(), &mut length)
    };

    if status != 0 || (length as usize) < mem::size_of::<libc::tcp_info>() { return; }

    //UNSENT BYTES AT THE CURRENT TARGET, PLUS RTT ABOVE THE PATH'S BEST
    let bitrate = u64::from(BITRATE.load(Ordering::Relaxed).max(1));
    let unsent = u64::from(info.tcpi_notsent_bytes) * 8_000_000 / bitrate;
    let inflated = u64::from(info.tcpi_rtt.saturating_sub(info.tcpi_min_rtt));

    DELAY.fetch_min(unsent + inflated, Ordering::Relaxed);
}

#[cfg(not(target_os = "linux"))]
pub fn probe(_stream: &tokio::net::TcpStream) {}
