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

//! # Attachment Media Player
//!
//! Embedded audio and video attachment playback. Audio uses the voice-message
//! output device, resampler and volume. Decoders run away from the UI and audio callbacks:
//! only four audio packets and one pending video frame may wait for their consumers.
//! The encoded attachment is shared by both demuxers for seeking; decoded movies are never
//! accumulated in memory.

//MODULES
mod video;

use std::
{
    io::{ Cursor, ErrorKind },
    path::Path,
    thread,
    time::Instant,
    sync::
    {
        Arc,
        Mutex,
        mpsc::{ self, SyncSender, TrySendError },
        atomic::{ AtomicBool, Ordering },
    },
};

use tokio::{ task, sync::mpsc::Sender };

use symphonia::
{
    default,
    core::
    {
        errors::Error,
        io::MediaSourceStream,
        meta::MetadataOptions,
        packet::Packet,
        audio::{ Channels, Position },
        formats::
        {
            FormatReader,
            FormatOptions,
            probe::Hint,
        },
        codecs::audio::
        {
            well_known,
            AudioDecoder,
            AudioDecoderOptions,
        },
    },
};

use audiopus::
{
    Channels as OpusChannels,
    SampleRate,
    coder::Decoder as OpusDecoder,
};

use crate::
{
    cache,
    crypto,
    consts,
    i18n,
    network::
    {
        voice::client::message,
        client::
        {
            ClientEvent,
            image::{ Animation, ImageFrame },
        },
    },
};

use video::Video;

//STRUCTS
/// The shared lifetime and clocks of one media playback.
struct Session
{
    hash: [u8; 32],
    generation: usize,
    started: Mutex<Instant>,
    audio_started: AtomicBool,
    audio_done: AtomicBool,
    video_done: AtomicBool,
    frame_pending: AtomicBool,
}

/// A demuxer and decoder positioned at the start of an audio track.
struct Audio
{
    format: Box<dyn FormatReader>,
    decoder: AudioCodec,
    track: u32,
    rate: u32,
}

/// Audio decoders share the same PCM queue and output stream.
enum AudioCodec
{
    Symphonia(Box<dyn AudioDecoder>),
    Opus { decoder: OpusDecoder, channels: usize, skip: usize },
}

struct AudioPacket
{
    samples: Vec<f32>,
    channels: Channels,
    rate: u32,
}

//GLOBAL VARIABLES
static SESSION: Mutex<Option<Arc<Session>>> = Mutex::new(None);
static AWAITED: Mutex<Option<([u8; 32], usize)>> = Mutex::new(None);

//IMPLEMENTATIONS
impl Session
{
    /// Follows audio samples when an output exists, otherwise the monotonic video clock.
    fn elapsed(&self) -> u32
    {
        message::playing().filter(|(hash, _)| *hash == self.hash)
            .map(|(_, millis)| millis)
            .unwrap_or_else(|| self.started.lock().unwrap().elapsed().as_millis().min(u32::MAX as u128) as u32)
    }
}

impl AudioCodec
{
    fn decode(&mut self, packet: &Packet) -> Result<AudioPacket, &'static str>
    {
        match self
        {
            Self::Symphonia(decoder) =>
            {
                let decoded = decoder.decode(packet).map_err(|_| "media.failed")?;
                let spec = decoded.spec();

                if decoded.frames() > consts::MAX_MEDIA_AUDIO_FRAMES || spec.channels().count() > 8
                {
                    return Err("media.unsupported");
                }

                let channels = spec.channels().clone();
                let rate = spec.rate();
                let mut samples = vec![0.; decoded.samples_interleaved()];
                decoded.copy_to_slice_interleaved(&mut samples);

                Ok(AudioPacket { samples, channels, rate })
            },

            Self::Opus { decoder, channels, skip } =>
            {
                //OPUS PACKETS ARE AT MOST 120 MS AT THE 48 KHZ DECODE RATE
                let mut samples = vec![0.; 5760 * *channels];
                let frames = decoder.decode_float(Some(packet.data.as_ref()), &mut samples, false)
                    .map_err(|_| "media.failed")?;
                samples.truncate(frames * *channels);

                let start = (*skip).max(packet.trim_start.get() as usize).min(frames);
                *skip = skip.saturating_sub(start);
                let end = frames.saturating_sub(packet.trim_end.get() as usize).max(start);
                let samples = samples[start * *channels..end * *channels].to_vec();
                let channels = match *channels
                {
                    1 => Channels::Positioned(Position::FRONT_CENTER),
                    _ => Channels::Positioned(Position::FRONT_LEFT | Position::FRONT_RIGHT),
                };

                Ok(AudioPacket { samples, channels, rate: 48000 })
            },
        }
    }
}

impl Audio
{
    /// Selects a supported audio track; Opus reuses the voice-message decoder library.
    fn open(data: Arc<[u8]>) -> Option<Self>
    {
        let source = MediaSourceStream::new(Box::new(Cursor::new(data)), Default::default());
        let format = default::get_probe().probe(&Hint::new(), source,
            FormatOptions::default(), MetadataOptions::default()).ok()?;

        for track in format.tracks()
        {
            let Some(params) = track.codec_params.as_ref().and_then(|params| params.audio()) else { continue };
            let Some(mut rate) = params.sample_rate.filter(|rate| (8000..=192000).contains(rate)) else { continue };

            let decoder = if params.codec == well_known::CODEC_ID_OPUS
            {
                let channels = params.channels.as_ref()?.count();
                let mode = match channels
                {
                    1 => OpusChannels::Mono,
                    2 => OpusChannels::Stereo,
                    _ => continue,
                };
                let decoder = OpusDecoder::new(SampleRate::Hz48000, mode).ok()?;
                let skip = track.delay.unwrap_or_default() as usize;
                rate = 48000;

                AudioCodec::Opus { decoder, channels, skip }
            } else
            {
                let Ok(decoder) = default::get_codecs().make_audio_decoder(params, &AudioDecoderOptions::default()) else { continue };
                AudioCodec::Symphonia(decoder)
            };
            let id = track.id;

            return Some(Self { format, decoder, track: id, rate });
        }

        None
    }

    /// Decodes packets into a bounded stereo queue; stop cancels producer backpressure.
    fn run(mut self, session: &Session, sender: SyncSender<Vec<f32>>) -> Result<(), &'static str>
    {
        while message::current(session.generation)
        {
            let packet = match self.format.next_packet()
            {
                Ok(Some(packet)) => packet,
                Ok(None) => return Ok(()),
                Err(Error::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => return Ok(()),
                Err(_) => return Err("media.failed"),
            };

            if packet.track_id != self.track { continue; }

            let decoded = self.decoder.decode(&packet)?;
            let channels = decoded.channels.count();

            if channels == 0 || channels > 8 || decoded.rate != self.rate
                || decoded.samples.len() / channels > consts::MAX_MEDIA_AUDIO_FRAMES
            {
                return Err("media.unsupported");
            }

            //MAP POSITIONED CHANNELS TO STEREO, INCLUDING CENTRE DIALOGUE
            let mut weights = vec![(0., 0.); channels];
            for (position, left, right) in
            [
                (Position::FRONT_LEFT, 1., 0.), (Position::FRONT_RIGHT, 0., 1.),
                (Position::FRONT_CENTER, 0.707, 0.707),
                (Position::REAR_LEFT, 0.5, 0.), (Position::SIDE_LEFT, 0.5, 0.),
                (Position::REAR_RIGHT, 0., 0.5), (Position::SIDE_RIGHT, 0., 0.5),
            ]
            {
                if !matches!(&decoded.channels, Channels::Positioned(positions) if positions.contains(position)) { continue; }

                if let Some(index) = decoded.channels.get_canonical_index_for_positioned_channel(position)
                {
                    weights[index] = (left, right);
                }
            }

            if channels == 1 { weights[0] = (1., 1.); }
            else if weights.iter().all(|weight| *weight == (0., 0.))
            {
                weights[0] = (1., 0.);
                weights[1] = (0., 1.);
            }

            let mut stereo = Vec::with_capacity(decoded.samples.len() / channels * 2);
            for frame in decoded.samples.chunks_exact(channels)
            {
                let mut left = 0.;
                let mut right = 0.;

                for (sample, (left_gain, right_gain)) in frame.iter().zip(&weights)
                {
                    let sample = if sample.is_finite() { *sample } else { 0. };
                    left += sample * left_gain;
                    right += sample * right_gain;
                }

                stereo.extend_from_slice(&[left, right]);
            }

            //THE REALTIME CALLBACK NEVER WAITS FOR THIS WORKER
            loop
            {
                if !message::current(session.generation) { return Ok(()); }

                match sender.try_send(stereo)
                {
                    Ok(()) => break,
                    Err(TrySendError::Disconnected(_)) => return Ok(()),
                    Err(TrySendError::Full(packet)) => stereo = packet,
                }

                thread::sleep(consts::MEDIA_POLL_DELAY);
            }
        }

        Ok(())
    }
}

//FUNCTIONS
//PRIVATE
/// Keeps at most one converted frame in the UI queue and drops late frames.
fn present(frame: ImageFrame, time: u64, session: &Session, tx: &Sender<ClientEvent>) -> Result<(), &'static str>
{
    loop
    {
        if !message::current(session.generation) { return Ok(()); }

        let waiting = !session.audio_started.load(Ordering::Relaxed) && !session.audio_done.load(Ordering::Relaxed);
        if !waiting && session.elapsed() as u64 >= time { break; }

        thread::sleep(consts::MEDIA_POLL_DELAY);
    }

    if session.elapsed() as u64 > time.saturating_add(consts::MEDIA_FRAME_LATENESS) || session.frame_pending.swap(true, Ordering::Relaxed) { return Ok(()); }

    if tx.try_send(ClientEvent::MediaFrame
    {
        hash: session.hash,
        generation: session.generation,
        frame,
    }).is_err()
    {
        session.frame_pending.store(false, Ordering::Relaxed);
    }

    Ok(())
}

/// Reports failures only while this decoder still owns playback.
fn failed(generation: usize, key: &'static str, tx: &Sender<ClientEvent>)
{
    if message::current(generation)
    {
        tx.blocking_send(ClientEvent::MediaFailed { reason: i18n::text(key).to_owned() }).ok();
    }
}

//PUBLIC
//DECODE A STILL VIDEO PREVIEW WITHOUT STARTING PLAYBACK
/// Returns one still frame of a supported video attachment.
pub fn thumbnail(data: &[u8]) -> Option<Animation>
{
    if data.len() > consts::MAX_ATTACHMENT_SIZE || !video::recognizes(data) { return None; }

    let data = video::prepare(Arc::from(data)).ok()?;
    let video = Video::open(data, false).ok()??;

    Some(vec![video.thumbnail()?])
}

//MEDIA TYPES OFFERED A PLAY BUTTON
/// Returns whether an attachment name suggests one of the supported media containers.
pub fn supported(filename: &str) -> bool
{
    Path::new(filename).extension().and_then(|extension| extension.to_str()).is_some_and(|extension|
        matches!(extension.to_ascii_lowercase().as_str(), "mp3" | "wav" | "flac" | "ogg" | "oga" | "aac" | "m4a" | "mp4" | "m4v" | "mov" | "mkv" | "webm" | "opus"))
}

//START FROM THE CACHE, OTHERWISE REQUEST THE ATTACHMENT
pub fn play(hash: [u8; 32], tx: Sender<ClientEvent>)
{
    let generation = message::begin_playback();
    *SESSION.lock().unwrap() = None;
    *AWAITED.lock().unwrap() = None;

    tokio::spawn(async move
    {
        match cache::load(&hash).await
        {
            Some(data) => play_data(hash, data, generation, tx).await,
            None if message::current(generation) =>
            {
                {
                    let mut awaited = AWAITED.lock().unwrap();
                    if !message::current(generation) { return; }
                    *awaited = Some((hash, generation));
                }

                tx.send(ClientEvent::AttachmentRequest { hash }).await.ok();
            },
            None => {},
        }
    });
}

/// Takes a requested media attachment off the pending slot.
pub fn awaited(hash: &[u8; 32]) -> Option<usize>
{
    let mut awaited = AWAITED.lock().unwrap();
    if awaited.as_ref().is_some_and(|(requested, _)| requested == hash)
    {
        return awaited.take().map(|(_, generation)| generation).filter(|generation| message::current(*generation));
    }

    None
}

//START THE EMBEDDED DECODERS
pub async fn play_data(hash: [u8; 32], data: Vec<u8>, generation: usize, tx: Sender<ClientEvent>)
{
    if !message::current(generation) { return; }

    tx.send(ClientEvent::MediaFetched { hash }).await.ok();

    task::spawn_blocking(move ||
    {
        if !message::current(generation) { return; }

        if data.len() > consts::MAX_ATTACHMENT_SIZE || data.is_empty() || crypto::sha256(&data) != hash
        {
            failed(generation, "media.failed", &tx);
            return;
        }

        let data = match video::prepare(data.into())
        {
            Ok(data) => data,
            Err(key) => { failed(generation, key, &tx); return; },
        };
        let audio = Audio::open(data.clone());
        let video = match Video::open(data, true)
        {
            Ok(video) => video,
            Err(key) =>
            {
                failed(generation, key, &tx);
                return;
            },
        };

        if audio.is_none() && video.is_none()
        {
            failed(generation, "media.unsupported", &tx);
            return;
        }

        let session = Arc::new(Session
        {
            hash,
            generation,
            started: Mutex::new(Instant::now()),
            audio_started: AtomicBool::new(false),
            audio_done: AtomicBool::new(audio.is_none()),
            video_done: AtomicBool::new(video.is_none()),
            frame_pending: AtomicBool::new(false),
        });

        //PUBLISH UNDER THE LOCK SO AN OLDER START CANNOT REPLACE A NEW ONE
        {
            let mut active = SESSION.lock().unwrap();
            if !message::current(generation) { return; }
            *active = Some(session.clone());
        }

        //AUDIO AND VIDEO CAN BLOCK INDEPENDENTLY WITHOUT BLOCKING THE UI
        if let Some(audio) = audio
        {
            let session = session.clone();
            let tx = tx.clone();

            task::spawn_blocking(move ||
            {
                let (sender, receiver) = mpsc::sync_channel(consts::MEDIA_AUDIO_QUEUE);
                let result = message::open_pcm(hash, generation, audio.rate, receiver)
                    .map_err(|_| "media.output_failed")
                    .and_then(|()|
                    {
                        *session.started.lock().unwrap() = Instant::now();
                        session.audio_started.store(true, Ordering::Relaxed);
                        audio.run(&session, sender)
                    });

                if !session.audio_started.load(Ordering::Relaxed)
                {
                    *session.started.lock().unwrap() = Instant::now();
                }

                session.audio_done.store(true, Ordering::Relaxed);
                if let Err(key) = result { failed(session.generation, key, &tx); }
            });
        }

        if let Some(video) = video
        {
            task::spawn_blocking(move ||
            {
                let result = video.run(&session, &tx);
                session.video_done.store(true, Ordering::Relaxed);
                if let Err(key) = result { failed(session.generation, key, &tx); }
            });
        }
    }).await.ok();
}

//FOLLOW PLAYBACK WITHOUT RETAINING COMPLETED DECODERS
pub fn playing() -> Option<([u8; 32], u32)>
{
    let mut guard = SESSION.lock().unwrap();
    let Some(session) = guard.as_ref() else
    {
        drop(guard);

        //THE CAPTION CAN STOP A PLAY THAT IS STILL WAITING FOR THE SERVER
        let mut awaited = AWAITED.lock().unwrap();
        let (hash, generation) = (*awaited)?;

        if !message::current(generation)
        {
            *awaited = None;
            return None;
        }

        return Some((hash, 0));
    };

    if !message::current(session.generation)
        || (session.video_done.load(Ordering::Relaxed) && session.audio_done.load(Ordering::Relaxed)
            && message::playing().is_none())
    {
        *guard = None;
        return None;
    }

    Some((session.hash, session.elapsed()))
}

/// Returns whether a queued UI frame still belongs to the shared player.
pub fn current(generation: usize) -> bool
{
    message::current(generation)
}

/// Releases the UI frame slot; stale frames must not affect a later playback.
pub fn frame_consumed(generation: usize)
{
    if let Some(session) = SESSION.lock().unwrap().as_ref().filter(|session| session.generation == generation)
    {
        session.frame_pending.store(false, Ordering::Relaxed);
    }
}
