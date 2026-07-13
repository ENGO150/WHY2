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
pub mod device;
pub mod sfx;
pub mod options;

use std::
{
    thread,
    net::{ UdpSocket, TcpStream },
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
        mpsc::Sender,
        atomic::{ AtomicUsize, Ordering },
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
            consts,
            self,
            VoiceCode,
            VoicePacket,
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
        }
    }
}

//PRIVATE
fn find_devices(host: &Host) -> Option<(Device, Device)> //FIND CONFIGURED/DEFAULT DEVICE
{
    //GET DEVICES FROM CONFIG
    let input_device = config::read_config::<String>("input_device");
    let output_device = config::read_config::<String>("output_device");

    //GET INPUT DEVICE (OR DEFAULT IF EMPTY)
    let input_device = if !input_device.is_empty()
    {
        host.input_devices().ok()?.find(|d| d.description().is_ok_and(|desc| desc.to_string() == input_device))
    } else { host.default_input_device() };

    //GET OUTPUT DEVICE (OR DEFAULT IF EMPTY)
    let output_device = if !output_device.is_empty()
    {
        host.output_devices().ok()?.find(|d| d.description().is_ok_and(|desc| desc.to_string() == output_device))
    } else { host.default_output_device() };

    Some((input_device?, output_device?))
}

pub fn configure_device(supported_configs: impl Iterator<Item = SupportedStreamConfigRange>, default_config: SupportedStreamConfig) -> StreamConfig
{
    supported_configs
        .filter(|c| c.min_sample_rate() <= consts::SAMPLE_RATE && c.max_sample_rate() >= consts::SAMPLE_RATE)
        .next()
        .map(|c| c.with_sample_rate(consts::SAMPLE_RATE))
        .unwrap_or(default_config)
        .into()
}

fn transmit_audio(encoder: &Encoder, frame: &[f32], buffer: &mut [u8], id: usize, socket: &UdpSocket)
{
    //ENCODE (IGNORE ERRORS)
    if let Ok(len) = encoder.encode_float(&frame, buffer)
    {
        //TRANSMIT
        voice::send(socket, VoicePacket
        {
            voice: Some(buffer[..len].to_vec()),
            id: Some(id),

            ..Default::default()
        }, &chat_options::get_keys().unwrap()).unwrap();
    }
}

//PUBLIC
pub fn listen_server_voice //SERVER -> CLIENT
(
    id: usize,
    username: String,
    tx: Sender<ClientEvent>,
    write_stream: Arc<Mutex<TcpStream>>
)
{
    //RESET SEQs
    options::set_seq(0);
    options::set_server_seq(0);

    //DUPLICATE STREAM GUARDS
    let current_generation = AUDIO_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    let _guard = StreamGuard { generation: current_generation };

    //CONNECT
    let socket = Arc::new(UdpSocket::bind("0.0.0.0:0").expect("Binding UDP failed"));
    socket.connect(chat_options::get_server_address()).expect("Connecting to server UDP failed");

    //SET SOCKET TIMEOUT
    socket.set_read_timeout(Some(Duration::from_millis(200))).expect("Setting socket timeout failed");

    //INIT AUDIO HOST
    let host =
    {
        #[cfg(target_os = "linux")]
        {
            cpal::host_from_id(HostId::Alsa).unwrap_or_else(|_| cpal::default_host())
        }

        #[cfg(not(target_os = "linux"))]
        {
            cpal::default_host()
        }
    };

    //SUPPRESS STDERR (AVOID ALSA ERRORS)
    let stderr_gag = Gag::stderr().unwrap();

    //FIND INPUT/OUTPUT DEVICE
    let (input_device, output_device) = match find_devices(&host)
    {
        Some((input, output)) => (input, output), //FOUND
        _ => //NOT FOUND
        {
            //LEAVE VOICE
            command::send_command_code(&mut write_stream.lock().unwrap(), &Command::Voice, &None);
            return;
        }
    };

    //DISABLE SUPPRESSION
    drop(stderr_gag);

    //SEND HELLO PACKET
    voice::send(&socket, VoicePacket
    {
        id: Some(id),
        ..Default::default()
    }, &chat_options::get_keys().unwrap()).unwrap();

    //CONFIGURE CPAL INPUT
    let input_config = configure_device(input_device.supported_input_configs().unwrap(), input_device.default_input_config().unwrap());

    //CONFIGURE CPAL OUTPUT
    let output_config = configure_device(output_device.supported_output_configs().unwrap(), output_device.default_output_config().unwrap());

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
    let input_channels = input_config.channels as usize;
    let input_source_rate = input_config.sample_rate as f32;
    let input_target_rate = consts::SAMPLE_RATE as f32;

    //INPUT INTERPOLATION
    let input_resample_step = input_source_rate / input_target_rate;
    let mut input_resample_pos = 0.;

    //VAD
    let gate_open = Arc::new(Mutex::new(false)); //NOISE GATE
    let preroll_buffer = Arc::new(Mutex::new(VecDeque::<Vec<f32>>::with_capacity(3))); //PRE-ROLL BUFFER
    let hold_frames_remaining = Arc::new(Mutex::new(0usize)); //HOLD TIME

    //NOISE REDUCTION
    let mut denoiser = DenoiseState::new();
    let mut denoise_buffer = [0.0f32; consts::SAMPLE_RATE as usize / 100];

    //CONFIGURE INPUT STREAM
    let send_socket = socket.clone();
    let input_stream = input_device.build_input_stream(input_config, move |data: &[f32], _: &_|
    {
        //CHECK FOR MUTING
        if chat_options::is_muted(None)
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

            //NOISE REDUCTION
            for chunk in frame.chunks_mut(consts::SAMPLE_RATE as usize / 100)
            {
                if chunk.len() == consts::SAMPLE_RATE as usize / 100
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

            //VAD
            let rms = (frame.iter().map(|&x| x * x).sum::<f32>() / frame.len() as f32 + 1e-10).sqrt(); //RMS CALCULATION (+ SMALL BIAS)
            let mut gate = gate_open.lock().unwrap();
            let mut preroll = preroll_buffer.lock().unwrap();
            let mut hold_frames = hold_frames_remaining.lock().unwrap();

            //HYSTERESIS
            if !*gate && rms > consts::TRESHOLD_OPEN
            {
                *gate = true; //SPEAKING

                //SEND STORED FRAMES
                for old_frame in preroll.iter()
                {
                    transmit_audio(&opus_encoder, old_frame, &mut encoded_buffer, id, &send_socket);
                }

                preroll.clear();
                *hold_frames = consts::HOLD_FRAMES;
            } else if *gate && rms < consts::TRESHOLD_CLOSE
            {
                if *hold_frames > 0 //SILENT FRAME, DECREMENT
                {
                    *hold_frames -= 1;
                } else //HOLD TIME EXPIRED, CLOSE GATE
                {
                    *gate = false;
                }
            } else if *gate && rms >= consts::TRESHOLD_CLOSE //SPEAKING CONTINUES, RESET HOLD TIMER
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
                transmit_audio(&opus_encoder, &frame, &mut encoded_buffer, id, &send_socket);
            }
        }
    }, |_| {}, None).unwrap();

    //OUTPUT RESAMPLING
    let output_channels = output_config.channels as usize;
    let output_source_rate = consts::SAMPLE_RATE as f32;
    let output_target_rate = output_config.sample_rate as f32;

    //OUTPUT INTERPOLATION
    let output_resample_step = output_source_rate / output_target_rate;

    //CONFIGURE OUTPUT STREAM
    let output_stream = output_device.build_output_stream(output_config, move |data: &mut [f32], _: &_|
    {
        //CHECK GENERATION
        if AUDIO_GENERATION.load(Ordering::Relaxed) != current_generation { return; }

        //CLEAR OUTPUT BUFFER
        data.fill(0.);

        let frames_to_write = data.len() / output_channels;
        let mut consumers_guard = CONSUMERS.lock().unwrap();

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

            //SOFT CLIPPING (HYPERBOLIC TANGENT)
            mixed_sample = mixed_sample.tanh();

            //WRITE SAMPLE TO ALL CHANNELS
            for channel in 0..output_channels
            {
                data[i * output_channels + channel] = mixed_sample;
            }
        }
    }, |_| {}, None).unwrap();

    //RUN STREAMS
    input_stream.play().unwrap();  //INPUT
    output_stream.play().unwrap(); //OUTPUT

    //MOVE STREAMS TO GLOBAL STORAGE
    *LOCAL_STREAMS.lock().unwrap() = Some(LocalStream
    {
        _input: input_stream,
        _output: output_stream,
    });

    //PLAY JOIN SOUND
    sfx::clear_effects();
    sfx::queue_effect(SoundEffect::Join);

    //START VOICE ACTIVITY DISPLAY & PING THREAD
    let vad_socket = socket.clone();
    thread::spawn(move ||
    {
        let mut iteration_counter = 0u8;

        loop
        {
            //QUIT ON /leave
            if !options::get_use_voice()
            {
                tx.send(ClientEvent::VoiceActivity(Vec::new())).unwrap(); //CLEAR WINDOW
                return;
            }

            iteration_counter += 1; //INCREMENT

            //SHOW VOICE ACTIVITY
            display_active_speakers(&username, &tx);

            //SEND PING PACKET
            if iteration_counter == 10
            {
                voice::send(&vad_socket, VoicePacket
                {
                    id: Some(id),
                    code: Some(VoiceCode::PING),
                    timestamp: Some(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()),

                    ..Default::default()
                }, &chat_options::get_keys().unwrap()).unwrap();

                //RESET COUNTER
                iteration_counter = 0;
            }

            thread::sleep(Duration::from_millis(100));
        }
    });

    //OUTPUT BUFFERS
    let mut decoded_buffer = [0.0f32; consts::FRAME_SIZE];

    loop
    {
        //READ
        let (network_buffer, _) = match voice::receive(&socket)
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

                    thread::sleep(Duration::from_millis(50));
                }

                return;
            }
        };

        //VERIFY SERVER SEQ
        if network_buffer.seq <= options::get_server_seq() { continue; } //INGORE INVALID SEQs
        options::set_server_seq(network_buffer.seq); //SET SERVER SEQ

        //GET ID OF SENDER
        let sender_id = match network_buffer.id
        {
            Some(id) => id,
            None => continue
        };

        //CREATE NEW CLIENT CONTEXT ON UNKNOWN CLIENT
        if !CONSUMERS.lock().unwrap().contains_key(&sender_id)
        {
            add_consumer(sender_id, network_buffer.username.unwrap());
        }

        if let Some((stream, peer)) = CONSUMERS.lock().unwrap().get_mut(&sender_id)
        {
            //PING/PONG
            if let Some(code) = network_buffer.code && let Some(timestamp) = network_buffer.timestamp
            {
                match code
                {
                    //PING RECEIVED, SEND BACK
                    VoiceCode::PING =>
                    {
                        //SEND PONG PACKET
                        voice::send(&socket, VoicePacket
                        {
                            id: Some(id),
                            target_id: Some(sender_id),
                            code: Some(VoiceCode::PONG),
                            timestamp: Some(timestamp),

                            ..Default::default()
                        }, &chat_options::get_keys().unwrap()).unwrap();
                    },

                    //PING FORWARDED BACK, CALCULATE LATENCY
                    VoiceCode::PONG =>
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
                    }
                }
            }

            //CHECK FOR VOICE IN PACKET
            if network_buffer.voice.is_none() { continue; }

            //CHECK FOR MUTED CLIENT
            if chat_options::is_muted(Some(sender_id)) { continue; }

            //DECODE
            if let Ok(decoded_len) = peer.decoder.decode_float(network_buffer.voice.as_deref(), &mut decoded_buffer[..], false)
            {
                //PUSH TO RINGBUFFER
                peer.producer.push_slice(&decoded_buffer[..decoded_len]);
            }
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

fn display_active_speakers(local_username: &str, tx: &Sender<ClientEvent>)
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
    tx.send(ClientEvent::VoiceActivity(users_to_display)).unwrap();
}
