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
//! Embedded audio decoding and MP4/H.264 video playback. Audio uses the voice-message
//! output device, resampler and volume. Decoders run away from the UI and audio callbacks:
//! only four audio packets and one pending video frame may wait for their consumers.
//! The encoded attachment is shared by both demuxers for seeking; decoded movies are never
//! accumulated in memory.

use std::
{
    io::{ Cursor, ErrorKind },
    path::Path,
    thread,
    time::{ Duration, Instant },
    sync::
    {
        Arc,
        Mutex,
        mpsc::{ self, SyncSender, TrySendError },
        atomic::{ AtomicBool, Ordering },
    },
};

use tokio::{ task, sync::mpsc::Sender };

use image::{ DynamicImage, RgbImage };

use mp4::{ Mp4Reader, MediaType, TrackType };

use openh264::
{
    decoder::{ Decoder as VideoDecoder, DecodeOptions, DecodedYUV, Flush },
    formats::YUVSource,
};

use symphonia::
{
    default,
    core::
    {
        errors::Error,
        probe::Hint,
        io::MediaSourceStream,
        meta::MetadataOptions,
        audio::{ Channels, SampleBuffer },
        formats::{ FormatReader, FormatOptions },
        codecs::
        {
            Decoder as AudioDecoder,
            DecoderOptions,
        },
    },
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
    decoder: Box<dyn AudioDecoder>,
    track: u32,
    rate: u32,
}

/// One MP4 video track and its presentation timestamps, with no predecoded frames.
struct Video
{
    reader: Mp4Reader<Cursor<Arc<[u8]>>>,
    decoder: VideoDecoder,
    track: u32,
    length_size: usize,
    count: u32,
    times: Vec<u64>,
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

impl Audio
{
    /// Selects the first supported audio track without decoding the entire file.
    fn open(data: Arc<[u8]>) -> Option<Self>
    {
        let source = MediaSourceStream::new(Box::new(Cursor::new(data)), Default::default());
        let format = default::get_probe().format(&Hint::new(), source,
            &FormatOptions::default(), &MetadataOptions::default()).ok()?.format;

        for track in format.tracks()
        {
            let Some(rate) = track.codec_params.sample_rate.filter(|rate| (8000..=192000).contains(rate)) else { continue };
            let Ok(decoder) = default::get_codecs().make(&track.codec_params, &DecoderOptions::default()) else { continue };
            let id = track.id;

            return Some(Self { format, decoder, track: id, rate });
        }

        None
    }

    /// Decodes packets into a bounded stereo queue; a new play or stop cancels backpressure.
    fn run(mut self, session: &Session, sender: SyncSender<Vec<f32>>) -> Result<(), &'static str>
    {
        while message::current(session.generation)
        {
            let packet = match self.format.next_packet()
            {
                Ok(packet) => packet,
                Err(Error::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => return Ok(()),
                Err(_) => return Err("media.failed"),
            };

            if packet.track_id() != self.track { continue; }

            let decoded = self.decoder.decode(&packet).map_err(|_| "media.failed")?;
            let spec = *decoded.spec();
            let channels = spec.channels.count();

            if channels == 0 || channels > 8 || spec.rate != self.rate || decoded.capacity() > consts::MAX_MEDIA_AUDIO_FRAMES
            {
                return Err("media.unsupported");
            }

            let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
            samples.copy_interleaved_ref(decoded);

            let mut stereo = Vec::with_capacity(samples.len() / channels * 2);

            for frame in samples.samples().chunks_exact(channels)
            {
                if channels == 1
                {
                    stereo.extend_from_slice(&[frame[0], frame[0]]);
                    continue;
                }

                let mut left = 0.;
                let mut right = 0.;

                for (sample, channel) in frame.iter().zip(spec.channels.iter())
                {
                    match channel
                    {
                        Channels::FRONT_LEFT => left += sample,
                        Channels::FRONT_RIGHT => right += sample,
                        Channels::REAR_LEFT | Channels::SIDE_LEFT => left += sample * 0.5,
                        Channels::REAR_RIGHT | Channels::SIDE_RIGHT => right += sample * 0.5,
                        Channels::LFE1 | Channels::LFE2 => {},
                        _ => { left += sample * 0.707; right += sample * 0.707; },
                    }
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

impl Video
{
    /// Reads AVC configuration, with presentation timing only when playback needs it.
    fn open(data: Arc<[u8]>, timed: bool) -> Result<Option<Self>, &'static str>
    {
        let size = data.len() as u64;
        let Ok(mut reader) = Mp4Reader::read_header(Cursor::new(data), size) else { return Ok(None) };
        let Some(track) = reader.tracks().values().find(|track| track.track_type().ok() == Some(TrackType::Video))
            else { return Ok(None) };

        if track.media_type().ok() != Some(MediaType::H264) || track.width() == 0 || track.height() == 0
            || track.width() as usize * track.height() as usize > consts::MAX_MEDIA_PIXELS
            || track.sample_count() > consts::MAX_MEDIA_SAMPLES || track.timescale() == 0
        {
            return Err("media.unsupported");
        }

        let avc = &track.trak.mdia.minf.stbl.stsd.avc1.as_ref().ok_or("media.unsupported")?.avcc;
        let length_size = (avc.length_size_minus_one & 3) as usize + 1;
        let id = track.track_id();
        let count = track.sample_count();
        let scale = track.timescale() as u64;
        let mut decoder = VideoDecoder::new().map_err(|_| "media.failed")?;
        let mut configuration = Vec::new();

        for parameter in [track.sequence_parameter_set(), track.picture_parameter_set()]
        {
            configuration.extend_from_slice(&[0, 0, 0, 1]);
            configuration.extend_from_slice(parameter.map_err(|_| "media.unsupported")?);
        }

        decoder.decode(&configuration).map_err(|_| "media.failed")?;

        //DECODE ORDER AND PRESENTATION ORDER DIFFER FOR B-FRAMES
        let mut times = Vec::new();

        if timed
        {
            times.reserve(count as usize);

            for sample in 1..=count
            {
                let sample = reader.read_sample(id, sample).map_err(|_| "media.failed")?.ok_or("media.failed")?;
                let time = (sample.start_time as i128 + sample.rendering_offset as i128).max(0) as u64;
                times.push(time.saturating_mul(1000) / scale);
            }

            times.sort_unstable();
            let first = times.first().copied().unwrap_or_default();
            for time in &mut times { *time -= first; }
        }

        Ok(Some(Self { reader, decoder, track: id, length_size, count, times }))
    }

    /// Converts one length-prefixed MP4 sample to Annex B without decoding other samples.
    fn packet(&mut self, index: u32) -> Result<Vec<u8>, &'static str>
    {
        let sample = self.reader.read_sample(self.track, index).map_err(|_| "media.failed")?.ok_or("media.failed")?;
        let mut bytes = sample.bytes.as_ref();
        let mut annex = Vec::with_capacity(bytes.len());

        //MP4 PREFIXES NALS WITH LENGTHS; OPENH264 EXPECTS ANNEX B START CODES
        while !bytes.is_empty()
        {
            let prefix = bytes.get(..self.length_size).ok_or("media.failed")?;
            let size = prefix.iter().fold(0usize, |size, byte| (size << 8) | *byte as usize);
            bytes = &bytes[self.length_size..];
            let nal = bytes.get(..size).ok_or("media.failed")?;
            annex.extend_from_slice(&[0, 0, 0, 1]);
            annex.extend_from_slice(nal);
            bytes = &bytes[size..];
        }

        Ok(annex)
    }

    /// Decodes only enough samples for a poster frame; no clock or audio stream is started.
    fn thumbnail(mut self) -> Option<ImageFrame>
    {
        for index in 1..=self.count.min(consts::MEDIA_THUMBNAIL_SAMPLES)
        {
            let packet = self.packet(index).ok()?;

            if let Some(frame) = self.decoder.decode_with_options(&packet,
                DecodeOptions::new().flush_after_decode(Flush::NoFlush)).ok()?
            {
                return preview(frame).ok();
            }
        }

        self.decoder.flush_remaining().ok()?.into_iter().next().and_then(|frame| preview(frame).ok())
    }

    /// Decodes one sample at a time, presenting against the audio clock when available.
    fn run(mut self, session: &Session, tx: &Sender<ClientEvent>) -> Result<(), &'static str>
    {
        let mut shown = 0;

        for index in 1..=self.count
        {
            if !message::current(session.generation) { return Ok(()); }

            let annex = self.packet(index)?;

            if let Some(frame) = self.decoder.decode_with_options(&annex,
                DecodeOptions::new().flush_after_decode(Flush::NoFlush)).map_err(|_| "media.failed")?
            {
                present(frame, self.times.get(shown).copied().unwrap_or_default(), session, tx)?;
                shown += 1;
            }
        }

        //DRAIN DELAYED FRAMES WITHOUT KEEPING THE MOVIE IN MEMORY
        for frame in self.decoder.flush_remaining().map_err(|_| "media.failed")?
        {
            present(frame, self.times.get(shown).copied().unwrap_or_default(), session, tx)?;
            shown += 1;
        }

        Ok(())
    }
}

//FUNCTIONS
//PRIVATE
/// Converts one decoded frame to a bounded still image shared by previews and playback.
fn preview(frame: DecodedYUV<'_>) -> Result<ImageFrame, &'static str>
{
    let (width, height) = frame.dimensions();
    if width == 0 || height == 0 || width * height > consts::MAX_MEDIA_PIXELS { return Err("media.unsupported"); }

    let mut rgb = vec![0; width * height * 3];
    frame.write_rgb8(&mut rgb);
    let image = RgbImage::from_raw(width as u32, height as u32, rgb).ok_or("media.failed")?;
    let image = DynamicImage::ImageRgb8(image);
    let image = match width as u32 > consts::MEDIA_PREVIEW_SIZE.0 || height as u32 > consts::MEDIA_PREVIEW_SIZE.1
    {
        true => image.thumbnail(consts::MEDIA_PREVIEW_SIZE.0, consts::MEDIA_PREVIEW_SIZE.1),
        false => image,
    };

    Ok(ImageFrame { image, delay: Duration::ZERO })
}

/// Keeps at most one converted frame in the UI queue and drops late frames.
fn present(frame: DecodedYUV<'_>, time: u64, session: &Session, tx: &Sender<ClientEvent>) -> Result<(), &'static str>
{
    loop
    {
        if !message::current(session.generation) { return Ok(()); }

        let waiting = !session.audio_started.load(Ordering::Relaxed) && !session.audio_done.load(Ordering::Relaxed);
        if !waiting && session.elapsed() as u64 >= time { break; }

        thread::sleep(consts::MEDIA_POLL_DELAY);
    }

    if session.elapsed() as u64 > time.saturating_add(consts::MEDIA_FRAME_LATENESS) || session.frame_pending.swap(true, Ordering::Relaxed) { return Ok(()); }

    let frame = preview(frame)?;

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
/// Returns one frame of a supported MP4 attachment, or `None` for other files.
pub fn thumbnail(data: &[u8]) -> Option<Animation>
{
    if data.len() > consts::MAX_ATTACHMENT_SIZE || data.get(4..8) != Some(b"ftyp") { return None; }

    let video = Video::open(Arc::from(data), false).ok()??;

    Some(vec![video.thumbnail()?])
}

//MEDIA TYPES OFFERED A PLAY BUTTON
/// Returns whether an attachment name suggests one of the supported media containers.
pub fn supported(filename: &str) -> bool
{
    Path::new(filename).extension().and_then(|extension| extension.to_str()).is_some_and(|extension|
        matches!(extension.to_ascii_lowercase().as_str(), "mp3" | "wav" | "flac" | "ogg" | "oga" | "aac" | "m4a" | "mp4" | "m4v"))
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

        let data: Arc<[u8]> = data.into();
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
