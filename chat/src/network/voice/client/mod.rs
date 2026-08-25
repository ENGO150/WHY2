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
pub mod sfx;
pub mod aec;
pub mod options;

use std::
{
    collections::{ BTreeMap, VecDeque },
    time::
    {
        SystemTime,
        Duration,
        UNIX_EPOCH,
    },
    sync::
    {
        Arc,
        Mutex,
        atomic::{ AtomicUsize, Ordering },
    },
};

use tokio::
{
    time,
    net::UdpSocket,
    net::tcp::OwnedWriteHalf,
    sync::
    {
        Mutex as MutexAsync,
        mpsc::{ self, Sender },
    },
};

use cpal::
{
    Host,
    Stream,
    Device,
    StreamConfig,
    SupportedStreamConfig,
    SupportedStreamConfigRange,
    traits::
    {
        DeviceTrait,
        HostTrait,
        StreamTrait,
    },
};

use audiopus::
{
    Channels,
    SampleRate,
    Application,
    TryFrom,
    coder::{ Encoder, Decoder },
};

use ringbuf::
{
    HeapRb,
    HeapCons,
    HeapProd,
    traits::
    {
        Split,
        Producer,
        Consumer,
    },
};

use nnnoiseless::DenoiseState;

use gag::Gag;

use crate::
{
    config,
    command::{ self, Command },
    options as chat_options,
    network::
    {
        client::{ ClientEvent, VoiceUser },
        voice::
        {
            self,
            consts,
            VoicePacketCode,
            client::sfx::SoundEffect,
        },
    },
};

#[cfg(target_os = "linux")]
use cpal::HostId;

//STRUCTS
struct LocalStream
{
    _input: Stream,
    _output: Stream,

    //THE PAIR THIS WAS BUILT FROM (EMPTY = SYSTEM DEFAULT), SO A FAILED SWITCH CAN GO BACK TO IT
    input_id: String,
    output_id: String,
}

//ONE DEVICE AS THE SETTINGS OVERLAY SHOWS IT. THE id IS WHAT client.toml STORES AND WHAT find_devices
//MATCHES ON - THE label IS ONLY EVER DISPLAYED, AND IS NOT UNIQUE ON ALSA.
#[derive(Clone)]
pub struct AudioDevice
{
    pub id: String,
    pub label: String,
}

struct StreamGuard
{
    generation: usize,
}

struct RemoteStream
{
    consumer: HeapCons<f32>,   //RINGBUFFER READER
    resample_pos: f32,         //POSITION IN BETWEEN SAMPLES
    current_sample: f32,       //CURRENT SAMPLE FOR INTERPOLATION
    next_sample: f32,          //NEXT SAMPLE FOR INTERPOLATION
    activity_hold: usize,      //ACTIVITY TIMER
    display_hold: usize,       //ACTIVITY WINDOW TIMER
    username: String,          //USERNAME
    latencies: VecDeque<u128>, //HISTORY OF LATENCIES
    avg_latency: u128,         //AVERAGE LATENCY TO DISPLAY
}

pub struct PeerData
{
    decoder: Decoder,        //DECODER
    producer: HeapProd<f32>, //RINGBUFFER WRITER
}

//GLOBAL VARIABLES
static LOCAL_STREAMS: Mutex<Option<LocalStream>> = Mutex::new(None);
static CONSUMERS: Mutex<BTreeMap<usize, (RemoteStream, PeerData)>> = Mutex::new(BTreeMap::new()); //OTHER CLIENTS

static LOCAL_DISPLAY_HOLD: AtomicUsize = AtomicUsize::new(0);
static AUDIO_GENERATION: AtomicUsize = AtomicUsize::new(0);

//IMPLEMENTATIONS
impl Drop for StreamGuard
{
    //CLEAR STREAMS
    fn drop(&mut self)
    {
        if AUDIO_GENERATION.load(Ordering::Relaxed) == self.generation
        {
            if let Ok(mut streams) = LOCAL_STREAMS.lock()
            {
                *streams = None;
            }

            //NOTHING OF OURS REACHES THE SINK ANY MORE, SO A RUNNING SHARE MUST STOP SUBTRACTING
            aec::set_rate(0);
        }
    }
}

//PRIVATE
fn device_id(device: &Device) -> String //THE HOST-QUALIFIED cpal ID, E.G. "alsa:plughw:CARD=1,DEV=0"
{
    device.id().map(|id| id.to_string()).unwrap_or_default()
}

//WHETHER A DEVICE IS WORTH OFFERING IN /settings. ALSA ENUMERATES EVERY PCM PLUGIN, MOST OF WHICH SHARE
//THE SAME DESCRIPTION AND EITHER CANNOT BE OPENED AT ALL (hw:, surround*, iec958 ON A STEREO CARD) OR
//DUPLICATE ONE THAT CAN - plughw: DOES THE FORMAT CONVERSION FOR US, SO IT IS THE ONE WE KEEP. cards IS
//CLEARED ONCE A SOUND SERVER IS IN THE PICTURE: IT HOLDS THE CARDS ITSELF AND ALSA ONLY REPORTS THEM BUSY,
//SO THE SERVER'S OWN HOST IS WHERE A NAMED DEVICE COMES FROM.
fn is_usable(id: &str, cards: bool) -> bool
{
    #[cfg(target_os = "linux")]
    {
        let Some(pcm) = id.strip_prefix("alsa:") else { return true };

        if pcm == "null" { return false; }

        //A PLUGIN WITHOUT A CARD (default, sysdefault, pipewire, pulse, ...) IS THE SOUND SERVER ITSELF
        if !pcm.contains("CARD=") { return true; }

        cards && pcm.starts_with("plughw:")
    }

    #[cfg(not(target_os = "linux"))]
    {
        let (_, _) = (id, cards);
        true
    }
}

//EVERY HOST THE VOICE CLIENT IS WILLING TO OPEN A DEVICE IN, LOWEST LATENCY FIRST. audio_host() IS WHERE
//THE SYSTEM DEFAULT COMES FROM; THE SOUND SERVER'S OWN HOST FOLLOWS IT, BECAUSE PIPEWIRE/PULSEAUDIO HOLD
//THE RAW CARDS AND ALSA CAN ONLY REPORT THEM AS BUSY - PICKING A NAMED DEVICE HAS TO GO THROUGH THE SERVER.
fn audio_hosts() -> Vec<Host>
{
    let primary = audio_host();
    let mut hosts = vec![primary];

    let fallback = cpal::default_host();
    if fallback.id() != hosts[0].id() { hosts.push(fallback); }

    hosts
}

//EVERY DEVICE THE VOICE CLIENT COULD OPEN, ENUMERATED IN THE SAME HOSTS THAT LATER OPEN THEM - THE LIST
//THAT /settings SHOWS AND THE ID IT WRITES HAVE TO COME FROM THE SAME PLACE, OR THE SAVED DEVICE MATCHES
//NOTHING WHEN THE TIME COMES TO OPEN IT.
//BLOCKING, AND ALSA SPEAKS TO fd 2 - HENCE THE GAG.
pub fn list_devices() -> (Vec<AudioDevice>, Vec<AudioDevice>) //(INPUT, OUTPUT)
{
    let _stderr_gag = Gag::stderr().ok();
    let hosts = audio_hosts();

    //WITH A SOUND SERVER RUNNING, THE RAW ALSA CARDS BELONG TO IT AND ARE NOT OURS TO OPEN
    let cards = hosts.len() == 1;

    let collect = |input: bool|
    {
        let mut out: Vec<AudioDevice> = Vec::new();

        for host in hosts.iter() { collect_devices(host, input, cards, &mut out); }

        out
    };

    (collect(true), collect(false))
}

fn collect_devices(host: &Host, input: bool, cards: bool, out: &mut Vec<AudioDevice>)
{
    let Ok(devices) = (if input { host.input_devices() } else { host.output_devices() }) else { return };

    let start = out.len();

    for device in devices
    {
        let id = device_id(&device);

        if id.is_empty() || !is_usable(&id, cards) || out.iter().any(|entry| entry.id == id) { continue; }

        let Ok(label) = device.description().map(|d| d.to_string()) else { continue };

        out.push(AudioDevice { id, label });
    }

    //EACH HOST IS SORTED WITHIN ITSELF, SO THE ORDER OF THE HOSTS THEMSELVES SURVIVES
    out[start..].sort_by(|a, b| a.label.cmp(&b.label).then_with(|| a.id.cmp(&b.id)));
}

//THE DEVICE wanted POINTS AT, OR THE SYSTEM DEFAULT WHEN IT IS EMPTY. THE ID CARRIES ITS HOST, SO A DEVICE
//IS LOOKED FOR IN EVERY HOST AND CAN ONLY EVER MATCH THE ONE IT CAME FROM.
fn pick_device(wanted: &str, input: bool) -> Option<Device>
{
    let hosts = audio_hosts();

    if wanted.is_empty()
    {
        let host = hosts.first()?;

        return if input { host.default_input_device() } else { host.default_output_device() };
    }

    let devices: Vec<Device> = hosts.iter()
        .filter_map(|host| if input { host.input_devices().ok() } else { host.output_devices().ok() })
        .flatten()
        .collect();

    devices.iter().find(|device| device_id(device) == wanted)
        //A CONFIG WRITTEN BEFORE DEVICES WERE STORED BY ID STILL HOLDS A DESCRIPTION
        .or_else(|| devices.iter().find(|device| device.description().is_ok_and(|desc| desc.to_string() == wanted)))
        .cloned()
}

fn configured_ids() -> (String, String) //THE PAIR client.toml CURRENTLY POINTS AT
{
    (config::read_config::<String>("input_device"), config::read_config::<String>("output_device"))
}

pub fn configure_device(device: &cpal::Device, supported_configs: impl Iterator<Item = SupportedStreamConfigRange>, default_config: SupportedStreamConfig, is_input_stream: bool) -> StreamConfig
{
    let mut config: StreamConfig = supported_configs
        .filter(|c| c.min_sample_rate() <= consts::SAMPLE_RATE && c.max_sample_rate() >= consts::SAMPLE_RATE)
        .next()
        .map(|c| c.with_sample_rate(consts::SAMPLE_RATE))
        .unwrap_or(default_config.clone())
        .into();

    //TEST CONFIG (WASAPI FALLBACK)
    if is_input_stream
    {
        if let Ok(test_stream) = device.build_input_stream(config.clone(), |_: &[f32], _| {}, |_| {}, None)
        {
            drop(test_stream); //WORKS, PROCEED
        } else
        {
            config = default_config.into(); //FALLBACK TO DEFAULT
        }
    } else
    {
        if let Ok(test_stream) = device.build_output_stream(config.clone(), |_: &mut [f32], _| {}, |_| {}, None)
        {
            drop(test_stream); //WORKS, PROCEED
        } else
        {
            config = default_config.into(); //FALLBACK TO DEFAULT
        }
    }

    config
}

fn transmit_audio(encoder: &Encoder, frame: &mut [f32], buffer: &mut [u8], tx: &Sender<Vec<u8>>)
{
    //MICROPHONE VOLUME (/settings) - APPLIED AFTER THE AGC, SO IT STAYS THE USER'S LAST WORD ON THE LEVEL
    let gain = options::get_input_gain();
    if gain != 1.
    {
        for sample in frame.iter_mut()
        {
            *sample = (*sample * gain).clamp(-1., 1.);
        }
    }

    //ENCODE (IGNORE ERRORS)
    if let Ok(len) = encoder.encode_float(&frame, buffer)
    {
        //HAND OVER TO THE NETWORK TASK (NEVER BLOCK THE AUDIO CALLBACK)
        tx.try_send(buffer[..len].to_vec()).ok();
    }
}

//NORMALIZE ONE FRAME IN PLACE, MUST BE CALLED ON EVERY TRANSMITTED FRAME IN ORDER (THE GAIN IS STATEFUL)
fn apply_agc(frame: &mut [f32], rms: f32, is_speech: bool, envelope: &mut f32, gain: &mut f32)
{
    if !options::automatic_gain() { return; } //TURNED OFF IN /settings - THE RAW LEVEL GOES OUT AS CAPTURED

    //TRACK THE SPEECH LEVEL ONLY WHILE SOMEBODY IS ACTUALLY TALKING
    if is_speech
    {
        let smoothing = if rms > *envelope { consts::AGC_ATTACK } else { consts::AGC_RELEASE };
        *envelope += (rms - *envelope) * smoothing;
    }

    //GAIN
    let target_gain = (consts::AGC_TARGET_RMS / envelope.max(1e-6)).clamp(consts::AGC_MIN_GAIN, consts::AGC_MAX_GAIN);
    let slew = if target_gain < *gain { consts::AGC_GAIN_DOWN } else { consts::AGC_GAIN_UP };
    *gain += (target_gain - *gain) * slew;

    //APPLY WITH A SOFT KNEE, EVERYTHING BELOW THE KNEE STAYS UNTOUCHED
    for sample in frame.iter_mut()
    {
        let amplified = *sample * *gain;
        let magnitude = amplified.abs();

        *sample = if magnitude > consts::LIMITER_KNEE
        {
            let headroom = 1. - consts::LIMITER_KNEE;
            let over = (magnitude - consts::LIMITER_KNEE) / headroom;

            amplified.signum() * (consts::LIMITER_KNEE + headroom * over.tanh())
        } else
        {
            amplified
        };
    }
}

fn audio_host() -> Host //THE HOST THE VOICE CLIENT TALKS TO
{
    #[cfg(target_os = "linux")]
    {
        cpal::host_from_id(HostId::Alsa).unwrap_or_else(|_| cpal::default_host())
    }

    #[cfg(not(target_os = "linux"))]
    {
        cpal::default_host()
    }
}

//CAPTURE SIDE. EVERY PIECE OF STATE (VAD, AGC, RESAMPLER) IS BUILT HERE, SO A REBUILD STARTS FROM A CLEAN SLATE.
fn build_input_stream(device: &Device, config: StreamConfig, current_generation: usize, packet_tx: Sender<Vec<u8>>) -> Option<Stream>
{
    //PREPARE OPUS ENCODER
    let opus_encoder = Encoder::new
    (
        <SampleRate as TryFrom<i32>>::try_from(consts::SAMPLE_RATE as i32).unwrap(),
        Channels::Mono,
        Application::Voip
    ).unwrap();

    //INPUT BUFFERS
    let mut input_accum: Vec<f32> = Vec::with_capacity(consts::FRAME_SIZE * 2);
    let mut encoded_buffer = [0u8; 1500]; //ALLOCATE BUFFER TO STANDARD MTU

    //INPUT RESAMPLING
    let input_channels = config.channels as usize;
    let input_source_rate = config.sample_rate as f32;
    let input_target_rate = consts::SAMPLE_RATE as f32;

    //INPUT INTERPOLATION
    let input_resample_step = input_source_rate / input_target_rate;
    let mut input_resample_pos = 0.;

    //VAD
    let gate_open = Arc::new(Mutex::new(false)); //NOISE GATE
    let preroll_buffer = Arc::new(Mutex::new(VecDeque::<Vec<f32>>::with_capacity(3))); //PRE-ROLL BUFFER
    let hold_frames_remaining = Arc::new(Mutex::new(0usize)); //HOLD TIME
    let noise_floor = Arc::new(Mutex::new(consts::INITIAL_NOISE_FLOOR)); //ADAPTIVE NOISE FLOOR
    let agc_envelope = Arc::new(Mutex::new(consts::AGC_TARGET_RMS)); //ENVELOPE
    let agc_gain = Arc::new(Mutex::new(1.0f32)); //ACTUAL GAIN

    //NOISE REDUCTION
    let mut denoiser = DenoiseState::new();
    let mut denoise_buffer = [0.0f32; consts::SAMPLE_RATE as usize / 100];

    //CONFIGURE INPUT STREAM
    let noise_floor_cb = noise_floor.clone();
    let agc_envelope_cb = agc_envelope.clone();
    let agc_gain_cb = agc_gain.clone();
    device.build_input_stream(config, move |data: &[f32], _: &_|
    {
        //CHECK FOR MUTING (A MICROPHONE TURNED DOWN TO 0% IS OFF, VOICE ACTIVITY INCLUDED)
        if chat_options::is_muted(None) || options::get_input_volume() == 0
        {
            LOCAL_DISPLAY_HOLD.store(0, Ordering::Relaxed); //CLEAR VAD WINDOW
            input_accum.clear(); //REMOVE BLOB REMAINING IN BUFFER

            //CLOSE GATE
            if let Ok(mut gate) = gate_open.lock()
            {
                *gate = false;
            }

            //CLEAR PREROLL
            if let Ok(mut preroll) = preroll_buffer.lock()
            {
                preroll.clear();
            }

            return;
        }

        //CHECK GENERATION
        if AUDIO_GENERATION.load(Ordering::Relaxed) != current_generation { return; }

        let frames_in_buffer = data.len() / input_channels;

        let current_hold = LOCAL_DISPLAY_HOLD.load(Ordering::Relaxed);
        if current_hold > 0
        {
             LOCAL_DISPLAY_HOLD.store(current_hold.saturating_sub(frames_in_buffer), Ordering::Relaxed);
        }

        //MONO DOWNMIX CLOSURE
        let get_mono_sample = |index: usize| -> f32
        {
            if index >= frames_in_buffer { return 0. }

            let mut sum = 0.;
            for c in 0..input_channels
            {
                sum += data[index * input_channels + c];
            }

            sum / input_channels as f32
        };

        //RESAMPLE LOOP
        while input_resample_pos < (frames_in_buffer as f32) - 1.
        {
            let idx = input_resample_pos.floor() as usize;
            let frac = input_resample_pos - idx as f32;

            let s0 = get_mono_sample(idx);
            let s1 = get_mono_sample(idx + 1);

            let interpolated = s0 + (s1 - s0) * frac;
            input_accum.push(interpolated);

            input_resample_pos += input_resample_step;
        }

        //ADJUST POSITION FOR NEXT BUFFER
        input_resample_pos -= frames_in_buffer as f32;

        //PROCESS
        while input_accum.len() >= consts::FRAME_SIZE
        {
            let mut frame: Vec<f32> = input_accum.drain(0..consts::FRAME_SIZE).collect();

            //NOISE REDUCTION (SKIPPED WHEN TURNED OFF IN /settings)
            for chunk in frame.chunks_mut(consts::SAMPLE_RATE as usize / 100)
            {
                if options::noise_suppression() && chunk.len() == consts::SAMPLE_RATE as usize / 100
                {
                    //SCALE UP
                    for sample in chunk.iter_mut()
                    {
                        *sample *= 32767.;
                    }

                    //PROCESS NOISE
                    denoiser.process_frame(&mut denoise_buffer, chunk);

                    //SCALE DOWN & COPY
                    for (i, sample) in denoise_buffer.iter().enumerate()
                    {
                        chunk[i] = sample / 32767.;
                    }
                }
            }

            //VAD (RUNS ON THE CLEAN SIGNAL, BEFORE THE AGC, SO THE TRESHOLDS STAY COMPARABLE ACROSS FRAMES)
            let rms = (frame.iter().map(|&x| x * x).sum::<f32>() / frame.len() as f32 + 1e-10).sqrt(); //RMS CALCULATION (+ SMALL BIAS)
            let mut gate = gate_open.lock().unwrap();
            let mut preroll = preroll_buffer.lock().unwrap();
            let mut hold_frames = hold_frames_remaining.lock().unwrap();
            let mut nf = noise_floor_cb.lock().unwrap();
            let mut envelope = agc_envelope_cb.lock().unwrap();
            let mut gain = agc_gain_cb.lock().unwrap();

            //PREVENT NOISE FLOOR CONTAMINATION BY VOICE
            if !*gate
            {
                *nf += (rms - *nf) * consts::NOISE_FLOOR_ALPHA;
            }

            //DYNAMIC TRESHOLDS
            let treshold_open = (*nf * consts::NOISE_OPEN_MULT).max(consts::MIN_TRESHOLD_OPEN);
            let treshold_close = (*nf * consts::NOISE_CLOSE_MULT).max(consts::MIN_TRESHOLD_CLOSE);
            drop(nf);

            //HYSTERESIS
            if !*gate && rms > treshold_open
            {
                *gate = true; //SPEAKING

                //SEND STORED FRAMES (QUIET LEAD-IN, SO NORMALIZE THEM BUT KEEP THEM OUT OF THE ENVELOPE)
                for mut old_frame in preroll.drain(..)
                {
                    apply_agc(&mut old_frame, rms, false, &mut envelope, &mut gain);
                    transmit_audio(&opus_encoder, &mut old_frame, &mut encoded_buffer, &packet_tx);
                }

                *hold_frames = consts::HOLD_FRAMES;
            } else if *gate && rms < treshold_close
            {
                if *hold_frames > 0 //SILENT FRAME, DECREMENT
                {
                    *hold_frames -= 1;
                } else //HOLD TIME EXPIRED, CLOSE GATE
                {
                    *gate = false;
                }
            } else if *gate && rms >= treshold_close //SPEAKING CONTINUES, RESET HOLD TIMER
            {
                *hold_frames = consts::HOLD_FRAMES;
            }

            //STORE TO PRE-ROLL BUFFER (MAX 3 FRAMES)
            if !*gate
            {
                preroll.push_back(frame.clone());
                if preroll.len() > 3
                {
                    preroll.pop_front();
                }
            }

            //TRANSMIT ONLY IF GATE IS OPEN
            if *gate
            {
                LOCAL_DISPLAY_HOLD.store((consts::SAMPLE_RATE * consts::DISPLAY_HOLD as u32 / 1000) as usize, Ordering::Relaxed);

                //ONLY FRAMES CARRYING REAL SPEECH ENERGY FEED THE ENVELOPE, HOLD-TAIL FRAMES DO NOT
                apply_agc(&mut frame, rms, rms >= treshold_close, &mut envelope, &mut gain);
                transmit_audio(&opus_encoder, &mut frame, &mut encoded_buffer, &packet_tx);
            }
        }
    }, |_| {}, None).ok()
}

//PLAYBACK SIDE. THE PEERS THEMSELVES LIVE IN CONSUMERS, WHICH A REBUILD LEAVES ALONE.
fn build_output_stream(device: &Device, config: StreamConfig, current_generation: usize) -> Option<Stream>
{
    //OUTPUT RESAMPLING
    let output_channels = config.channels as usize;
    let output_source_rate = consts::SAMPLE_RATE as f32;
    let output_target_rate = config.sample_rate as f32;

    //OUTPUT INTERPOLATION
    let output_resample_step = output_source_rate / output_target_rate;

    //THE SCREEN SHARE SUBTRACTS THIS STREAM BACK OUT OF THE SINK MONITOR (SEE aec.rs), AND NEEDS THE RATE
    //IT IS PRODUCED AT. IT IS ALSO HOW A REBUILT STREAM TELLS THE CANCELLER TO FIND THE DELAY AGAIN.
    aec::set_rate(config.sample_rate);

    let mut reference = Vec::with_capacity(consts::FRAME_SIZE);

    //CONFIGURE OUTPUT STREAM
    device.build_output_stream(config, move |data: &mut [f32], _: &_|
    {
        //CHECK GENERATION
        if AUDIO_GENERATION.load(Ordering::Relaxed) != current_generation { return; }

        //CLEAR OUTPUT BUFFER
        data.fill(0.);

        let output_gain = options::get_output_gain(); //ONCE PER CALLBACK, NOT PER SAMPLE
        let frames_to_write = data.len() / output_channels;
        let mut consumers_guard = CONSUMERS.lock().unwrap();

        reference.clear();

        for i in 0..frames_to_write
        {
            let mut mixed_sample = 0.;
            let mut active_speakers = 0;

            for (stream, _) in consumers_guard.values_mut()
            {
                //RESAMPLE LOOP
                while stream.resample_pos >= 1.
                {
                    stream.current_sample = stream.next_sample;
                    stream.next_sample = stream.consumer.try_pop().unwrap_or(0.); //SILENCE ON UNDERRUN
                    stream.resample_pos -= 1.;
                }

                //LINEAR INTERPOLATION
                let interpolated = stream.current_sample + (stream.next_sample - stream.current_sample) * stream.resample_pos;
                stream.resample_pos += output_resample_step; //MOVE RESAMPLER POSITION FOR THIS CLIENT

                //ACTIVE SPEAKER DETECTION
                if interpolated.abs() > consts::MIXING_TRESHOLD
                {
                    stream.activity_hold = consts::ACTIVITY_HOLD; //SET TIMER TO ~100ms
                    stream.display_hold = (consts::SAMPLE_RATE * consts::DISPLAY_HOLD as u32 / 1000) as usize; //SET DISPLAY FOR ~1000ms
                }

                if stream.activity_hold > 0
                {
                    //MIX
                    mixed_sample += interpolated;
                    active_speakers += 1;
                    stream.activity_hold -= 1;
                }

                //DECREMENT DISPLAY TIMER
                if stream.display_hold > 0
                {
                    stream.display_hold -= 1;
                }
            }

            //NORMALIZATION
            if active_speakers > 1
            {
                mixed_sample /= (active_speakers as f32).sqrt();
            }

            //MIX EFFECTS
            sfx::play_effects(&mut mixed_sample);

            //PLAYBACK VOLUME (/settings)
            mixed_sample *= output_gain;

            //SOFT CLIPPING (HYPERBOLIC TANGENT)
            mixed_sample = mixed_sample.tanh();

            //THE REFERENCE IS TAPPED HERE, PAST EVERY STAGE THAT SHAPES IT - WHAT THE SINK RECEIVES IS
            //WHAT THE SHARE HAS TO TAKE BACK OUT, AND A SILENT FRAME COUNTS AS MUCH AS A LOUD ONE
            reference.push(mixed_sample);

            //WRITE SAMPLE TO ALL CHANNELS
            for channel in 0..output_channels
            {
                data[i * output_channels + channel] = mixed_sample;
            }
        }

        drop(consumers_guard);

        aec::push_reference(&reference);
    }, |_| {}, None).ok()
}

//BOTH STREAMS, ALREADY PLAYING. wanted OVERRIDES THE CONFIGURED PAIR (USED TO GO BACK TO A DEVICE THAT WORKED).
fn build_streams(current_generation: usize, packet_tx: &Sender<Vec<u8>>, wanted: Option<(String, String)>) -> Option<LocalStream>
{
    let (wanted_input, wanted_output) = wanted.unwrap_or_else(configured_ids);

    //FIND INPUT/OUTPUT DEVICE (SUPPRESS STDERR TO AVOID ALSA ERRORS)
    let (input_device, output_device) =
    {
        let _stderr_gag = Gag::stderr().ok();

        (pick_device(&wanted_input, true)?, pick_device(&wanted_output, false)?)
    };

    //CONFIGURE CPAL INPUT
    let input_config = configure_device(&input_device, input_device.supported_input_configs().ok()?,
        input_device.default_input_config().ok()?, true);

    //CONFIGURE CPAL OUTPUT
    let output_config = configure_device(&output_device, output_device.supported_output_configs().ok()?,
        output_device.default_output_config().ok()?, false);

    let input_stream = build_input_stream(&input_device, input_config, current_generation, packet_tx.clone())?;
    let output_stream = build_output_stream(&output_device, output_config, current_generation)?;

    //RUN STREAMS
    input_stream.play().ok()?;  //INPUT
    output_stream.play().ok()?; //OUTPUT

    Some(LocalStream
    {
        _input: input_stream,
        _output: output_stream,
        input_id: wanted_input,
        output_id: wanted_output,
    })
}

//SWAPS IN A FRESHLY BUILT PAIR AFTER A /settings DEVICE CHANGE. THE UDP SESSION, THE PEERS AND THE JITTER
//BUFFERS ARE UNTOUCHED - ONLY THE TWO CPAL STREAMS ARE REPLACED.
fn replace_streams(current_generation: usize, packet_tx: &Sender<Vec<u8>>) -> bool
{
    //THE OLD PAIR HAS TO STOP BEFORE THE NEW ONE OPENS - AN ALSA PCM IS EXCLUSIVE, SO THE DEVICE THAT IS
    //KEPT ACROSS THE SWITCH (ONLY ONE OF THE TWO USUALLY CHANGES) WOULD REFUSE THE SECOND OPEN
    let previous = LOCAL_STREAMS.lock().unwrap().take();
    let restore = previous.as_ref().map(|streams| (streams.input_id.clone(), streams.output_id.clone()));
    drop(previous);

    if let Some(streams) = build_streams(current_generation, packet_tx, None)
    {
        *LOCAL_STREAMS.lock().unwrap() = Some(streams);

        return true;
    }

    //THE NEW DEVICE WILL NOT OPEN - PUT THE CALL BACK ON THE PAIR THAT WAS PLAYING, AND POINT THE CONFIG
    //AT IT AGAIN, SO THE SETTINGS ROW AND THE NEXT JOIN BOTH AGREE WITH WHAT IS ACTUALLY RUNNING
    if let Some((input_id, output_id)) = restore
    {
        config::client_write("input_device", &input_id);
        config::client_write("output_device", &output_id);

        if let Some(streams) = build_streams(current_generation, packet_tx, Some((input_id, output_id)))
        {
            *LOCAL_STREAMS.lock().unwrap() = Some(streams);
        }
    }

    false
}

//PUBLIC
pub async fn listen_server_voice //SERVER -> CLIENT
(
    id: usize,
    username: String,
    tx: Sender<ClientEvent>,
    write_stream: Arc<MutexAsync<OwnedWriteHalf>>
)
{
    //RESET SEQs
    options::set_seq(0);
    options::set_server_seq(0);

    //LOAD THE AUDIO PREFERENCES WHILE WE ARE STILL ALLOWED TO TOUCH THE FILESYSTEM
    options::init_audio();

    //DUPLICATE STREAM GUARDS
    let current_generation = AUDIO_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    let _guard = StreamGuard { generation: current_generation };

    //CONNECT
    let socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await.expect("Binding UDP failed"));
    socket.connect(chat_options::get_server_address()).await.expect("Connecting to server UDP failed");

    //SEND HELLO PACKET
    voice::send(&socket, id, VoicePacketCode::Hello, &chat_options::get_keys().unwrap()).await.ok();

    //AUDIO CALLBACK -> NETWORK TASK BRIDGE
    let (packet_tx, mut packet_rx) = mpsc::channel::<Vec<u8>>(consts::SEND_CHANNEL_BOUND);

    //SPAWN TRANSMIT TASK
    let send_socket = socket.clone();
    tokio::spawn(async move
    {
        while let Some(data) = packet_rx.recv().await
        {
            //CHECK GENERATION
            if AUDIO_GENERATION.load(Ordering::Relaxed) != current_generation { return; }

            voice::send(&send_socket, id, VoicePacketCode::Audio
            {
                data,
                username: None,
            }, &chat_options::get_keys().unwrap()).await.ok();
        }
    });

    //BUILD AND START THE CPAL STREAMS
    let streams = match build_streams(current_generation, &packet_tx, None)
    {
        Some(streams) => streams,
        None => //NO USABLE DEVICE
        {
            //LEAVE VOICE
            command::send_command_code(&mut *write_stream.lock().await, &Command::Voice, &None).await;
            return;
        }
    };

    //MOVE STREAMS TO GLOBAL STORAGE
    *LOCAL_STREAMS.lock().unwrap() = Some(streams);

    //PLAY JOIN SOUND
    sfx::clear_effects();
    sfx::queue_effect(SoundEffect::Join);

    //START VOICE ACTIVITY DISPLAY & PING TASK (ALSO THE ONE PLACE THAT NOTICES A /settings DEVICE CHANGE)
    let vad_socket = socket.clone();
    tokio::spawn(async move
    {
        let mut iteration_counter = 0u8;
        let mut devices = options::device_generation();

        loop
        {
            //A LEFTOVER TASK FROM AN EARLIER SESSION MUST NOT TOUCH THIS ONE'S STREAMS
            if AUDIO_GENERATION.load(Ordering::Relaxed) != current_generation { return; }

            //QUIT ON /leave
            if !options::get_use_voice()
            {
                tx.send(ClientEvent::VoiceActivity(Vec::new())).await.unwrap(); //CLEAR WINDOW
                return;
            }

            //THE USER PICKED A DIFFERENT DEVICE - REBUILD BOTH STREAMS, KEEP THE CALL
            let generation = options::device_generation();
            if generation != devices
            {
                devices = generation;

                //A DEVICE THAT WILL NOT OPEN IS WORTH SAYING OUT LOUD - THE OLD ONE KEEPS RUNNING
                if !replace_streams(current_generation, &packet_tx)
                {
                    tx.send(ClientEvent::VoiceDeviceFailed).await.unwrap();
                }
            }

            iteration_counter += 1; //INCREMENT

            //SHOW VOICE ACTIVITY
            display_active_speakers(&username, &tx).await;

            //SEND PING PACKET
            if iteration_counter == 10
            {
                voice::send(&vad_socket, id, VoicePacketCode::Ping
                {
                    timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis(),
                }, &chat_options::get_keys().unwrap()).await.ok();

                //RESET COUNTER
                iteration_counter = 0;
            }

            time::sleep(Duration::from_millis(100)).await;
        }
    });

    //OUTPUT BUFFERS
    let mut decoded_buffer = [0.0f32; consts::FRAME_SIZE];

    loop
    {
        //READ
        let (network_buffer, _) = match voice::receive(&socket).await
        {
            Some(r) => r,
            None => //READING FAILED, TIMEOUT OR CRASH PROBABLY
            {
                //CHECK GENERATION
                if AUDIO_GENERATION.load(Ordering::Relaxed) != current_generation { return; }

                //PLAY LEAVE SOUND EFFECT
                sfx::queue_effect(SoundEffect::Leave);

                //FINISH SOUND EFFECTS
                while sfx::is_playing()
                {
                    //CHECK GENERATION AGAIN AHAHAHHAHAAH
                    if AUDIO_GENERATION.load(Ordering::Relaxed) != current_generation { return; }

                    time::sleep(Duration::from_millis(50)).await;
                }

                return;
            }
        };

        //VERIFY SERVER SEQ
        if network_buffer.seq <= options::get_server_seq() { continue; } //INGORE INVALID SEQs
        options::set_server_seq(network_buffer.seq); //SET SERVER SEQ

        //PING HAS TO BE ANSWERED OUTSIDE OF THE CONSUMERS LOCK
        let mut pong: Option<u128> = None;

        if let Some((stream, peer)) = CONSUMERS.lock().unwrap().get_mut(&network_buffer.id)
        {
            //CODES
            match network_buffer.code
            {
                //AUDIO RECEIVED
                VoicePacketCode::Audio { data, .. } =>
                {

                    //CHECK FOR MUTED CLIENT
                    if chat_options::is_muted(Some(network_buffer.id)) { continue; }

                    //DECODE
                    if let Ok(decoded_len) = peer.decoder.decode_float(Some(&data), &mut decoded_buffer[..], false)
                    {
                        //PUSH TO RINGBUFFER
                        peer.producer.push_slice(&decoded_buffer[..decoded_len]);
                    }
                },

                //PING RECEIVED, SEND BACK
                VoicePacketCode::Ping { timestamp } =>
                {
                    pong = Some(timestamp);
                },

                //PING FORWARDED BACK, CALCULATE LATENCY
                VoicePacketCode::Pong { timestamp, .. } =>
                {
                    //CALCULATE LATENCY
                    let latency = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis().saturating_sub(timestamp);

                    //STORE LATENCY TO BUFFER
                    stream.latencies.push_back(latency);
                    if stream.latencies.len() > 20 //STORE ONLY LATEST 20 LATENCIES
                    {
                        stream.latencies.pop_front();
                    }

                    //CALCULATE AVERAGE LATENCY
                    let sum: u128 = stream.latencies.iter().sum();
                    if !stream.latencies.is_empty()
                    {
                        //STORE IN AVG_LATENCY
                        stream.avg_latency = sum / stream.latencies.len() as u128;
                    }
                },

                _ => {}, //IGNORE OTHER CODES
            }
        }

        //SEND PONG PACKET
        if let Some(timestamp) = pong
        {
            voice::send(&socket, id, VoicePacketCode::Pong
            {
                target_id: network_buffer.id,
                timestamp: timestamp,
            }, &chat_options::get_keys().unwrap()).await.ok();
        }
    }
}

pub fn remove_consumer(id: &usize)
{
    if CONSUMERS.lock().unwrap().remove(id).is_some()
    {
        //PLAY LEAVE SOUND EFFECT
        sfx::queue_effect(SoundEffect::Leave);
    }
}

pub fn remove_all_consumers()
{
    CONSUMERS.lock().unwrap().clear();

    //PLAY JOIN SOUND EFFECT (THIS IS CALLED ON CHANNEL CHANGE)
    sfx::queue_effect(SoundEffect::Join);
}

pub fn add_consumer(id: usize, username: String)
{
    //OPUS DECODER
    let decoder = Decoder::new
    (
        <SampleRate as TryFrom<i32>>::try_from(consts::SAMPLE_RATE as i32).unwrap(),
        Channels::Mono,
    ).unwrap();

    //JITTER BUFFER
    let rb = HeapRb::<f32>::new(consts::FRAME_SIZE * consts::JITTER_BUFFER_SIZE);
    let (producer, mut consumer) = rb.split();

    let first_sample = consumer.try_pop().unwrap_or(0.0);

    //INSERT TO SHARED AUDIO THREAD MAP
    CONSUMERS.lock().unwrap().insert(id, (RemoteStream
    {
        consumer: consumer,
        resample_pos: 0.,
        current_sample: 0.,
        next_sample: first_sample,
        activity_hold: 0,
        display_hold: 0,
        username: username,
        latencies: VecDeque::with_capacity(20),
        avg_latency: 0,
    }, PeerData
    {
        decoder: decoder,
        producer: producer,
    }));

    //PLAY JOIN SOUND EFFECT
    sfx::queue_effect(SoundEffect::Join);
}

async fn display_active_speakers(local_username: &str, tx: &Sender<ClientEvent>)
{
    //ALL USERS
    let mut users_to_display = Vec::new();

    //ADD LOCAL CLIENT
    let local_speaking = LOCAL_DISPLAY_HOLD.load(Ordering::Relaxed) > 0;
    users_to_display.push(VoiceUser
    {
        id: 0,
        username: local_username.to_string(),
        is_speaking: local_speaking,
        latency: 0,
        is_local: true,
    });

    //COLLECT OTHER USERS
    if let Ok(consumers) = CONSUMERS.try_lock()
    {
        for (id, (stream, _)) in consumers.iter()
        {
            users_to_display.push(VoiceUser
            {
                id: *id,
                username: stream.username.clone(),
                is_speaking: stream.display_hold > 0, //SPEAKING
                latency: stream.avg_latency,
                is_local: false,
            });
        }
    }

    //SORT
    if users_to_display.len() > 1
    {
        users_to_display[1..].sort_by_key(|u| u.id);
    }

    //DISPLAY
    tx.send(ClientEvent::VoiceActivity(users_to_display)).await.unwrap();
}
