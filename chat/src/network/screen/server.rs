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

//! # Screen Sharing — Server Side
//!
//! Relays a sharer's stream to its viewers ([`screen`](fn@screen)), with one task and one
//! one-slot video queue per viewer so the share runs at the sharer's rate rather than the slowest
//! viewer's.
//!
//! # Shedding a Slow Viewer
//! The share loop never waits on a viewer. A delta frame that would replace a delta still waiting
//! in that viewer's slot is dropped, and the viewer is marked as needing a keyframe: the replaced
//! frame was the reference for it. A keyframe always preempts the slot, so the recovery picture
//! itself can never be lost to a full queue. Audio has its own small queue with drop-oldest
//! semantics, so it never waits behind a video frame.
//!
//! # Attaching
//! A viewer is built where it attaches ([`attach`]), not on the next frame, and opens on the
//! share's last keyframe, kept in the share's entry of the share table. It then waits for the next
//! IDR like a shed viewer. Asking the sharer for one is the same path a shed viewer uses, so a
//! fresh viewer gets a live picture without waiting for the scheduled keyframe.
//!
//! # Feedback
//! Each viewer task measures the backlog a frame joins before writing it, and each viewer reports
//! its own receive-to-display age and keyframe needs back over the attach socket. A reporter task
//! sends the sharer the worst figures every [`FEEDBACK_INTERVAL`](screen::consts::FEEDBACK_INTERVAL)
//! on a reverse stream of the upload socket. A viewer that shed a frame, or asked for a keyframe,
//! is folded in as `shed`; the sharer answers with an IDR rather than a slower target. One slow
//! viewer among several does not move the global bitrate.
//!
//! # Muted Shares
//! While the sharer is muted by a moderator, viewers are sent a looping placeholder animation
//! (all IDRs) instead of the share.

use std::
{
    collections::{ HashMap, VecDeque },
    time::{ Duration, Instant },
    sync::
    {
        Arc,
        LazyLock,
        Mutex as MutexSync,
        atomic::
        {
            AtomicBool,
            AtomicU64,
            Ordering,
        },
    },
};

use dashmap::DashMap;

use tokio::
{
    time,
    task::AbortHandle,
    net::tcp::{ OwnedReadHalf, OwnedWriteHalf },
    sync::
    {
        Mutex,
        Notify,
    },
};

use crate::
{
    crypto,
    consts::SharedKeys,
    network::
    {
        self,
        Streams,
        codes::PacketCode,
        server::
        {
            self,
            connection::Connection,
        },
        screen::{ self, ScreenPacketCode },
    },
};

//PRIVATE
//STATICS
//A MUTED SHARER'S PLACEHOLDER, ANNEX-B H.264
//ffmpeg -i assets/muted.gif -an -vf "pad=220:216:0:0:color=black,format=yuv420p" -c:v libx264 -profile:v baseline -preset veryslow -crf 20 -x264-params keyint=1:min-keyint=1:scenecut=0:repeat-headers=1 -f h264 assets/muted.h264
/// The placeholder shown for a muted sharer, one Annex B access unit per frame.
static MUTED_ANIMATION: LazyLock<Vec<Vec<u8>>> = LazyLock::new(|| split_access_units(include_bytes!("./assets/muted.h264")));

//EVERY RUNNING SHARE, REACHABLE FROM AN ATTACH
/// Every running share, by sharer id, where an attach can reach it.
static SHARES: LazyLock<DashMap<usize, Arc<Share>>> = LazyLock::new(|| DashMap::new());

//STRUCTS
/// Tears a share down when its task ends: unlists it, stops its reporter and clears the
/// connection's screen stream.
struct ScreenTransferGuard
{
    /// The sharer's session id.
    id: usize,
    /// The task reporting backlog to the sharer.
    feedback: AbortHandle, //THE TASK REPORTING BACK TO THE SHARER
}

/// What a running share leaves where an attaching viewer can reach it.
struct Share //WHAT A SHARE LEAVES WHERE AN ATTACHING VIEWER CAN REACH IT
{
    /// The last access unit that stands on its own.
    keyframe: MutexSync<Option<Vec<u8>>>,     //THE LAST PICTURE THAT STANDS ON ITS OWN
    /// Viewers built at their attach, waiting for the share loop to adopt them.
    pending: MutexSync<Vec<(usize, Viewer)>>, //VIEWERS BUILT AT THE ATTACH, WAITING TO BE PICKED UP
    /// Every served viewer's backlog figures.
    stats: MutexSync<Vec<Arc<Backlog>>>,      //EVERY SERVED VIEWER'S BACKLOG
    /// Whether anybody asked the sharer for a keyframe.
    key_requested: AtomicBool,                //SOMEBODY NEEDS AN IDR
    /// Frames relayed, for the diagnostic line.
    frames: AtomicU64,
    /// Keyframes relayed, for the diagnostic line.
    keyframes: AtomicU64,
    /// Frames dropped for a viewer, for the diagnostic line.
    shed: AtomicU64,
}

/// What one viewer is behind by, as the worst case since the last report.
struct Backlog //WHAT ONE VIEWER IS BEHIND BY, WORST SINCE THE LAST REPORT
{
    /// Microseconds a frame sat in this viewer's queue, plus RTT above the path's minimum.
    delay: AtomicU64,    //MICROSECONDS QUEUED HERE PLUS RTT OVER THE PATH'S BEST
    /// Bytes the viewer's socket has not sent.
    unsent: AtomicU64,   //BYTES THE SOCKET HAS NOT SENT
    /// The viewer's own receive-to-display age, microseconds.
    reported: AtomicU64, //WHAT THE VIEWER SAYS ITS PICTURE IS BEHIND BY
    /// Whether a frame was dropped for this viewer.
    shed: AtomicBool,    //A FRAME WAS DROPPED FOR IT
}

/// One packet waiting for a viewer's writer task.
enum Queued //ONE PACKET WAITING FOR A VIEWER
{
    /// An access unit and when it was queued.
    Video { queued: Instant, data: Vec<u8> },
    /// One Opus packet and when it was queued.
    Audio { queued: Instant, data: Vec<u8> },
}

/// What happened to a video frame offered to a viewer's queue.
enum VideoPush //WHAT HAPPENED TO A VIDEO FRAME
{
    /// The frame is waiting in the slot.
    Queued,
    /// A keyframe is already on its way; the delta was dropped.
    KeyframePending,
    /// The replaced frame was a reference, so the chain is broken.
    Shed,
}

/// A viewer's one-slot video slot and small audio queue.
struct ViewerQueue //A VIEWER'S WAITING PACKETS
{
    /// The newest video frame, and whether it stands on its own.
    video: MutexSync<Option<(Instant, Vec<u8>, bool)>>,
    /// Audio packets and when they were queued, oldest first.
    audio: MutexSync<VecDeque<(Instant, Vec<u8>)>>,
    /// Wakes the writer task.
    ready: Notify,
    /// Set when the socket failed.
    closed: AtomicBool,
}

/// One attached viewer and the tasks that talk to it. Dropping it aborts them.
struct Viewer //ONE ATTACHED CLIENT, AND THE TASKS THAT TALK TO IT
{
    /// The attachment the tasks were built for.
    token: [u8; 32],            //THE ATTACHMENT THESE TASKS WERE BUILT FOR
    /// Handoff to the writer task.
    queue: Arc<ViewerQueue>,    //HANDOFF TO THE WRITER
    /// What the writer measures.
    stats: Arc<Backlog>,        //WHAT THE WRITER MEASURES
    /// The writer task.
    task: AbortHandle,          //THE WRITER
    /// The task reading the viewer's reports.
    reader: AbortHandle,        //THE READER
    /// Whether video is skipped until the next keyframe.
    needs_key: bool,            //SHED: WAITING FOR THE NEXT IDR
    /// Whether the viewer muted the share's audio.
    muted: bool,                //VIEWER MUTED THE SHARE AUDIO
}

//IMPLEMENTATIONS
impl Drop for ScreenTransferGuard
{
    fn drop(&mut self)
    {
        //NOTHING MAY ATTACH TO A SHARE THAT IS OVER
        SHARES.remove(&self.id);

        self.feedback.abort();

        if let Some(mut conn) = server::CONNECTIONS.iter_mut().find(|c| c.id() == Some(&self.id))
        {
            //REMOVE SCREEN STREAM
            conn.remove_screen_stream();
        }
    }
}

impl Drop for Viewer
{
    fn drop(&mut self)
    {
        //A TASK MAY BE PARKED IN write_all
        self.task.abort();
        self.reader.abort();
    }
}

impl Backlog
{
    /// Creates empty backlog figures.
    fn new() -> Self
    {
        Self
        {
            delay: AtomicU64::new(0),
            unsent: AtomicU64::new(0),
            reported: AtomicU64::new(0),
            shed: AtomicBool::new(false),
        }
    }

    /// Records the backlog one frame joined.
    fn observe(&self, queued: Duration, unsent: u64)
    {
        self.delay.fetch_max(queued.as_micros() as u64, Ordering::Relaxed);
        self.unsent.fetch_max(unsent, Ordering::Relaxed);
    }

    /// Records what the viewer says its picture is behind by.
    fn report(&self, delay: u64)
    {
        self.reported.fetch_max(delay, Ordering::Relaxed);
    }

    /// Takes the interval's worst figures and resets them.
    fn take(&self) -> (u64, u64, bool) //DELAY, UNSENT, AND WHETHER IT WAS SHED
    {
        let delay = self.delay.swap(0, Ordering::Relaxed).max(self.reported.swap(0, Ordering::Relaxed));
        let unsent = self.unsent.swap(0, Ordering::Relaxed);
        let shed = self.shed.swap(false, Ordering::Relaxed);

        (delay, unsent, shed)
    }
}

impl ViewerQueue
{
    /// Creates an empty queue.
    fn new() -> Self
    {
        Self
        {
            video: MutexSync::new(None),
            audio: MutexSync::new(VecDeque::new()),
            ready: Notify::new(),
            closed: AtomicBool::new(false),
        }
    }

    /// Offers one video frame, replacing what a newer frame made useless.
    fn push_video(&self, data: Vec<u8>, keyframe: bool) -> VideoPush
    {
        let outcome =
        {
            let mut slot = self.video.lock().unwrap();
            let held_keyframe = slot.as_ref().is_some_and(|(_, _, key)| *key);

            match (keyframe, held_keyframe)
            {
                //A KEYFRAME IS ALWAYS WELCOME
                (true, _) => { *slot = Some((Instant::now(), data, true)); VideoPush::Queued },

                //KEEP THE KEYFRAME ALREADY ON ITS WAY
                (false, true) => VideoPush::KeyframePending,

                //THE WAITING DELTA WAS THIS FRAME'S REFERENCE
                (false, false) if slot.is_some() => { *slot = None; VideoPush::Shed },

                (false, false) => { *slot = Some((Instant::now(), data, false)); VideoPush::Queued }
            }
        };

        if matches!(outcome, VideoPush::Queued) { self.ready.notify_one(); }

        outcome
    }

    /// Queues one audio packet, dropping the oldest beyond the bound.
    fn push_audio(&self, data: Vec<u8>)
    {
        let mut audio = self.audio.lock().unwrap();

        if audio.len() >= screen::consts::VIEWER_AUDIO_BOUND { audio.pop_front(); }
        audio.push_back((Instant::now(), data));

        drop(audio);

        self.ready.notify_one();
    }

    /// Takes the next packet to write, audio first.
    fn next(&self) -> Option<Queued>
    {
        if let Some((queued, data)) = self.audio.lock().unwrap().pop_front()
        {
            return Some(Queued::Audio { queued, data });
        }

        self.video.lock().unwrap().take().map(|(queued, data, _)| Queued::Video { queued, data })
    }

    /// Returns how many packets are waiting.
    fn depth(&self) -> usize
    {
        self.audio.lock().unwrap().len() + usize::from(self.video.lock().unwrap().is_some())
    }

    /// Stops the writer task at its next wakeup.
    fn close(&self)
    {
        self.closed.store(true, Ordering::Relaxed);
        self.ready.notify_one();
    }

    /// Returns whether the socket failed.
    fn is_closed(&self) -> bool
    {
        self.closed.load(Ordering::Relaxed)
    }
}

//FUNCTIONS
/// Cuts an Annex B stream into one buffer per access unit, splitting at each SPS.
fn split_access_units(bitstream: &[u8]) -> Vec<Vec<u8>> //CUT AN ANNEX-B STREAM INTO ONE BUFFER PER FRAME
{
    let mut units = Vec::new();
    let mut start = None;

    let mut index = 0;
    while index + 3 < bitstream.len()
    {
        //NOT A START CODE
        if bitstream[index..index + 3] != [0, 0, 1] { index += 1; continue; }

        //AN SPS OPENS A FRAME, SO THE PREVIOUS ONE ENDS
        if bitstream[index + 3] & 0x1f == 7
        {
            //BACK UP OVER A FOUR-BYTE START CODE
            let mut boundary = index;
            while boundary > 0 && bitstream[boundary - 1] == 0 { boundary -= 1; }

            if let Some(start) = start.replace(boundary)
            {
                units.push(bitstream[start..boundary].to_vec());
            }
        }

        index += 3;
    }

    //THE LAST FRAME RUNS TO THE END
    if let Some(start) = start { units.push(bitstream[start..].to_vec()); }

    units
}

/// Spawns the tasks that write to one viewer and read its reports.
///
/// The viewer starts needing a keyframe.
///
/// # Returns
/// `None` if the packet stream could not be keyed.
fn spawn_viewer //ONE WRITER AND ONE READER PER VIEWER
(
    stream: Arc<Mutex<OwnedWriteHalf>>,
    read_stream: OwnedReadHalf,
    keys: &SharedKeys,
    token: [u8; 32],
    share: Arc<Share>,
) -> Option<Viewer>
{
    //THE REX STREAM AND SEQUENCE ARE PER VIEWER
    let mut rex_stream = crypto::init_rex_stream(keys, &token)?;
    let queue = Arc::new(ViewerQueue::new());
    let stats = Arc::new(Backlog::new());

    //WRITER
    let writer_queue = queue.clone();
    let writer_stats = stats.clone();
    let writer_stream = stream.clone();

    let task = tokio::spawn(async move
    {
        let mut seq = 0usize;
        let mut last_acked = 0u64;
        let mut capped_at = Instant::now();

        loop
        {
            while let Some(item) = writer_queue.next()
            {
                let mut stream = writer_stream.lock().await;
                let tcp = screen::tcp_stats(stream.as_ref());

                let (unsent, inflated, acked) = tcp.map_or((0, Duration::ZERO, 0), |stats| (stats.unsent, stats.inflated, stats.acked));

                //THE BACKLOG THIS PACKET JOINS
                let (queued_at, code) = match item
                {
                    Queued::Video { queued, data } => (queued, ScreenPacketCode::Video { data }),
                    Queued::Audio { queued, data } => (queued, ScreenPacketCode::Audio { data }),
                };

                writer_stats.observe(queued_at.elapsed() + inflated, unsent);

                if screen::send_frame(&mut stream, code, &mut rex_stream, Some(&mut seq)).await.is_err()
                {
                    writer_queue.close();

                    return;
                }

                //BOUND THE BUFFERS TO WHAT THIS VIEWER DRAINS
                let elapsed = capped_at.elapsed();

                if elapsed >= Duration::from_secs(1)
                {
                    let goodput = acked.saturating_sub(last_acked) as f64 * 8.0 / elapsed.as_secs_f64();
                    let target = (goodput as u32).clamp(screen::consts::MIN_BITRATE, screen::consts::H264_BITRATE);

                    screen::cap_socket_buffers(stream.as_ref(), target);

                    last_acked = acked;
                    capped_at = Instant::now();
                }
            }

            if writer_queue.is_closed() { return; }

            writer_queue.ready.notified().await;
        }
    }).abort_handle();

    //READER
    let reader_stats = stats.clone();
    let reader_queue = queue.clone();
    let reader_keys = keys.clone();

    let reader = tokio::spawn(async move
    {
        let Some(mut reverse_stream) = crypto::init_reverse_stream(&reader_keys, &token) else
        {
            reader_queue.close();

            return;
        };

        let mut read_stream = read_stream;
        let mut streams = (&mut read_stream, stream);
        let mut seq = 0usize;

        while let Some(code) = screen::receive_frame(&mut streams, &mut reverse_stream, &mut seq).await
        {
            if let ScreenPacketCode::Feedback { delay, shed, .. } = code
            {
                //A VIEWER'S WORD IS BOUNDED
                let delay = u64::from(delay).min(screen::consts::MAX_REPORTED_DELAY.as_micros() as u64);

                reader_stats.report(delay);

                if shed { share.key_requested.store(true, Ordering::Relaxed); }
            }
        }

        //THE VIEWER'S SOCKET WENT AWAY
        reader_queue.close();
    }).abort_handle();

    //A JUST-ATTACHED VIEWER STARTS LIKE A SHED ONE
    Some(Viewer { token, queue, stats, task, reader, needs_key: true, muted: false })
}

/// Returns the index of the placeholder frame due now, played off the wall clock.
fn muted_frame(started: &Instant) -> Option<usize> //INDEX OF THE PLACEHOLDER FRAME DUE RIGHT NOW
{
    //PLAY THE ANIMATION OFF THE WALL CLOCK
    let frames = MUTED_ANIMATION.len();
    if frames == 0 { return None; }

    Some((started.elapsed().as_millis() / screen::consts::MUTED_FRAME_INTERVAL.as_millis()) as usize % frames)
}

/// Ends a share whose upload socket closed: detaches every viewer and tells the sharer.
async fn end_share(id: usize) //TEAR THE SHARE DOWN AND TELL EVERYONE ABOUT IT
{
    //TAKE THE SHARE STATE WITHOUT ABORTING
    let (write_stream, keys, username) =
    {
        let mut conn = match server::CONNECTIONS.iter_mut().find(|c| c.id() == Some(&id))
        {
            Some(c) => c,
            None => return
        };

        //ALREADY TORN DOWN BY SOMEBODY ELSE
        if conn.take_screen_stream().is_none() { return; }

        (conn.write_stream().clone(), conn.keys().cloned(), conn.username().cloned())
    };

    log::info!("Screen share ended (upload socket closed): {}", server::log_addr(&id));

    //DEATTACH EVERY VIEWER
    if let Some(username) = username
    {
        server::deattach(id, &username).await;
    }

    //TELL THE SHARER ITS SHARE IS GONE
    network::send(&mut *write_stream.lock().await, PacketCode::Screen { token: None }, keys.as_ref()).await;
}

/// Reports the worst viewer's backlog to the sharer every
/// [`FEEDBACK_INTERVAL`](screen::consts::FEEDBACK_INTERVAL), skipping intervals with nothing to
/// say.
async fn feedback //TELL THE SHARER HOW FAR BEHIND ITS VIEWERS ARE
(
    share: Arc<Share>,
    stream: Arc<Mutex<OwnedWriteHalf>>,
    mut rex_stream: crypto::RexPacketStream,
)
{
    let mut seq = 0usize;
    let mut interval = time::interval(screen::consts::FEEDBACK_INTERVAL);

    loop
    {
        interval.tick().await;

        let stats = share.stats.lock().map(|stats| stats.clone()).unwrap_or_default();
        let viewers = stats.len();

        //THE WORST VIEWER
        let mut worst = 0u64;
        let mut unsent = 0u64;
        let mut congested = 0usize;
        let mut shed = false;

        for (delay, un, dropped) in stats.iter().map(|stats| stats.take())
        {
            shed |= dropped;

            worst = worst.max(delay);
            unsent = unsent.max(un);

            if delay >= screen::consts::RATE_DELAY.as_micros() as u64 { congested += 1; }
        }

        //ONE SLOW VIEWER MUST NOT DRAG EVERYBODY DOWN
        if congested <= 1 && viewers > 1
        {
            worst = 0;
            unsent = 0;
        }

        //A SHED OR ASKING VIEWER NEEDS A FRESH PICTURE
        if shed { share.key_requested.store(true, Ordering::Relaxed); }

        let key = share.key_requested.swap(false, Ordering::Relaxed);

        //NOTHING TO SAY
        if worst == 0 && unsent == 0 && !key { continue; }

        let code = ScreenPacketCode::Feedback
        {
            delay: worst.min(u64::from(u32::MAX)) as u32,
            unsent: unsent.min(u64::from(u32::MAX)) as u32,
            shed: key,
        };

        if screen::send_frame(&mut *stream.lock().await, code, &mut rex_stream, Some(&mut seq)).await.is_err()
        {
            return;
        }
    }
}

//PUBLIC
/// Attaches a viewer to a running share.
///
/// The viewer is built here, sent the share's last keyframe, and left for the share loop to
/// adopt on its next frame. It also gets a live keyframe requested, so a still desktop does not
/// leave it looking at an old picture.
///
/// # Parameters
/// - `sharer_id`: The sharer's session id.
/// - `client_id`: The viewer's session id.
/// - `keys`: The viewer's session keys.
/// - `read_stream`: The viewer socket's read half, for its reports.
/// - `stream`: The viewer socket's write half.
/// - `token`: The attachment's token.
///
/// # Returns
/// `false` if the share is not running or the viewer could not be built.
pub fn attach //BUILD A VIEWER WHERE IT ATTACHES
(
    sharer_id: usize,
    client_id: usize,
    keys: &SharedKeys,
    read_stream: OwnedReadHalf,
    stream: Arc<Mutex<OwnedWriteHalf>>,
    token: [u8; 32],
) -> bool
{
    //THE SHARE IS OVER (OR NEVER STARTED)
    let Some(share) = SHARES.get(&sharer_id).map(|share| share.clone()) else { return false; };

    let Some(viewer) = spawn_viewer(stream, read_stream, keys, token, share.clone()) else { return false; };

    //OPEN ON THE SHARE'S LAST KEYFRAME
    if let Some(frame) = share.keyframe.lock().ok().and_then(|frame| frame.clone())
    {
        viewer.queue.push_video(frame, true);
    }

    //A FRESH VIEWER NEEDS A LIVE PICTURE
    share.key_requested.store(true, Ordering::Relaxed);

    //PICKED UP BY THE SHARE LOOP ON ITS NEXT FRAME
    match share.pending.lock()
    {
        Ok(mut pending) =>
        {
            pending.push((client_id, viewer));

            true
        },
        Err(_) => false,
    }
}

/// Receives one client's screen share and relays it to its viewers until the upload socket
/// closes.
///
/// # Parameters
/// - `token`: The share's token.
/// - `id`: The sharer's session id.
/// - `streams`: The upload socket's halves.
/// - `task`: This task's abort handle, registered on the connection.
pub async fn screen(token: [u8; 32], id: usize, streams: &mut Streams<'_>, task: AbortHandle)
{
    //GET CLIENT KEYS
    let keys =
    {
        //FIND CONNECTION BY ID
        let conn = server::CONNECTIONS.iter_mut()
            .find(|e| e.value().id() == Some(&id));

        match conn
        {
            Some(mut c) =>
            {
                let keys = match c.keys()
                {
                    Some(k) => k.clone(),
                    None => return
                };

                //ADD SCREEN STREAM
                c.set_screen_stream(task);

                keys
            },
            None => return
        }
    };

    //AN AUXILIARY SOCKET IS LOGGED AS THE MAIN ONE
    let owner = server::log_addr(&id);

    log::info!("Screen share started: {owner}");

    //LOCAL SEQ
    let mut seq = 0usize;

    //ONE ENTRY PER ATTACHED VIEWER
    let mut viewers = HashMap::<usize, Viewer>::new();

    //INIT REX STREAMS, ONE EACH WAY
    let mut rex_stream = crypto::init_rex_stream(&keys, &token).unwrap();
    let reverse_stream = crypto::init_reverse_stream(&keys, &token).unwrap();

    //PLACEHOLDER PLAYBACK STATE
    let started = Instant::now();
    let mut sent_muted_frame = None;

    //REACHABLE FROM THE ACCEPT LOOP FROM HERE ON
    let share = Arc::new(Share
    {
        keyframe: MutexSync::new(None),
        pending: MutexSync::new(Vec::new()),
        stats: MutexSync::new(Vec::new()),
        key_requested: AtomicBool::new(false),
        frames: AtomicU64::new(0),
        keyframes: AtomicU64::new(0),
        shed: AtomicU64::new(0),
    });

    //REPORT BACK ON THE SAME SOCKET
    let reporter = tokio::spawn(feedback(share.clone(), streams.1.clone(), reverse_stream)).abort_handle();

    //DISCONNECT GUARD
    let _guard = ScreenTransferGuard { id, feedback: reporter };

    SHARES.insert(id, share.clone());

    let mut stats_at = Instant::now();

    //LOOP READING
    loop
    {
        //READ
        let read = match screen::receive_frame(streams, &mut rex_stream, &mut seq).await
        {
            Some(r) => r,
            None => break
        };

        //IS THE SHARER MUTED? (DROP THE GUARD)
        let muted = server::CONNECTIONS.iter()
            .find(|c| c.id() == Some(&id))
            .map(|c| *c.muted())
            .unwrap_or(false);

        //SILENCE MUTED USERS, SEND THE PLACEHOLDER
        let read = match (muted, read)
        {
            //ONLY THE SERVER REPORTS
            (_, ScreenPacketCode::Feedback { .. }) => continue,

            //NOT MUTED, FORWARD WHATEVER CAME IN
            (false, read) =>
            {
                sent_muted_frame = None;

                read
            },

            //MUTED AUDIO IS SIMPLY DROPPED
            (true, ScreenPacketCode::Audio { .. }) => continue,

            (true, ScreenPacketCode::Video { .. }) =>
            {
                let Some(frame) = muted_frame(&started) else { continue; };

                //DO NOT RESEND THE SAME PLACEHOLDER FRAME
                if sent_muted_frame == Some(frame) { continue; }
                sent_muted_frame = Some(frame);

                ScreenPacketCode::Video { data: MUTED_ANIMATION[frame].clone() }
            },
        };

        //TAKE ON WHOEVER ATTACHED SINCE THE LAST FRAME
        let arrivals = share.pending.lock().map(|mut pending| pending.drain(..).collect::<Vec<_>>()).unwrap_or_default();

        for (client_id, viewer) in arrivals
        {
            viewers.insert(client_id, viewer);

            log::info!("Screen viewer serving ({} attached): share of {owner}", viewers.len());
        }

        //COLLECT WHO IS STILL ATTACHED TO US
        let entries: Vec<(usize, [u8; 32], bool)> = server::CONNECTIONS.iter().filter_map(|entry|
        {
            match entry.value()
            {
                Connection::Authenticated { id: client_id, attached_screen, .. } =>
                {
                    //FILTER ATTACHED CLIENTS
                    if let Some(attached_screen) = attached_screen && attached_screen.target_id == id
                    {
                        Some((*client_id, attached_screen.token, attached_screen.muted))
                    } else { None }
                },
                _ => None,
            }
        }).collect();

        //RETIRE WHOEVER LEFT OR WHOSE SOCKET FAILED
        viewers.retain(|client_id, viewer|
        {
            if viewer.queue.is_closed()
            {
                log::warn!("Screen viewer dropped (write failed): share of {owner}");

                return false;
            }

            match entries.iter().find(|(e, token, _)| e == client_id && *token == viewer.token)
            {
                Some((_, _, muted)) =>
                {
                    //REFRESH MUTE
                    viewer.muted = *muted;

                    true
                },
                None => false,
            }
        });

        //WHAT THE REPORTER READS
        if let Ok(mut stats) = share.stats.lock()
        {
            *stats = viewers.values().map(|viewer| viewer.stats.clone()).collect();
        }

        //FORWARD PACKET
        match read
        {
            ScreenPacketCode::Video { data } =>
            {
                let keyframe = screen::is_keyframe(&data);

                for viewer in viewers.values_mut()
                {
                    //A VIEWER THAT MISSED A FRAME WAITS FOR AN IDR
                    if viewer.needs_key
                    {
                        if !keyframe { continue; }

                        viewer.needs_key = false;
                    }

                    if let VideoPush::Shed = viewer.queue.push_video(data.clone(), keyframe)
                    {
                        //ONLY THE FIRST SHED FRAME OF A RUN IS LOGGED
                        if !viewer.needs_key { log::warn!("Screen viewer shed (link too slow): share of {owner}"); }

                        viewer.needs_key = true;
                        viewer.stats.shed.store(true, Ordering::Relaxed);
                        share.key_requested.store(true, Ordering::Relaxed);
                        share.shed.fetch_add(1, Ordering::Relaxed);
                    }
                }

                share.frames.fetch_add(1, Ordering::Relaxed);
                if keyframe { share.keyframes.fetch_add(1, Ordering::Relaxed); }

                //KEEP THE LAST KEYFRAME FOR THE NEXT ATTACH
                if keyframe
                {
                    if let Ok(mut cached) = share.keyframe.lock() { *cached = Some(data); }
                }
            },

            ScreenPacketCode::Audio { data } =>
            {
                for (client_id, viewer) in viewers.iter_mut()
                {
                    //PREVENT FEEDBACK
                    if *client_id == id { continue; }

                    //SKIP USERS WHO MUTED THE STREAM
                    if viewer.muted { continue; }

                    viewer.queue.push_audio(data.clone());
                }
            },

            ScreenPacketCode::Feedback { .. } => {},
        }

        if screen::stats_enabled() && stats_at.elapsed() >= Duration::from_secs(1)
        {
            let frames = share.frames.swap(0, Ordering::Relaxed);
            let keyframes = share.keyframes.swap(0, Ordering::Relaxed);
            let shed = share.shed.swap(0, Ordering::Relaxed);
            let depths: Vec<usize> = viewers.values().map(|viewer| viewer.queue.depth()).collect();

            log::info!("Screen stats: {} viewers, {frames} frames/s, {keyframes} keyframes/s, {shed} shed, queues {depths:?}: share of {owner}",
                viewers.len());

            stats_at = Instant::now();
        }
    }

    //THE UPLOAD SOCKET DIED
    end_share(id).await;
}
