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

use std::net::UdpSocket;

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
    coder::Encoder,
};

use crate::chat::
{
    options as chat_options,
    network::voice::{ self, options },
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
pub fn listen_server_voice()
{
    //CONNECT
    let socket = UdpSocket::bind("0.0.0.0:0").expect("Binding UDP failed");
    socket.connect(chat_options::get_server_address()).expect("Connecting to server UDP failed");

    //INIT AUDIO HOST
    let host = cpal::default_host();

    //FIND INPUT DEVICE
    let input_device = find_device(host.input_devices().expect("No input device found"))
        .or_else(|| host.default_input_device()).unwrap();

    //CONFIGURE CPAL INPUT
    let mut input_config: StreamConfig = input_device.default_input_config().unwrap().into();
    input_config.sample_rate = options::SAMPLE_RATE;
    input_config.channels = 1;

    //PREPARE OPUS ENCODER
    let opus_encoder = Encoder::new
    (
        SampleRate::Hz48000,
        Channels::Mono,
        Application::Voip
    ).unwrap();

    //INPUT BUFFERS
    let mut input_accum: Vec<f32> = Vec::with_capacity(options::FRAME_SIZE * 2);
    let mut encoded_buffer = [0u8; 1500]; //ALLOCATE BUFFER TO STANDARD MTU

    //CONFIGURE INPUT STREAM
    let input_stream = input_device.build_input_stream(&input_config, move |data: &[f32], _: &_|
    {
        //ACCUMULATE
        input_accum.extend_from_slice(data);

        //PROCESS
        while input_accum.len() >= options::FRAME_SIZE
        {
            let frame: Vec<f32> = input_accum.drain(0..options::FRAME_SIZE).collect();

            //ENCODE
            match opus_encoder.encode_float(&frame, &mut encoded_buffer)
            {
                Ok(len) =>
                {
                    voice::send(&socket, &encoded_buffer[..len]).unwrap();
                },
                Err(_) => {}, //IGNORE ENCODER ERRORS
            }
        }
    }, |_| {}, None).unwrap();

    input_stream.play().unwrap();
    loop {}
}
