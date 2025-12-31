/*
This is part of WHY2
Copyright (C) 2022-2025 Václav Šmejkal

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
    sync::Arc,
    net::UdpSocket,
};

use cpal::
{
    Device,
    StreamConfig,
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
    coder::{ Encoder, Decoder },
};

use ringbuf::
{
    HeapRb,
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

//PUBLIC
pub fn listen_server_voice(id: usize)
{
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
    let mut stream_config: StreamConfig = input_device.default_input_config().unwrap().into();
    stream_config.sample_rate = options::SAMPLE_RATE;
    stream_config.channels = 1;

    //PREPARE OPUS ENCODER
    let opus_encoder = Encoder::new
    (
        SampleRate::Hz48000,
        Channels::Mono,
        Application::Voip
    ).unwrap();

    let mut opus_decoder = Decoder::new
    (
        SampleRate::Hz48000,
        Channels::Mono,
    ).unwrap();

    //INPUT BUFFERS
    let mut input_accum: Vec<f32> = Vec::with_capacity(options::FRAME_SIZE * 2);
    let mut encoded_buffer = [0u8; 1500]; //ALLOCATE BUFFER TO STANDARD MTU

    //CONFIGURE INPUT STREAM
    let send_socket = socket.clone();
    let input_stream = input_device.build_input_stream(&stream_config, move |data: &[f32], _: &_|
    {
        //ACCUMULATE
        input_accum.extend_from_slice(data);

        //PROCESS
        while input_accum.len() >= options::FRAME_SIZE
        {
            let frame: Vec<f32> = input_accum.drain(0..options::FRAME_SIZE).collect();

            //ENCODE (IGNORE ERRORS)
            if let Ok(len) = opus_encoder.encode_float(&frame, &mut encoded_buffer)
            {
                voice::send(&send_socket, VoicePacket
                {
                    voice: encoded_buffer[..len].to_vec(),
                    id: Some(id),

                    ..Default::default()
                }, &chat_options::get_keys().unwrap()).unwrap();
            }
        }
    }, |_| {}, None).unwrap();

    //JITTER BUFFER
    let rb = HeapRb::<f32>::new(options::FRAME_SIZE * 20); //~400ms BUFFER
    let (mut producer, mut consumer) = rb.split();

    //CONFIGURE OUTPUT STREAM
    let output_stream = output_device.build_output_stream(&stream_config, move |data: &mut [f32], _: &_|
    {
        let read_count = consumer.pop_slice(data);

        //FILL WITH SILENCE ON UNDERRUN
        if read_count < data.len()
        {
            for i in read_count..data.len()
            {
                data[i] = 0.0;
            }
        }
    }, |_| {}, None).unwrap();

    //RUN STREAMS
    input_stream.play().unwrap();  //INPUT
    output_stream.play().unwrap(); //OUTPUT

    //OUTPUT BUFFERS
    let mut network_buffer: VoicePacket;
    let mut decoded_buffer = [0.0f32; options::FRAME_SIZE];

    loop
    {
        //READ
        network_buffer = match voice::receive(&socket)
        {
            Some(r) => r.0,
            None => return
        };

        //VERIFY SERVER SEQ
        if network_buffer.seq <= options::get_server_seq() { continue; } //INGORE INVALID SEQs
        options::set_server_seq(network_buffer.seq); //SET SERVER SEQ

        //DECODE
        if let Ok(decoded_len) = opus_decoder.decode_float(Some(&network_buffer.voice), &mut decoded_buffer[..], false)
        {
            //PUSH TO RINGBUFFER
            producer.push_slice(&decoded_buffer[..decoded_len]);
        }
    }
}
