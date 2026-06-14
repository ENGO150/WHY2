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
    thread::{ self, JoinHandle },
    time::{ Duration, Instant },
    sync::
    {
        Arc,
        atomic::{ AtomicBool, Ordering },
    },
};

use cpal::
{
    StreamConfig,
    BufferSize,
    traits::
    {
        DeviceTrait,
        HostTrait,
        StreamTrait,
    },
};

use crossbeam_channel::Sender;

use audiopus::
{
    SampleRate,
    Channels,
    Application,
    TryFrom,
    coder::Encoder,
};

use crate::network::voice::consts;

//STRUCTS
pub struct AudioFrame
{
    pub data: Vec<u8>,
}

//FUNCTIONS
pub fn spawn_audio_capture(tx: Sender<AudioFrame>, running: Arc<AtomicBool>) -> JoinHandle<()> //CAPTURE AUDIO
{
    thread::spawn(move ||
    {
        let host = cpal::default_host();

        let device = if cfg!(target_os = "linux")
        {
            let default_out = host.default_output_device().expect("No audio output device found");
            let monitor_name = format!("Monitor of {}", default_out.to_string());

            let mut target_device = None;
            if let Ok(devices) = host.input_devices()
            {
                for d in devices
                {
                    if d.to_string() == monitor_name
                    {
                        target_device = Some(d);
                        break;
                    }
                }
            }

            target_device.unwrap_or_else(|| host.default_input_device().expect("No input device found either"))
        } else if cfg!(target_os = "windows")
        {
            host.default_output_device().expect("No audio output device found for Windows loopback")
        } else
        {
            host.default_input_device().expect("No microphone found")
        };

        //TODO: Implement dynamic config
        let config = StreamConfig
        {
            channels: 2,
            sample_rate: 48000,
            buffer_size: BufferSize::Fixed(480), // 10ms at 48kHz
        };
        let opus_frame_samples = 1920; // 960 frames * 2 channels = 20ms

        let encoder = Encoder::new
        (
            <SampleRate as TryFrom<i32>>::try_from(consts::SAMPLE_RATE as i32).unwrap(),
            Channels::Stereo,
            Application::LowDelay,
        ).unwrap();

        let (chunk_tx, chunk_rx) = crossbeam_channel::bounded::<Vec<f32>>(8);

        let stream = device.build_input_stream
        (
            config.clone(),
            move |data: &[f32], _: &cpal::InputCallbackInfo|
            {
                let _ = chunk_tx.try_send(data.to_vec());
            },
            |_| {},
            None,
        ).unwrap();

        stream.play().unwrap();

        let mut accum = Vec::with_capacity(opus_frame_samples * 2);
        let mut out = vec![0u8; 4000];
        let mut last_log = Instant::now();

        while running.load(Ordering::Relaxed)
        {
            match chunk_rx.recv_timeout(Duration::from_millis(100))
            {
                Ok(chunk) => accum.extend_from_slice(&chunk),
                Err(_) =>
                {
                    if !running.load(Ordering::Relaxed) { return; }
                    continue;
                },
            }

            //DRAIN ANY QUEUED CHUNKS
            while let Ok(chunk) = chunk_rx.try_recv()
            {
                accum.extend_from_slice(&chunk);
            }

            //ENCODE OPUS FRAMES
            while accum.len() >= opus_frame_samples
            {
                match encoder.encode_float(&accum[..opus_frame_samples], &mut out)
                {
                    Ok(len) =>
                    {
                        if tx.send(AudioFrame { data: out[..len].to_vec() }).is_err() { return; }
                    },

                    Err(e) => eprintln!("[audio] Opus encode error: {}", e),
                }
                accum.drain(..opus_frame_samples);
            }

            if last_log.elapsed() >= Duration::from_secs(3)
            {
                last_log = Instant::now();
            }
        }
    })
}
