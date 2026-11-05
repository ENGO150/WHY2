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
pub mod audio;
pub mod capture;
pub mod display;
pub mod encoder;
pub mod options;
pub mod rate;
pub mod video;

#[cfg(target_os = "linux")]
pub mod audio_pipewire;

#[cfg(target_os = "linux")]
pub mod portal;

use std::
{
    time::{ Duration, Instant },
    sync::
    {
        Arc,
        RwLock,
        atomic::{ AtomicBool, AtomicU32, AtomicU64, Ordering },
    },
};

use tokio::
{
    task,
    time,
    io::AsyncWriteExt,
    net::tcp::OwnedWriteHalf,
    sync::
    {
        Mutex,
        mpsc::
        {
            self,
            Sender,
            Receiver,
            UnboundedSender,
        },
    },
};

use winit::event_loop::EventLoopProxy;

use crate::
{
    t,
    crypto,
    options as chat_options,
    network::
    {
        self,
        codes::PacketCode,
        client::{ ClientEvent, handshake },
        screen::
        {
            self,
            consts,
            ScreenPacketCode,
            client::audio::AudioFrame,
        },
    },
};

//STRUCTS
/// One received access unit and what the viewer knows about it.
#[derive(Clone)]
pub struct VideoFrame
{
    /// The Annex B access unit.
    pub data: Vec<u8>,
    /// When it was read off the socket.
    pub received: Instant,
    /// Whether it can be decoded on its own.
    pub keyframe: bool,
}

/// What a viewer tells the sharer about its own picture.
#[derive(Default)]
pub struct ViewerReports
{
    needs_key: AtomicBool,  //WAITING FOR AN IDR
    max_age: AtomicU64,     //OLDEST RECEIVE-TO-DISPLAY AGE SINCE THE LAST REPORT, MICROSECONDS
    depth: AtomicU32,       //FRAMES WAITING TO BE DECODED
    stat_age: AtomicU64,    //OLDEST AGE THIS INTERVAL, MICROSECONDS
    stat_decode: AtomicU64, //LONGEST DECODE THIS INTERVAL, MICROSECONDS
    stat_frames: AtomicU64, //FRAMES DISPLAYED THIS INTERVAL
    stat_dropped: AtomicU64, //FRAMES DROPPED THIS INTERVAL
    stat_requests: AtomicU64, //KEYFRAMES REQUESTED THIS INTERVAL
}

pub struct ScreenShareRequest
{
    pub rx: Receiver<VideoFrame>,
    pub running: Arc<AtomicBool>,
    pub reports: Arc<ViewerReports>,
    pub deattach: UnboundedSender<()>, //DEATTACH REQUEST FROM THE WINDOW
}

//ENUMS
pub enum UserEvent //CUSTOM WINIT EVENTS
{
    NewSession(ScreenShareRequest),
    NewFrame,
}

//GLOBAL VARIABLES
pub static SCREEN_SHARE_PROXY: RwLock<Option<EventLoopProxy<UserEvent>>> = RwLock::new(None);

//WHERE AN ATTACHED SHARE'S PICTURE GOES
pub static SCREEN_FRAME_SINK: RwLock<Option<UnboundedSender<Vec<u8>>>> = RwLock::new(None);

//IMPLEMENTATIONS
impl ViewerReports
{
    pub fn new() -> Self
    {
        Self::default()
    }

    pub fn request_keyframe(&self) //ASK FOR A FRESH PICTURE
    {
        self.needs_key.store(true, Ordering::Relaxed);
        self.stat_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub fn keyframe_seen(&self) //A FRESH PICTURE ARRIVED
    {
        self.needs_key.store(false, Ordering::Relaxed);
    }

    pub fn note_age(&self, age: Duration)
    {
        let micros = age.as_micros() as u64;

        self.max_age.fetch_max(micros, Ordering::Relaxed);
        self.stat_age.fetch_max(micros, Ordering::Relaxed);
    }

    pub fn note_decode(&self, time: Duration)
    {
        self.stat_decode.fetch_max(time.as_micros() as u64, Ordering::Relaxed);
    }

    pub fn note_depth(&self, depth: usize)
    {
        self.depth.store(depth.min(u32::MAX as usize) as u32, Ordering::Relaxed);
    }

    pub fn note_frame(&self)
    {
        self.stat_frames.fetch_add(1, Ordering::Relaxed);
    }

    pub fn note_dropped(&self, count: usize)
    {
        self.stat_dropped.fetch_add(count as u64, Ordering::Relaxed);
    }

    /// Takes the figures the sharer is told, keeping the keyframe request until it is served.
    pub fn take_report(&self) -> (u32, u32, bool) //AGE, QUEUE DEPTH, NEEDS A KEYFRAME
    {
        let age = self.max_age.swap(0, Ordering::Relaxed).min(u64::from(u32::MAX)) as u32;

        (age, self.depth.load(Ordering::Relaxed), self.needs_key.load(Ordering::Relaxed))
    }

    pub fn stats_report(&self) -> String //ONE INTERVAL'S COUNTERS
    {
        format!
        (
            "screen view: age={}ms decode={}ms depth={} frames={} dropped={} key_requests={}",
            self.stat_age.swap(0, Ordering::Relaxed) / 1000,
            self.stat_decode.swap(0, Ordering::Relaxed) / 1000,
            self.depth.load(Ordering::Relaxed),
            self.stat_frames.swap(0, Ordering::Relaxed),
            self.stat_dropped.swap(0, Ordering::Relaxed),
            self.stat_requests.swap(0, Ordering::Relaxed),
        )
    }
}

//FUNCTIONS
/// Sends one share-audio packet on the upload socket.
async fn send_audio
(
    write_stream: &Arc<Mutex<OwnedWriteHalf>>,
    data: Vec<u8>,
    rex_stream: &mut crypto::RexPacketStream,
    seq: &mut usize,
) -> std::io::Result<()>
{
    screen::send_frame(&mut *write_stream.lock().await, ScreenPacketCode::Audio { data }, rex_stream, Some(seq)).await
}

pub async fn screen(token: [u8; 32], events: Sender<ClientEvent>)
{
    //INIT FILE CONNECTION
    let (mut read_stream, mut write_stream) = handshake::connect(chat_options::get_server_address()).await
        .expect("Screen upload connection failed");

    //SEND TOKEN
    write_stream.write_all(&token).await.unwrap();

    //SHARED STATE
    let (tx, mut rx) = mpsc::channel(consts::VIDEO_CHANNEL_BOUND);
    let (audio_tx, mut audio_rx) = mpsc::channel(consts::MULTIPLEX_CHANNEL_BOUND);

    let running = Arc::new(AtomicBool::new(true));

    //FRESH BITRATE ESTIMATE
    rate::start();

    //KEEP THE UPLOAD'S BACKLOG WITHIN THE LATENCY BUDGET
    screen::cap_socket_buffers(write_stream.as_ref(), rate::bitrate());

    //SPAWN CAPTURE TASKS (BLOCKING CPU LOOP)
    let running_capture = running.clone();
    let running_audio = running.clone();
    let capture = task::spawn_blocking(move || capture::capture_loop(tx, running_capture, consts::TARGET_FPS));
    tokio::spawn(audio::spawn_audio_capture(audio_tx, running_audio));

    //LOCAL SEQ COUNTER
    let mut seq = 0usize;

    //INIT REX STREAMS, ONE EACH WAY
    let keys = chat_options::get_keys().unwrap();
    let mut rex_stream = crypto::init_rex_stream(&keys, &token).unwrap();
    let mut reverse_stream = crypto::init_reverse_stream(&keys, &token).unwrap();

    //THE READER NEEDS THE WRITE HALF BESIDE IT
    let write_stream = Arc::new(Mutex::new(write_stream));
    let reader_write = write_stream.clone();

    //THE SERVER'S WORD ON ITS VIEWERS
    let reader = tokio::spawn(async move
    {
        let mut streams = (&mut read_stream, reader_write);
        let mut seq = 0usize;

        while let Some(code) = screen::receive_frame(&mut streams, &mut reverse_stream, &mut seq).await
        {
            if let ScreenPacketCode::Feedback { delay, unsent, shed } = code { rate::report(delay, unsent, shed); }
        }
    }).abort_handle();

    //PACE, DRAIN AND RECOVER
    let mut pacer = rate::Pacer::new();
    let mut awaiting_key = false;
    let mut capped = rate::bitrate();
    let mut tick = time::interval(consts::DRAIN_POLL_INTERVAL);
    let mut stats_at = Instant::now();
    let mut write_failed = false;

    //LOOP SENDING FRAMES
    'share: loop
    {
        //EXIT ON DISABLED SCREEN
        if !options::get_use_screen() { break; }

        tokio::select!
        {
            //AUDIO FIRST, BEFORE A VIDEO WRITE CAN HOLD IT
            biased;

            msg = audio_rx.recv() =>
            {
                let Some(audio_frame) = msg else { break };

                //SOUND NOT SHARED
                if !options::get_share_audio() { continue; }

                let bytes = audio_frame.data.len();
                let started = Instant::now();

                if send_audio(&write_stream, audio_frame.data, &mut rex_stream, &mut seq).await.is_err()
                {
                    write_failed = true;
                    break 'share;
                }

                rate::record(bytes, started.elapsed());
            },

            //VIDEO FRAME
            msg = rx.recv() =>
            {
                let compressed_frame = match msg
                {
                    Some(f) => f,
                    None => break,
                };

                let keyframe = screen::is_keyframe(&compressed_frame);

                //THE QUEUE THIS FRAME WOULD JOIN
                let queued =
                {
                    let stream = write_stream.lock().await;

                    rate::queue_estimate(stream.as_ref())
                };

                //A LATE FRAME IS NOT WORTH SENDING
                if rate::draining() || queued.is_some_and(|queue| queue >= consts::DRAIN_HARD)
                {
                    rate::set_draining(true);
                    rate::request_keyframe();
                    rate::note_dropped();

                    awaiting_key = true;

                    continue;
                }

                //RECOVERY WAITS FOR AN IDR
                if awaiting_key && !keyframe
                {
                    rate::note_dropped();

                    continue;
                }

                //SPACE THE FRAME WITHOUT STOPPING AUDIO
                let wait = pacer.delay(compressed_frame.len());

                if !wait.is_zero()
                {
                    let deadline = time::Instant::now() + wait;

                    loop
                    {
                        tokio::select!
                        {
                            _ = time::sleep_until(deadline) => break,

                            audio = audio_rx.recv() =>
                            {
                                let Some(audio_frame) = audio else
                                {
                                    write_failed = true;
                                    break 'share;
                                };

                                if options::get_share_audio()
                                {
                                    let bytes = audio_frame.data.len();
                                    let started = Instant::now();

                                    if send_audio(&write_stream, audio_frame.data, &mut rex_stream, &mut seq).await.is_err()
                                    {
                                        write_failed = true;
                                        break 'share;
                                    }

                                    rate::record(bytes, started.elapsed());
                                }
                            },
                        }
                    }
                }

                let bytes = compressed_frame.len();

                let mut stream = write_stream.lock().await;

                //THE BUFFERS FOLLOW THE TARGET
                if rate::bitrate() != capped
                {
                    capped = rate::bitrate();

                    screen::cap_socket_buffers(stream.as_ref(), capped);
                }

                let started = Instant::now();
                let sent = screen::send_frame(&mut stream,
                    ScreenPacketCode::Video { data: compressed_frame }, &mut rex_stream, Some(&mut seq)).await;
                let busy = started.elapsed();

                drop(stream);

                if sent.is_err()
                {
                    write_failed = true;
                    break 'share;
                }

                rate::record(bytes, busy);
                rate::note_sent(keyframe);

                awaiting_key = false;
            },

            //KEEP AN EYE ON THE QUEUE WHILE NOTHING IS SENT
            _ = tick.tick() =>
            {
                let queued =
                {
                    let stream = write_stream.lock().await;

                    rate::queue_estimate(stream.as_ref())
                };

                if rate::draining()
                {
                    //THE CATCH-UP IS A FRESH PICTURE ONCE THE LINK IS CLEAR
                    if queued.is_none_or(|queue| queue < consts::DRAIN_TARGET)
                    {
                        rate::set_draining(false);
                        rate::request_keyframe();

                        awaiting_key = true;
                    }
                } else if queued.is_some_and(|queue| queue >= consts::DRAIN_HARD)
                {
                    rate::set_draining(true);
                    rate::request_keyframe();

                    awaiting_key = true;
                }

                if rate::stats_enabled() && stats_at.elapsed() >= Duration::from_secs(1)
                {
                    eprintln!("{}", rate::stats_report());

                    stats_at = Instant::now();
                }
            }
        }
    }

    //STOP THE CAPTURE LOOP AND REPORT WHY
    running.store(false, Ordering::Relaxed);
    reader.abort();

    let captured = match capture.await
    {
        Ok(Err(reason)) => Some(reason),
        Err(_) => Some(t!("screen.error.crashed").to_owned()), //PANICKED OR CANCELLED
        Ok(Ok(())) => None,
    };

    let reason = if write_failed { Some(t!("screen.error.connection").to_owned()) } else { captured };

    if let Some(reason) = reason
    {
        events.send(ClientEvent::ScreenFailed(reason)).await.ok();
    }
}

pub async fn attach(token: [u8; 32], main_stream: Arc<Mutex<OwnedWriteHalf>>)
{
    //INIT FILE CONNECTION
    let (mut read_stream, mut write_stream) = handshake::connect(chat_options::get_server_address()).await
        .expect("Screen download connection failed");

    //SEND TOKEN (HAHA, SLEEP TOKEN)
    write_stream.write_all(&token).await.unwrap();

    //A VIEWER KNOWS NO TARGET, SO THE CEILING SETS ITS BUDGET
    screen::cap_socket_buffers(read_stream.as_ref(), consts::SOCKET_TARGET);

    //SHARED STATE
    let (tx, rx) = mpsc::channel(consts::MULTIPLEX_CHANNEL_BOUND);
    let (audio_tx, audio_rx) = mpsc::channel(consts::NETWORK_CHANNEL_BOUND);
    let running = Arc::new(AtomicBool::new(true));
    let reports = Arc::new(ViewerReports::new());

    let running_audio = running.clone();
    tokio::spawn(audio::spawn_audio_playback(audio_rx, running_audio));

    //INIT REX STREAM
    let keys = chat_options::get_keys().unwrap();
    let mut rex_stream = crypto::init_rex_stream(&keys, &token).unwrap();

    //BRIDGE THE WINIT EVENT LOOP BACK TO THE SERVER
    let (deattach_tx, mut deattach_rx) = mpsc::unbounded_channel::<()>();
    tokio::spawn(async move
    {
        while deattach_rx.recv().await.is_some()
        {
            //DEATTACH ON SERVER
            network::send(&mut *main_stream.lock().await,
                PacketCode::DeattachRequest, chat_options::get_keys().as_ref()).await;
        }
    });

    //THE WRITE HALF IS SHARED WITH THE REPORT TASK
    let write_stream = Arc::new(Mutex::new(write_stream));

    //TELL THE SHARER WHAT THE PICTURE IS LIKE
    let report_keys = keys.clone();
    let report_running = running.clone();
    let report_reports = reports.clone();
    let report_write = write_stream.clone();
    tokio::spawn(async move
    {
        let Some(mut reverse_stream) = crypto::init_reverse_stream(&report_keys, &token) else { return; };

        let mut seq = 0usize;
        let mut interval = time::interval(consts::VIEWER_REPORT_INTERVAL);
        let mut stats_at = Instant::now();

        loop
        {
            interval.tick().await;

            if !report_running.load(Ordering::Relaxed) { return; }

            let (delay, depth, shed) = report_reports.take_report();

            let sent = screen::send_frame(&mut *report_write.lock().await,
                ScreenPacketCode::Feedback { delay, unsent: depth, shed }, &mut reverse_stream, Some(&mut seq)).await;

            if sent.is_err()
            {
                report_running.store(false, Ordering::Relaxed);

                return;
            }

            if screen::stats_enabled() && stats_at.elapsed() >= Duration::from_secs(1)
            {
                eprintln!("{}", report_reports.stats_report());

                stats_at = Instant::now();
            }
        }
    });

    //SPAWN NETWORK READER TASK
    let running_net = running.clone();
    let network_reports = reports.clone();
    tokio::spawn(async move
    {
        let mut streams = (&mut read_stream, write_stream);
        let mut seq = 0usize;

        while running_net.load(Ordering::Relaxed)
        {
            //EXIT ON DISABLED ATTACH
            if !options::get_attach_screen()
            {
                running_net.store(false, Ordering::Relaxed);
                return;
            }

            let read = match screen::receive_frame(&mut streams, &mut rex_stream, &mut seq).await
            {
                Some(r) => r,
                None =>
                {
                    running_net.store(false, Ordering::Relaxed);
                    return;
                }
            };

            match read
            {
                ScreenPacketCode::Video { data } =>
                {
                    let keyframe = screen::is_keyframe(&data);

                    network_reports.note_frame();

                    //CLONED OUT OF THE LOCK - THE SINK MAY CHANGE
                    let sink = SCREEN_FRAME_SINK.read().unwrap().clone();

                    match sink
                    {
                        Some(sink) => { sink.send(data).ok(); },

                        None =>
                        {
                            tx.send(VideoFrame { data, received: Instant::now(), keyframe }).await.ok();

                            if let Some(proxy) = SCREEN_SHARE_PROXY.read().unwrap().as_ref()
                            {
                                proxy.send_event(UserEvent::NewFrame).ok();
                            }
                        },
                    }
                },

                ScreenPacketCode::Audio { data } =>
                {
                    //ONE TCP STREAM, SO DROP RATHER THAN WAIT
                    audio_tx.try_send(AudioFrame { data }).ok();
                },

                //ONLY A SHARER IS REPORTED TO
                ScreenPacketCode::Feedback { .. } => {},
            }
        }
    });

    //A CLIENT DRAWING THE FRAMES GETS NO WINDOW
    if SCREEN_FRAME_SINK.read().unwrap().is_some() { return }

    if let Some(proxy) = SCREEN_SHARE_PROXY.read().unwrap().as_ref()
    {
        proxy.send_event(UserEvent::NewSession(ScreenShareRequest
        {
            rx,
            running,
            reports,
            deattach: deattach_tx,
        })).ok();
    }
}
