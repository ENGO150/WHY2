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
static SENT: AtomicU64 = AtomicU64::new(0);          //BYTES WRITTEN THIS WINDOW
static ACKED: AtomicU64 = AtomicU64::new(0);         //BYTES ACKNOWLEDGED THIS WINDOW
static BUSY: AtomicU64 = AtomicU64::new(0);          //MICROSECONDS SPENT WRITING
static BITRATE: AtomicU32 = AtomicU32::new(0);       //CURRENT TARGET
static DELAY: AtomicU64 = AtomicU64::new(0);         //WORST QUEUEING SEEN, MICROSECONDS
static REMOTE: AtomicU64 = AtomicU64::new(0);        //WORST QUEUEING THE SERVER REPORTED, MICROSECONDS
static STALL: AtomicU64 = AtomicU64::new(0);         //WORST BLOCKING WRITE THIS WINDOW, MICROSECONDS
static LAST_STALL: AtomicU64 = AtomicU64::new(0);    //LAST BLOCKING WRITE, WHERE tcp_info IS MISSING
static DELIVERY: AtomicU64 = AtomicU64::new(0);      //THE KERNEL'S DELIVERY RATE, BYTES PER SECOND
static ACKED_OK: AtomicBool = AtomicBool::new(false); //tcp_info COUNTED ACKNOWLEDGED BYTES
static LAST_ACKED: AtomicU64 = AtomicU64::new(0);    //CUMULATIVE BYTES THE PEER HAD ACKNOWLEDGED
static LAST_RETRANS: AtomicU64 = AtomicU64::new(0);  //CUMULATIVE RETRANSMITTED SEGMENTS
static RETRANS: AtomicU64 = AtomicU64::new(0);       //RETRANSMITTED SEGMENTS THIS WINDOW
static SHED: AtomicBool = AtomicBool::new(false);    //THE SERVER DROPPED A FRAME THIS WINDOW
static FORCE_KEYFRAME: AtomicBool = AtomicBool::new(false); //THE NEXT FRAME MUST STAND ALONE
static DRAINING: AtomicBool = AtomicBool::new(false); //VIDEO IS HELD UNTIL THE LINK EMPTIES
static FPS: AtomicU32 = AtomicU32::new(consts::TARGET_FPS); //CAPTURE RATE; CONGESTION IS ABSORBED BY THE TARGET

//STRUCTS
pub struct RateControl //FITS THE BITRATE TO THE LINK
{
    window: Instant,
    held: Instant,    //NO GROWTH BEFORE THIS
    hot: u32,         //CONSECUTIVE WINDOWS WITH A FULL LINK
    shed: u32,        //CONSECUTIVE WINDOWS WHERE A VIEWER SHED A FRAME
    delay_ewma: f64,  //THE TREND, WHERE THE WORST SAMPLE DECIDES THE CUT
}

impl RateControl
{
    pub fn new() -> Self
    {
        Self { window: Instant::now(), held: Instant::now(), hot: 0, shed: 0, delay_ewma: 0.0 }
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

        let seconds = elapsed.as_secs_f64();
        let sent = SENT.swap(0, Ordering::Relaxed) as f64 * 8.0 / seconds;
        let acked = ACKED.swap(0, Ordering::Relaxed) as f64 * 8.0 / seconds;
        let delivery = DELIVERY.load(Ordering::Relaxed) as f64 * 8.0;
        RETRANS.swap(0, Ordering::Relaxed);
        let busy = Duration::from_micros(BUSY.swap(0, Ordering::Relaxed)).as_secs_f64() / seconds;
        let delay = Duration::from_micros(DELAY.swap(0, Ordering::Relaxed).max(REMOTE.swap(0, Ordering::Relaxed)));
        let stall = Duration::from_micros(STALL.swap(0, Ordering::Relaxed));
        let shed = SHED.swap(false, Ordering::Relaxed);
        let acked_ok = ACKED_OK.load(Ordering::Relaxed);

        //THE WORST SAMPLE DECIDES THE CUT, THE TREND DECIDES THE GROWTH
        self.delay_ewma = self.delay_ewma * 0.6 + delay.as_secs_f64() * 0.4;

        let current = f64::from(self.bitrate());

        //WHAT THE LINK DRAINED, OR WHAT WE HANDED IT WHERE tcp_info IS MISSING
        let delivered = if acked_ok { acked } else { sent };

        //A QUEUE THAT KEEPS BUILDING, A BLOCKING WRITE, OR (WITHOUT tcp_info) A WRITE-SATURATED WINDOW
        let queueing = delay >= consts::RATE_DELAY;
        let stalled = stall >= consts::RATE_STALL;
        let busy_full = !acked_ok && busy >= consts::RATE_SATURATED;

        let severe = delay >= consts::RATE_HARD_DELAY || stall >= consts::RATE_STALL_SEVERE;

        if queueing || stalled || busy_full { self.hot = self.hot.saturating_add(1); } else { self.hot = 0; }
        if shed { self.shed = self.shed.saturating_add(1); } else { self.shed = 0; }

        //ONE BAD WINDOW IS NOISE; TWO IN A ROW, A SEVERE SAMPLE, OR REPEATED SHEDDING IS CONGESTION.
        //A SHED FRAME IS ALSO A KEYFRAME REQUEST, SO ONE IS ALREADY ALLOWED BEFORE A CUT.
        let cut = severe || self.hot >= 2 || self.shed >= 2;

        let next = if cut
        {
            //NEVER KEEP MORE THAN THE TARGET, AND TRUST THE KERNEL'S DELIVERY RATE ONLY
            //ABOVE THREE QUARTERS OF IT; ONE STALE SAMPLE MUST NOT COLLAPSE THE TARGET.
            let capacity = if delivery > 0.0 { delivery.max(current * 0.75) } else { current };
            let keep = if severe { consts::RATE_CUT_SEVERE } else { consts::RATE_BACKOFF };
            let cut_rate = if severe { consts::RATE_CUT_SEVERE } else { consts::RATE_CUT };

            (current * cut_rate).min(capacity * keep)
        } else
        {
            let now = Instant::now();

            //THE LINK DRAINED WHAT WE ASKED OF IT, SO PROBE A LITTLE HIGHER
            let healthy = self.hot == 0 && self.shed == 0 && stall < consts::RATE_STALL &&
                self.delay_ewma < consts::RATE_DELAY.as_secs_f64() / 2.0 && now >= self.held;

            if healthy && delivered >= current * consts::RATE_USED
            {
                if current < f64::from(consts::H264_BITRATE) * consts::RATE_NEAR
                {
                    current * consts::RATE_GROWTH
                } else
                {
                    current * consts::RATE_CREEP
                }
            } else
            {
                current
            }
        };

        let next = (next as u32).clamp(consts::MIN_BITRATE, consts::H264_BITRATE);

        if cut
        {
            self.held = Instant::now() + consts::RATE_HOLD;
            self.hot = 0;
            self.shed = 0;
        }

        if next == self.bitrate() { return None; }

        BITRATE.store(next, Ordering::Relaxed);

        Some(next)
    }
}

//FUNCTIONS
pub fn start() //A NEW SHARE
{
    SENT.store(0, Ordering::Relaxed);
    ACKED.store(0, Ordering::Relaxed);
    BUSY.store(0, Ordering::Relaxed);
    DELAY.store(0, Ordering::Relaxed);
    REMOTE.store(0, Ordering::Relaxed);
    STALL.store(0, Ordering::Relaxed);
    LAST_STALL.store(0, Ordering::Relaxed);
    DELIVERY.store(0, Ordering::Relaxed);
    ACKED_OK.store(false, Ordering::Relaxed);
    LAST_ACKED.store(0, Ordering::Relaxed);
    LAST_RETRANS.store(0, Ordering::Relaxed);
    RETRANS.store(0, Ordering::Relaxed);
    SHED.store(false, Ordering::Relaxed);
    FORCE_KEYFRAME.store(false, Ordering::Relaxed);
    DRAINING.store(false, Ordering::Relaxed);
    FPS.store(consts::TARGET_FPS, Ordering::Relaxed);
    BITRATE.store(consts::START_BITRATE, Ordering::Relaxed);
}

pub fn record(bytes: usize, busy: Duration) //ONE SOCKET WRITE
{
    SENT.fetch_add(bytes as u64, Ordering::Relaxed);
    BUSY.fetch_add(busy.as_micros() as u64, Ordering::Relaxed);

    //ONE BLOCKING WRITE IS ENOUGH
    if busy >= consts::RATE_STALL
    {
        STALL.fetch_max(busy.as_micros() as u64, Ordering::Relaxed);
        LAST_STALL.store(busy.as_micros() as u64, Ordering::Relaxed);
    }
}

/// Takes the last blocking write, for platforms without `tcp_info`.
pub fn take_stall() -> Option<Duration>
{
    match LAST_STALL.swap(0, Ordering::Relaxed)
    {
        0 => None,
        micros => Some(Duration::from_micros(micros)),
    }
}

/// Returns the queue a new frame would join, from `tcp_info` or the last blocking write.
pub fn queue_estimate(stream: &TcpStream) -> Option<Duration>
{
    let queued = probe(stream);

    match (queued, take_stall())
    {
        (Some(queued), Some(stall)) => Some(queued.max(stall)),
        (queued, stall) => queued.or(stall),
    }
}

pub fn probe(stream: &TcpStream) -> Option<Duration> //THE QUEUE A NEW FRAME JOINS
{
    let stats = screen::tcp_stats(stream)?;

    ACKED_OK.store(true, Ordering::Relaxed);

    //THE KERNEL'S OWN CAPACITY ESTIMATE, FOR THE NEXT WINDOW
    DELIVERY.store(stats.delivery_rate, Ordering::Relaxed);

    let delay = at_target(stats.unsent) + stats.inflated.as_micros() as u64;
    DELAY.fetch_max(delay, Ordering::Relaxed);

    let acked = stats.acked.saturating_sub(LAST_ACKED.swap(stats.acked, Ordering::Relaxed));
    ACKED.fetch_add(acked, Ordering::Relaxed);

    let retrans = u64::from(stats.retrans).saturating_sub(LAST_RETRANS.swap(u64::from(stats.retrans), Ordering::Relaxed));
    RETRANS.fetch_add(retrans, Ordering::Relaxed);

    Some(Duration::from_micros(delay))
}

pub fn report(delay: u32, unsent: u32, shed: bool) //THE SERVER'S WORD ON ITS VIEWERS
{
    REMOTE.fetch_max(u64::from(delay) + at_target(u64::from(unsent)), Ordering::Relaxed);

    //A SHED VIEWER NEEDS A FRESH PICTURE, NOT A SLOWER ONE
    if shed
    {
        SHED.store(true, Ordering::Relaxed);
        request_keyframe();
    }
}

pub fn request_keyframe() //THE NEXT FRAME MUST STAND ALONE
{
    FORCE_KEYFRAME.store(true, Ordering::Relaxed);
}

pub fn take_keyframe_request() -> bool //CONSUMES A PENDING REQUEST
{
    FORCE_KEYFRAME.swap(false, Ordering::Relaxed)
}

pub fn set_draining(on: bool)
{
    DRAINING.store(on, Ordering::Relaxed);
}

pub fn draining() -> bool
{
    DRAINING.load(Ordering::Relaxed)
}

pub fn bitrate() -> u32 //THE CURRENT TARGET
{
    BITRATE.load(Ordering::Relaxed)
}

pub fn fps() -> u32 //THE RATE CAPTURE SHOULD RUN AT
{
    FPS.load(Ordering::Relaxed).max(1)
}

fn at_target(bytes: u64) -> u64 //MICROSECONDS TO SEND bytes AT THE CURRENT TARGET
{
    bytes * 8_000_000 / u64::from(BITRATE.load(Ordering::Relaxed).max(1))
}

//STRUCTS
/// A token bucket that spaces frames at the current target.
pub struct Pacer
{
    tokens: f64,
    last: Instant,
}

impl Pacer
{
    pub fn new() -> Self
    {
        Self { tokens: f64::INFINITY, last: Instant::now() }
    }

    /// Returns how long the stream should wait before sending `bytes`, bounded by
    /// [`PACING_BURST`](consts::PACING_BURST).
    pub fn delay(&mut self, bytes: usize) -> Duration //HOW LONG BEFORE THIS FRAME MAY GO
    {
        let now = Instant::now();
        let rate = f64::from(BITRATE.load(Ordering::Relaxed).max(1));
        let capacity = rate * consts::PACING_BURST.as_secs_f64() / 8.0;

        self.tokens = (self.tokens + rate * now.duration_since(self.last).as_secs_f64() / 8.0).min(capacity);
        self.last = now;
        self.tokens -= bytes as f64;

        if self.tokens >= 0.0 { return Duration::ZERO; }

        let wait = Duration::from_secs_f64((-self.tokens) * 8.0 / rate);
        self.tokens = self.tokens.max(-capacity);

        wait.min(consts::PACING_BURST)
    }
}
