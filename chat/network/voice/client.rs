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
    thread,
    time::Duration,
    net::UdpSocket,
    io::{ self, Write },
    collections::{ HashMap, VecDeque },
    sync::
    {
        Arc,
        Mutex,
        LazyLock,
        atomic::{ AtomicUsize, Ordering },
    },
};

use cpal::
{
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
use crossterm::
{
    terminal,
    QueueableCommand,
    style::
    {
        Print,
        SetForegroundColor,
        Color,
        ResetColor,
        SetAttribute,
        Attribute
    },
    cursor::
    {
        MoveTo,
        SavePosition,
        RestorePosition,
    },
};

use crate::chat::
{
    options as chat_options,
    network::voice::
    {
        self,
        options,
        VoicePacket,
    },
};

//STRUCTS
struct RemoteStream
{
    consumer: HeapCons<f32>, //RINGBUFFER READER
    resample_pos: f32,       //POSITION IN BETWEEN SAMPLES
    current_sample: f32,     //CURRENT SAMPLE FOR INTERPOLATION
    next_sample: f32,        //NEXT SAMPLE FOR INTERPOLATION
    activity_hold: usize,    //ACTIVITY TIMER
    display_hold: usize,     //ACTIVITY WINDOW TIMER
    username: String,        //USERNAME
}

struct PeerData
{
    decoder: Decoder,        //DECODER
    producer: HeapProd<f32>, //RINGBUFFER WRITER
}

//GLOBAL VARIABLES
static LOCAL_DISPLAY_HOLD: AtomicUsize = AtomicUsize::new(0);
static CONSUMERS: LazyLock<Mutex<HashMap<usize, RemoteStream>>> = LazyLock::new(|| Mutex::new(HashMap::new())); //OTHER CLIENTS

//PRIVATE
fn find_device(mut devices: impl Iterator<Item = Device>) -> Option<Device>
{
    devices.find(|d|
    {
        if let Ok(desc) = d.description()
        {
            let name = desc.to_string().to_lowercase();
            name.contains("pipewire") || name.contains("pulse")
        } else { false }
    })
}

fn configure_device(supported_configs: impl Iterator<Item = SupportedStreamConfigRange>, default_config: SupportedStreamConfig) -> StreamConfig
{
    supported_configs
        .filter(|c| c.min_sample_rate() <= options::SAMPLE_RATE && c.max_sample_rate() >= options::SAMPLE_RATE)
        .next()
        .map(|c| c.with_sample_rate(options::SAMPLE_RATE))
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
            voice: buffer[..len].to_vec(),
            id: Some(id),

            ..Default::default()
        }, &chat_options::get_keys().unwrap()).unwrap();
    }
}

//PUBLIC
pub fn listen_server_voice(id: usize, username: String)
{
    //RESET SEQs
    options::set_seq(0);
    options::set_server_seq(0);

    //CONNECT
    let socket = Arc::new(UdpSocket::bind("0.0.0.0:0").expect("Binding UDP failed"));
    socket.connect(chat_options::get_server_address()).expect("Connecting to server UDP failed");

    //INIT AUDIO HOST
    let host = cpal::default_host();

    //SUPPRESS STDERR (AVOID ALSA ERRORS)
    let stderr_gag = Gag::stderr().unwrap();

    //FIND INPUT DEVICE
    let input_device = find_device(host.input_devices().expect("No input device found"))
        .or_else(|| host.default_input_device()).unwrap();

    //FIND OUTPUT DEVICE
    let output_device = find_device(host.output_devices().expect("No output device found"))
        .or_else(|| host.default_output_device()).unwrap();

    //DISABLE SUPPRESSION
    drop(stderr_gag);

    //CONFIGURE CPAL INPUT
    let input_config = configure_device(input_device.supported_input_configs().unwrap(), input_device.default_input_config().unwrap());

    //CONFIGURE CPAL OUTPUT
    let output_config = configure_device(output_device.supported_output_configs().unwrap(), output_device.default_output_config().unwrap());

    //PREPARE OPUS ENCODER
    let opus_encoder = Encoder::new
    (
        <SampleRate as TryFrom<i32>>::try_from(options::SAMPLE_RATE as i32).unwrap(),
        Channels::Mono,
        Application::Voip
    ).unwrap();

    //INPUT BUFFERS
    let mut input_accum: Vec<f32> = Vec::with_capacity(options::FRAME_SIZE * 2);
    let mut encoded_buffer = [0u8; 1500]; //ALLOCATE BUFFER TO STANDARD MTU

    //INPUT RESAMPLING
    let input_channels = input_config.channels as usize;
    let input_source_rate = input_config.sample_rate as f32;
    let input_target_rate = options::SAMPLE_RATE as f32;

    //INPUT INTERPOLATION
    let input_resample_step = input_source_rate / input_target_rate;
    let mut input_resample_pos = 0.;

    //VAD
    let gate_open = Arc::new(Mutex::new(false)); //NOISE GATE
    let preroll_buffer = Arc::new(Mutex::new(VecDeque::<Vec<f32>>::with_capacity(3))); //PRE-ROLL BUFFER
    let hold_frames_remaining = Arc::new(Mutex::new(0usize)); //HOLD TIME

    //NOISE REDUCTION
    let mut denoiser = DenoiseState::new();
    let mut denoise_buffer = [0.0f32; options::SAMPLE_RATE as usize / 100];

    //CONFIGURE INPUT STREAM
    let send_socket = socket.clone();
    let input_stream = input_device.build_input_stream(&input_config, move |data: &[f32], _: &_|
    {
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
        while input_accum.len() >= options::FRAME_SIZE
        {
            let mut frame: Vec<f32> = input_accum.drain(0..options::FRAME_SIZE).collect();

            //NOISE REDUCTION
            for chunk in frame.chunks_mut(options::SAMPLE_RATE as usize / 100)
            {
                if chunk.len() == options::SAMPLE_RATE as usize / 100
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
            if !*gate && rms > options::TRESHOLD_OPEN
            {
                *gate = true; //SPEAKING

                //SEND STORED FRAMES
                for old_frame in preroll.iter()
                {
                    transmit_audio(&opus_encoder, old_frame, &mut encoded_buffer, id, &send_socket);
                }

                preroll.clear();
                *hold_frames = options::HOLD_FRAMES;
            } else if *gate && rms < options::TRESHOLD_CLOSE
            {
                if *hold_frames > 0 //SILENT FRAME, DECREMENT
                {
                    *hold_frames -= 1;
                } else //HOLD TIME EXPIRED, CLOSE GATE
                {
                    *gate = false;
                }
            } else if *gate && rms >= options::TRESHOLD_CLOSE //SPEAKING CONTINUES, RESET HOLD TIMER
            {
                *hold_frames = options::HOLD_FRAMES;
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
                LOCAL_DISPLAY_HOLD.store(options::SAMPLE_RATE as usize, Ordering::Relaxed);
                transmit_audio(&opus_encoder, &frame, &mut encoded_buffer, id, &send_socket);
            }
        }
    }, |_| {}, None).unwrap();

    let mut peers: HashMap<usize, PeerData> = HashMap::new();

    //OUTPUT RESAMPLING
    let output_channels = output_config.channels as usize;
    let output_source_rate = options::SAMPLE_RATE as f32;
    let output_target_rate = output_config.sample_rate as f32;

    //OUTPUT INTERPOLATION
    let output_resample_step = output_source_rate / output_target_rate;

    //CONFIGURE OUTPUT STREAM
    let output_stream = output_device.build_output_stream(&output_config, move |data: &mut [f32], _: &_|
    {
        //CLEAR OUTPUT BUFFER
        data.fill(0.);

        let frames_to_write = data.len() / output_channels;
        let mut consumers_guard = CONSUMERS.lock().unwrap();

        for i in 0..frames_to_write
        {
            let mut mixed_sample = 0.;
            let mut active_speakers = 0;

            for stream in consumers_guard.values_mut()
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
                if interpolated.abs() > options::MIXING_TRESHOLD
                {
                    stream.activity_hold = options::SAMPLE_RATE as usize / 10; //SET TIMER TO ~100ms
                    stream.display_hold = options::SAMPLE_RATE as usize; //SET DISPLAY FOR ~1000ms
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

    //START VOICE ACTIVITY DISPLAY THREAD
    thread::spawn(move ||
    {
        loop
        {
            if !options::get_use_voice() { return; } //QUIT ON /leave

            display_active_speakers(&username); //SHOW VOICE ACTIVITY
            thread::sleep(Duration::from_millis(60));
        }
    });

    //OUTPUT BUFFERS
    let mut decoded_buffer = [0.0f32; options::FRAME_SIZE];

    loop
    {
        //READ
        let (network_buffer, _) = match voice::receive(&socket)
        {
            Some(r) => r,
            None => return
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
        if !peers.contains_key(&sender_id) || !CONSUMERS.lock().unwrap().contains_key(&sender_id)
        {
            //OPUS DECODER
            let decoder = Decoder::new
            (
                <SampleRate as TryFrom<i32>>::try_from(options::SAMPLE_RATE as i32).unwrap(),
                Channels::Mono,
            ).unwrap();

            //JITTER BUFFER
            let rb = HeapRb::<f32>::new(options::FRAME_SIZE * 20);
            let (producer, mut consumer) = rb.split();

            let first_sample = consumer.try_pop().unwrap_or(0.0);

            //INSERT TO LOCAL MAP
            peers.insert(sender_id, PeerData
            {
                decoder: decoder,
                producer: producer,
            });

            //INSERT TO SHARED AUDIO THREAD MAP
            CONSUMERS.lock().unwrap().insert(sender_id, RemoteStream
            {
                consumer: consumer,
                resample_pos: 0.,
                current_sample: 0.,
                next_sample: first_sample,
                activity_hold: 0,
                display_hold: 0,
                username: network_buffer.username.unwrap(),
            });
        }

        if let Some(peer) = peers.get_mut(&sender_id)
        {
            //DECODE
            if let Ok(decoded_len) = peer.decoder.decode_float(Some(&network_buffer.voice), &mut decoded_buffer[..], false)
            {
                //PUSH TO RINGBUFFER
                peer.producer.push_slice(&decoded_buffer[..decoded_len]);
            }
        }
    }
}

pub fn remove_consumer(id: &usize)
{
    CONSUMERS.lock().unwrap().remove(id);
}

fn display_active_speakers(local_username: &str)
{
    //HELPER STRUCT
    struct DisplayUser
    {
        username: String,
        is_speaking: bool,
    }

    //ALL USERS
    let mut users_to_display = Vec::new();

    //ADD LOCAL CLIENT
    let local_speaking = LOCAL_DISPLAY_HOLD.load(Ordering::Relaxed) > 0;
    users_to_display.push(DisplayUser
    {
        username: local_username.to_string(),
        is_speaking: local_speaking,
    });

    //COLLECT OTHER USERS
    if let Ok(consumers) = CONSUMERS.try_lock()
    {
        for (_, stream) in consumers.iter()
        {
            users_to_display.push(DisplayUser
            {
                username: stream.username.clone(),
                is_speaking: stream.display_hold > 0, //SPEAKING
            });
        }
    }

    //SORT
    users_to_display.sort_by(|a, b|
    {
        if a.is_speaking == b.is_speaking
        {
            a.username.cmp(&b.username)
        } else
        {
            b.is_speaking.cmp(&a.is_speaking)
        }
    });

    //PREPARE TERMINAL
    let mut stdout = io::stdout();
    let (cols, rows) = terminal::size().unwrap_or((80, 24));

    stdout.queue(SavePosition).unwrap();

    let overlay_width = 25;
    let bottom_row = rows.saturating_sub(2);
    let available_height = rows.saturating_sub(4) as usize;
    let limit = available_height.min(15);

    let header_text = "VOICE CHANNEL:"; //HEADER
    let mut max_content_width = header_text.len();

    //FIND WIDEST LINE
    for user in users_to_display.iter().take(limit)
    {
        let width = user.username.chars().count() + 3;
        if width > max_content_width
        {
            max_content_width = width;
        }
    }

    let clear_width = overlay_width.max(max_content_width);
    let align_x = cols.saturating_sub(max_content_width as u16).saturating_sub(1);

    //CLEAR WINDOW
    for i in 0..=limit
    {
        let y = bottom_row.saturating_sub(i as u16);
        let x = cols.saturating_sub(clear_width as u16);
        stdout.queue(MoveTo(x, y)).unwrap();
        stdout.queue(Print(" ".repeat(clear_width as usize))).unwrap();
    }

    //PRINT
    for (i, user) in users_to_display.iter().take(limit).enumerate()
    {
        let y = bottom_row.saturating_sub(i as u16);
        let text = format!("- {} ", user.username);

        stdout.queue(MoveTo(align_x, y)).unwrap();

        if user.is_speaking
        {
            //ACTIVE
            stdout.queue(SetForegroundColor(Color::Green)).unwrap();
            stdout.queue(SetAttribute(Attribute::Bold)).unwrap();
            stdout.queue(Print(text)).unwrap();
            stdout.queue(SetAttribute(Attribute::Reset)).unwrap();
            stdout.queue(ResetColor).unwrap();
        } else
        {
            //INACTIVE
            stdout.queue(SetForegroundColor(Color::DarkGrey)).unwrap();
            stdout.queue(Print(text)).unwrap();
            stdout.queue(ResetColor).unwrap();
        }
    }

    //HEADER PRINT
    if !users_to_display.is_empty()
    {
        let count = users_to_display.len().min(limit);
        let y = bottom_row.saturating_sub(count as u16);

        stdout.queue(MoveTo(align_x, y)).unwrap();
        stdout.queue(SetAttribute(Attribute::Underlined)).unwrap();
        stdout.queue(Print(header_text)).unwrap();
        stdout.queue(SetAttribute(Attribute::Reset)).unwrap();
    }

    stdout.queue(RestorePosition).unwrap();
    stdout.flush().unwrap();
}
