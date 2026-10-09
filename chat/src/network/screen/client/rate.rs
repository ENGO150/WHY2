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
        AtomicBool,
        AtomicU32,
        AtomicU64,
        Ordering,
    },
};

use tokio::net::TcpStream;

use crate::network::screen::
{
    self,
    consts,
};

//SHARED WITH THE NETWORK TASK
static SENT: AtomicU64 = AtomicU64::new(0);         //BYTES WRITTEN THIS WINDOW
static BUSY: AtomicU64 = AtomicU64::new(0);         //MICROSECONDS SPENT WRITING
static BITRATE: AtomicU32 = AtomicU32::new(0);      //CURRENT TARGET
static DELAY: AtomicU64 = AtomicU64::new(u64::MAX); //LEAST QUEUEING SEEN, MICROSECONDS
static REMOTE: AtomicU64 = AtomicU64::new(u64::MAX); //LEAST QUEUEING THE SERVER REPORTED, MICROSECONDS
static SHED: AtomicBool = AtomicBool::new(false);    //THE SERVER DROPPED A VIEWER BEHIND

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
        let delay = Duration::from_micros(take_min(&DELAY).max(take_min(&REMOTE)));
        let shed = SHED.swap(false, Ordering::Relaxed);

        let current = f64::from(self.bitrate());
        let throughput = sent / elapsed.as_secs_f64();

        let carried = throughput.min(current);

        //LINK FULL, A QUEUE BUILDING, OR A VIEWER LEFT BEHIND
        let next = if busy >= consts::RATE_SATURATED || delay >= consts::RATE_DELAY || shed
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
    REMOTE.store(u64::MAX, Ordering::Relaxed);
    SHED.store(false, Ordering::Relaxed);
    BITRATE.store(consts::START_BITRATE, Ordering::Relaxed);
}

pub fn record(bytes: usize, busy: Duration) //ONE SOCKET WRITE
{
    SENT.fetch_add(bytes as u64, Ordering::Relaxed);
    BUSY.fetch_add(busy.as_micros() as u64, Ordering::Relaxed);
}

pub fn probe(stream: &TcpStream) //THE QUEUE A NEW FRAME JOINS
{
    let Some((unsent, inflated)) = screen::tcp_backlog(stream) else { return };

    DELAY.fetch_min(at_target(unsent) + inflated.as_micros() as u64, Ordering::Relaxed);
}

pub fn report(delay: u32, unsent: u32, shed: bool) //THE SERVER'S WORD ON ITS VIEWERS
{
    REMOTE.fetch_min(u64::from(delay) + at_target(u64::from(unsent)), Ordering::Relaxed);

    if shed { SHED.store(true, Ordering::Relaxed); }
}

fn at_target(bytes: u64) -> u64 //MICROSECONDS TO SEND bytes AT THE CURRENT TARGET
{
    bytes * 8_000_000 / u64::from(BITRATE.load(Ordering::Relaxed).max(1))
}

fn take_min(slot: &AtomicU64) -> u64 //A WINDOW'S MINIMUM, ZERO IF NOTHING CAME
{
    match slot.swap(u64::MAX, Ordering::Relaxed)
    {
        u64::MAX => 0,
        micros => micros,
    }
}
