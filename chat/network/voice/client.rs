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
    net::UdpSocket,
    collections::HashMap,
    sync::{ Arc, Mutex },
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

use gag::Gag;

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
}

struct PeerData
{
    decoder: Decoder,        //DECODER
    producer: HeapProd<f32>, //RINGBUFFER WRITER
}

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

//PUBLIC
pub fn listen_server_voice(id: usize)
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

    //CONFIGURE INPUT STREAM
    let send_socket = socket.clone();
    let input_stream = input_device.build_input_stream(&input_config, move |data: &[f32], _: &_|
    {
        let frames_in_buffer = data.len() / input_channels;

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
            let frame: Vec<f32> = input_accum.drain(0..options::FRAME_SIZE).collect();

            //ENCODE (IGNORE ERRORS)
            if let Ok(len) = opus_encoder.encode_float(&frame, &mut encoded_buffer)
            {
                //TRANSMIT
                voice::send(&send_socket, VoicePacket
                {
                    voice: encoded_buffer[..len].to_vec(),
                    id: Some(id),

                    ..Default::default()
                }, &chat_options::get_keys().unwrap()).unwrap();
            }
        }
    }, |_| {}, None).unwrap();

    let consumers = Arc::new(Mutex::new(HashMap::<usize, RemoteStream>::new()));
    let consumers_cloned = consumers.clone();

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
        let mut consumers_guard = consumers_cloned.lock().unwrap();

        for i in 0..frames_to_write
        {
            let mut mixed_sample = 0.;

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

                //MIX
                mixed_sample += interpolated;
            }

            //CLIPPING PROTECTION
            mixed_sample = mixed_sample.clamp(-1., 1.);

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
        if !peers.contains_key(&sender_id)
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
            consumers.lock().unwrap().insert(sender_id, RemoteStream
            {
                consumer: consumer,
                resample_pos: 0.,
                current_sample: 0.,
                next_sample: first_sample,
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
