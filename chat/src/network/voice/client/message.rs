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

//! # Voice Messages — Recording and Playback
//!
//! Records voice messages into a [`Clip`] and plays them back.
//!
//! # Recording
//! A recording is 48 kHz stereo Opus at [`MESSAGE_BITRATE`](consts::MESSAGE_BITRATE), with none
//! of the voice chat's processing (no denoiser, gate or AGC) — a message may be music or a room,
//! which that processing is built to throw away. Only the microphone volume applies.
//!
//! While in a call, the recording is fed from the call's own capture callback ([`feed`]), since
//! a raw ALSA PCM is exclusive and a second capture would fail. Otherwise it opens the configured
//! input device itself. Frames are encoded inside the capture callback, which takes the lock with
//! `try_lock` only, so contention costs one buffer, never a blocked realtime thread. The
//! recording stops itself at [`MESSAGE_MAX_FRAMES`](consts::MESSAGE_MAX_FRAMES) or the size
//! ceiling, and anything shorter than [`MESSAGE_MIN_FRAMES`](consts::MESSAGE_MIN_FRAMES) is not
//! sent.
//!
//! # Playback
//! One clip plays at a time, on its own output stream, decoded one packet at a time in the
//! output callback, so even a five-minute clip is a few kilobytes of state. A clip missing from
//! the cache is requested from the server and started when it arrives ([`awaited`]); a play
//! generation makes a later play or a stop win over a start still in flight.

use std::sync::
{
    Arc,
    Mutex,
    atomic::{ AtomicBool, AtomicUsize, Ordering },
};

use tokio::
{
    task,
    sync::mpsc::Sender,
};

use cpal::
{
    Stream,
    traits::
    {
        DeviceTrait,
        StreamTrait,
    },
};

use audiopus::
{
    Bitrate,
    Channels,
    SampleRate,
    Application,
    TryFrom,
    coder::{ Encoder, Decoder },
};

use gag::Gag;

use crate::
{
    t,
    i18n,
    cache,
    config,
    consts as chat_consts,
    network::
    {
        client::ClientEvent,
        voice::
        {
            consts,
            message::Clip,
            client::options,
        },
    },
};

//STRUCTS
/// A voice message being recorded.
struct Recording //A VOICE MESSAGE BEING RECORDED
{
    /// The stereo Opus encoder.
    encoder: Encoder,
    /// Opus packets so far.
    frames: Vec<Vec<u8>>, //OPUS PACKETS SO FAR
    /// What the packets will take on the wire.
    size: usize,          //WHAT THEY WILL TAKE ON THE WIRE
    /// Resampled stereo audio not yet a whole frame.
    pending: Vec<f32>,    //RESAMPLED STEREO, NOT A WHOLE FRAME YET
    /// Fractional resampler position.
    position: f32,        //RESAMPLER POSITION
    /// Whether the recording is fed by the call's capture.
    voice: bool,          //FED BY THE VOICE CALL'S CAPTURE
    /// Whether a frame or size limit was hit.
    full: bool,           //A LIMIT WAS HIT
    /// The recording's generation, so a capture opened late can tell it is stale.
    generation: usize,
}

/// A voice message being played.
struct Player //A VOICE MESSAGE BEING PLAYED
{
    /// The clip's hash.
    hash: [u8; 32],
    /// How far playback has got.
    progress: Arc<Progress>,
    /// The output stream; dropping it stops playback.
    _stream: Stream,
}

/// How far a playback has got, shared with its output callback.
struct Progress //WHERE THE PLAYBACK IS
{
    /// Frames decoded.
    frames: AtomicUsize, //FRAMES DECODED
    /// Whether the clip ran out.
    done: AtomicBool,    //RAN OUT OF FRAMES
}

/// Decodes a clip packet by packet as the output callback asks.
struct Playhead //DECODES A CLIP AS THE DEVICE ASKS
{
    /// The clip's Opus packets.
    frames: Vec<Vec<u8>>,
    /// The clip's channel count.
    channels: usize,
    /// The Opus decoder.
    decoder: Decoder,
    /// Next packet to decode.
    next: usize,      //NEXT PACKET
    /// Decoded, interleaved samples.
    buffer: Vec<f32>, //DECODED, INTERLEAVED
    /// Read position in the buffer.
    read: usize,      //READ POSITION IN buffer
    /// How far playback has got.
    progress: Arc<Progress>,
}

//GLOBAL VARIABLES
/// The recording in progress.
static RECORDING: Mutex<Option<Recording>> = Mutex::new(None);
/// The capture opened for a recording made outside a call.
static OWN_INPUT: Mutex<Option<Stream>> = Mutex::new(None); //OUR OWN CAPTURE
/// Generation of the latest recording.
static RECORD_GENERATION: AtomicUsize = AtomicUsize::new(0);

/// The clip being played.
static PLAYER: Mutex<Option<Player>> = Mutex::new(None);
/// Generation of the latest play or stop.
static PLAY_GENERATION: AtomicUsize = AtomicUsize::new(0);
/// Clips requested from the server for playback, with the generation that asked.
static AWAITED: Mutex<Vec<([u8; 32], usize)>> = Mutex::new(Vec::new()); //CLIPS ASKED OF THE SERVER TO PLAY

//IMPLEMENTATIONS
impl Recording
{
    /// Resamples captured audio to 48 kHz stereo and encodes every whole frame.
    fn feed(&mut self, data: &[f32], channels: usize, rate: f32) //RESAMPLE AND ENCODE
    {
        if channels == 0 { return; }

        let frames = data.len() / channels;
        let step = rate / consts::SAMPLE_RATE as f32;
        let gain = options::get_input_gain();

        let sample = |index: usize| -> (f32, f32)
        {
            if index >= frames { return (0., 0.); }

            match channels
            {
                1 => (data[index], data[index]),
                _ => (data[index * channels], data[index * channels + 1]),
            }
        };

        while self.position < frames as f32 - 1.
        {
            let index = self.position.floor() as usize;
            let fraction = self.position - index as f32;

            let (l0, r0) = sample(index);
            let (l1, r1) = sample(index + 1);

            self.pending.push(((l0 + (l1 - l0) * fraction) * gain).clamp(-1., 1.));
            self.pending.push(((r0 + (r1 - r0) * fraction) * gain).clamp(-1., 1.));

            self.position += step;
        }

        self.position -= frames as f32;

        while !self.full && self.pending.len() >= consts::FRAME_SIZE * 2
        {
            let frame: Vec<f32> = self.pending.drain(..consts::FRAME_SIZE * 2).collect();
            self.encode(&frame);
        }
    }

    /// Encodes one stereo frame, marking the recording full at a frame or size limit.
    fn encode(&mut self, frame: &[f32]) //ENCODE ONE FRAME
    {
        let mut buffer = [0u8; consts::MESSAGE_MAX_PACKET];

        if let Ok(len) = self.encoder.encode_float(frame, &mut buffer)
        {
            self.frames.push(buffer[..len].to_vec());
            self.size += len + size_of::<u64>();
        }

        self.full = self.frames.len() >= consts::MESSAGE_MAX_FRAMES
            || self.size + consts::MESSAGE_MAX_PACKET + size_of::<u64>() > chat_consts::MAX_VOICE_MESSAGE_SIZE;
    }
}

impl Playhead
{
    /// Returns the next stereo sample, decoding the next packet when the buffer runs out.
    ///
    /// # Returns
    /// `None` once the clip has ended.
    fn next(&mut self) -> Option<(f32, f32)> //NEXT STEREO SAMPLE
    {
        while self.read >= self.buffer.len()
        {
            let packet = self.frames.get(self.next)?;
            self.next += 1;

            self.buffer.resize(consts::FRAME_SIZE * self.channels, 0.);

            //DECODE (BAD PACKET = SILENCE)
            let decoded = self.decoder.decode_float(Some(&packet[..]), &mut self.buffer[..], false).unwrap_or(0);

            self.buffer.truncate(decoded * self.channels);
            self.read = 0;

            self.progress.frames.store(self.next, Ordering::Relaxed);
        }

        let left = self.buffer[self.read];
        let right = if self.channels == 2 { self.buffer[self.read + 1] } else { left };

        self.read += self.channels;

        Some((left, right))
    }
}

//FUNCTIONS
//PRIVATE
/// Creates the recording's stereo encoder.
fn stereo() -> Option<Encoder> //THE RECORDING'S ENCODER
{
    let mut encoder = Encoder::new
    (
        <SampleRate as TryFrom<i32>>::try_from(consts::SAMPLE_RATE as i32).ok()?,
        Channels::Stereo,
        Application::Audio,
    ).ok()?;

    encoder.set_bitrate(Bitrate::BitsPerSecond(consts::MESSAGE_BITRATE)).ok()?;
    encoder.set_vbr_constraint(true).ok()?;

    Some(encoder)
}

/// Opens a capture of our own for a recording made outside a call. Blocking.
///
/// The stream is kept only if the recording that asked for it is still the current one.
///
/// # Returns
/// `true` if the capture opened.
fn open_input(generation: usize) -> bool //CAPTURE OF OUR OWN (BLOCKING)
{
    let _stderr_gag = Gag::stderr().ok();

    let Some(device) = super::pick_device(&config::read_config::<String>("input_device"), true) else { return false };
    let (Ok(supported), Ok(default)) = (device.supported_input_configs(), device.default_input_config()) else { return false };

    let config = super::configure_device(&device, supported, default, true);
    let channels = config.channels as usize;
    let rate = config.sample_rate as f32;

    let Ok(stream) = device.build_input_stream(config, move |data: &[f32], _: &_| feed(data, channels, rate, false), |_| {}, None) else { return false };

    if stream.play().is_err() { return false; }

    //KEEP IT IF STILL CURRENT
    let mut own = OWN_INPUT.lock().unwrap();
    let current = RECORDING.lock().unwrap().as_ref().is_some_and(|recording| recording.generation == generation);

    if current { *own = Some(stream); }

    true
}

/// Opens an output stream and starts playing a clip. Blocking.
///
/// # Returns
/// `Err` with the translation key of the failure.
fn open_output(hash: [u8; 32], data: &[u8], generation: usize) -> Result<(), &'static str> //START PLAYING (BLOCKING)
{
    let clip = Clip::decode(data).ok_or("voice_message.unreadable")?;

    let _stderr_gag = Gag::stderr().ok();

    let device = super::pick_device(&config::read_config::<String>("output_device"), false).ok_or("voice_message.play_failed")?;
    let (Ok(supported), Ok(default)) = (device.supported_output_configs(), device.default_output_config()) else { return Err("voice_message.play_failed") };

    let config = super::configure_device(&device, supported, default, false);

    let channels = if clip.channels == 1 { Channels::Mono } else { Channels::Stereo };
    let decoder = Decoder::new(<SampleRate as TryFrom<i32>>::try_from(consts::SAMPLE_RATE as i32).unwrap(), channels)
        .map_err(|_| "voice_message.play_failed")?;

    let progress = Arc::new(Progress { frames: AtomicUsize::new(0), done: AtomicBool::new(false) });

    let mut playhead = Playhead
    {
        frames: clip.frames,
        channels: clip.channels as usize,
        decoder,
        next: 0,
        buffer: Vec::with_capacity(consts::FRAME_SIZE * 2),
        read: 0,
        progress: progress.clone(),
    };

    //OUTPUT RESAMPLING
    let output_channels = config.channels as usize;
    let step = consts::SAMPLE_RATE as f32 / config.sample_rate as f32;

    let mut position = 0.;
    let mut current = (0., 0.);
    let mut next = (0., 0.);

    let callback_progress = progress.clone();

    let stream = device.build_output_stream(config, move |data: &mut [f32], _: &_|
    {
        let gain = options::get_output_gain(); //ONCE PER CALLBACK, NOT PER SAMPLE

        for frame in data.chunks_mut(output_channels)
        {
            while position >= 1.
            {
                current = next;
                next = match playhead.next()
                {
                    Some(sample) => sample,
                    None =>
                    {
                        callback_progress.done.store(true, Ordering::Relaxed);
                        (0., 0.)
                    },
                };

                position -= 1.;
            }

            let left = ((current.0 + (next.0 - current.0) * position) * gain).tanh();
            let right = ((current.1 + (next.1 - current.1) * position) * gain).tanh();
            position += step;

            match frame.len()
            {
                1 => frame[0] = (left + right) * 0.5,
                _ =>
                {
                    frame.fill(0.);
                    frame[0] = left;
                    frame[1] = right;
                },
            }
        }
    }, |_| {}, None).map_err(|_| "voice_message.play_failed")?;

    stream.play().map_err(|_| "voice_message.play_failed")?;

    //KEEP IT IF STILL CURRENT
    let mut player = PLAYER.lock().unwrap();

    if PLAY_GENERATION.load(Ordering::Relaxed) == generation
    {
        *player = Some(Player { hash, progress, _stream: stream });
    }

    Ok(())
}

//PUBLIC
//RECORDING
/// Starts recording a voice message.
///
/// A failure to open the input outside a call is reported later as
/// [`ClientEvent::VoiceMessageFailed`].
///
/// # Returns
/// `false` if a recording is already running or the encoder could not be created.
pub fn start_recording(tx: Sender<ClientEvent>) -> bool //BEGIN RECORDING
{
    let mut guard = RECORDING.lock().unwrap();

    if guard.is_some() { return false; }

    let Some(encoder) = stereo() else { return false };

    //TAP A RUNNING CALL
    let voice = options::get_use_voice() && super::LOCAL_STREAMS.lock().unwrap().is_some();
    let generation = RECORD_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;

    *guard = Some(Recording
    {
        encoder,
        frames: Vec::new(),
        size: 0,
        pending: Vec::with_capacity(consts::FRAME_SIZE * 4),
        position: 0.,
        voice,
        full: false,
        generation,
    });

    drop(guard);

    if !voice
    {
        tokio::spawn(async move
        {
            if task::spawn_blocking(move || open_input(generation)).await.unwrap_or(false) { return; }

            //NOTHING TO RECORD FROM
            {
                let mut guard = RECORDING.lock().unwrap();

                if guard.as_ref().is_some_and(|recording| recording.generation == generation) { *guard = None; }
            }

            tx.send(ClientEvent::VoiceMessageFailed(t!("voice_message.record_failed").to_owned())).await.ok();
        });
    }

    true
}

/// Feeds raw captured audio to the recording.
///
/// # Parameters
/// - `data`: Interleaved samples.
/// - `channels`: The capture's channel count.
/// - `rate`: The capture's sample rate.
/// - `voice`: Whether this comes from the call's capture; a recording only takes the source it
///   was started with.
pub fn feed(data: &[f32], channels: usize, rate: f32, voice: bool) //RAW CAPTURE IN
{
    let Ok(mut guard) = RECORDING.try_lock() else { return };

    if let Some(recording) = guard.as_mut().filter(|recording| recording.voice == voice && !recording.full)
    {
        recording.feed(data, channels, rate);
    }
}

/// Returns the recording's state.
///
/// # Returns
/// `(milliseconds recorded, whether it must end now)` — at a limit, or because the call it was
/// tapping ended — or `None` if nothing is recording.
pub fn recording() -> Option<(u32, bool)> //(MS RECORDED, OUT OF ROOM)
{
    let guard = RECORDING.lock().unwrap();
    let recording = guard.as_ref()?;

    //THE CALL IT WAS TAPPING ENDED
    let ended = recording.voice && !options::get_use_voice();

    Some((recording.frames.len() as u32 * consts::FRAME_MS, recording.full || ended))
}

/// Ends the recording and returns the encoded clip.
///
/// # Returns
/// The clip, or `None` if nothing was recording or it is shorter than
/// [`MESSAGE_MIN_FRAMES`](consts::MESSAGE_MIN_FRAMES).
pub fn finish_recording() -> Option<Vec<u8>> //THE CLIP, None IF TOO SHORT
{
    //STOP THE CAPTURE FIRST
    let stream = OWN_INPUT.lock().unwrap().take();
    drop(stream);

    let mut recording = RECORDING.lock().unwrap().take()?;

    //PAD THE LAST PART-FRAME WITH SILENCE
    if !recording.full && !recording.pending.is_empty()
    {
        let mut frame = std::mem::take(&mut recording.pending);
        frame.resize(consts::FRAME_SIZE * 2, 0.);
        recording.encode(&frame);
    }

    (recording.frames.len() >= consts::MESSAGE_MIN_FRAMES).then(|| Clip
    {
        channels: consts::MESSAGE_CHANNELS,
        frames: recording.frames,
    }.encode())
}

/// Discards the recording.
pub fn cancel_recording() //THROW IT AWAY
{
    let stream = OWN_INPUT.lock().unwrap().take();
    drop(stream);

    RECORDING.lock().unwrap().take();
}

//PLAYBACK
/// Plays a clip from the cache, or requests it from the server.
///
/// A cache miss parks the hash and raises [`ClientEvent::ImageRequest`]; the answer is routed
/// back here through [`awaited`] and [`play_data`].
pub fn play(hash: [u8; 32], tx: Sender<ClientEvent>) //OUT OF THE CACHE, ELSE ASK THE SERVER
{
    let generation = PLAY_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;

    //ONE AT A TIME
    PLAYER.lock().unwrap().take();

    tokio::spawn(async move
    {
        match cache::load(&hash).await
        {
            Some(data) => play_data(hash, data, generation, tx).await,

            None =>
            {
                AWAITED.lock().unwrap().push((hash, generation));
                tx.send(ClientEvent::ImageRequest(hash)).await.ok();
            },
        }
    });
}

/// Plays a clip already in hand, reporting a failure as [`ClientEvent::VoiceMessageFailed`].
///
/// # Parameters
/// - `hash`: The clip's hash.
/// - `data`: The encoded clip.
/// - `generation`: The play generation that asked; a stale one does not start.
/// - `tx`: The UI event channel.
pub async fn play_data(hash: [u8; 32], data: Vec<u8>, generation: usize, tx: Sender<ClientEvent>) //A CLIP IN HAND
{
    let result = task::spawn_blocking(move || open_output(hash, &data, generation)).await.unwrap_or(Err("voice_message.play_failed"));

    if let Err(key) = result
    {
        tx.send(ClientEvent::VoiceMessageFailed(i18n::text(key).to_owned())).await.ok();
    }
}

/// Takes a hash off the list of clips awaited for playback.
///
/// # Returns
/// The play generation that asked for it, or `None` if nobody did.
pub fn awaited(hash: &[u8; 32]) -> Option<usize> //ITS PLAY GENERATION, IF ASKED FOR
{
    let mut awaited = AWAITED.lock().unwrap();
    let index = awaited.iter().position(|(h, _)| h == hash)?;

    Some(awaited.swap_remove(index).1)
}

/// Stops playback, including a start still in flight.
pub fn stop() //STOP WHATEVER IS PLAYING
{
    PLAY_GENERATION.fetch_add(1, Ordering::Relaxed);
    PLAYER.lock().unwrap().take();
}

/// Returns what is playing.
///
/// # Returns
/// `(clip hash, milliseconds played)`, or `None` if nothing is (a finished clip is cleared here).
pub fn playing() -> Option<([u8; 32], u32)> //(CLIP, MS PLAYED)
{
    let mut guard = PLAYER.lock().unwrap();
    let player = guard.as_ref()?;

    //A FINISHED ONE GOES
    if player.progress.done.load(Ordering::Relaxed)
    {
        *guard = None;
        return None;
    }

    Some((player.hash, player.progress.frames.load(Ordering::Relaxed) as u32 * consts::FRAME_MS))
}
