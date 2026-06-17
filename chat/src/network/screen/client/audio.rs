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
    time::Duration,
    sync::
    {
        Arc,
        atomic::{ AtomicBool, Ordering },
    },
};

use cpal::
{
    StreamConfig,
    traits::
    {
        DeviceTrait,
        HostTrait,
        StreamTrait,
    },
};

use crossbeam_channel::{ Sender, Receiver };

use audiopus::
{
    SampleRate,
    Channels,
    Application,
    TryFrom,
    coder::{ Encoder, Decoder },
};

use crate::network::voice::
{
    consts,
    client as voice,
};

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
        if cfg!(target_os = "macos")
        {
            while running.load(Ordering::Relaxed)
            {
                thread::sleep(Duration::from_millis(100));
            }
            return;
        }

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

        let mut config: StreamConfig = if cfg!(target_os = "windows")
        {
            voice::configure_device(device.supported_output_configs().unwrap(), device.default_output_config().unwrap())
        } else
        {
            voice::configure_device(device.supported_input_configs().unwrap(), device.default_input_config().unwrap())
        };

        config.buffer_size = cpal::BufferSize::Fixed(960);
        let opus_frame_samples = 1920;

        let encoder = Encoder::new
        (
            <SampleRate as TryFrom<i32>>::try_from(consts::SAMPLE_RATE as i32).unwrap(),
            Channels::Stereo,
            Application::LowDelay,
        ).unwrap();

        let (chunk_tx, chunk_rx) = crossbeam_channel::bounded::<Vec<f32>>(8);

        let input_channels = config.channels as usize;
        let input_source_rate = config.sample_rate as f32;
        let input_target_rate = consts::SAMPLE_RATE as f32;
        let input_resample_step = input_source_rate / input_target_rate;
        let mut input_resample_pos = 0.;

        let mut input_accum: Vec<f32> = Vec::with_capacity(opus_frame_samples * 2);

        let stream = device.build_input_stream(config.clone(), move |data: &[f32], _: &cpal::InputCallbackInfo|
        {
            let frames_in_buffer = data.len() / input_channels;
            let mut chunk = Vec::with_capacity((frames_in_buffer as f32 / input_resample_step * 2.0) as usize + 2);

            let get_stereo_sample = |index: usize| -> (f32, f32)
            {
                if index >= frames_in_buffer { return (0., 0.) }

                if input_channels >= 2
                {
                    (data[index * input_channels], data[index * input_channels + 1])
                } else if input_channels == 1
                {
                    let s = data[index];
                    (s, s)
                } else {
                    (0., 0.)
                }
            };

            while input_resample_pos < (frames_in_buffer as f32) - 1.
            {
                let idx = input_resample_pos.floor() as usize;
                let frac = input_resample_pos - idx as f32;

                let (l0, r0) = get_stereo_sample(idx);
                let (l1, r1) = get_stereo_sample(idx + 1);

                let interpolated_l = l0 + (l1 - l0) * frac;
                let interpolated_r = r0 + (r1 - r0) * frac;

                chunk.push(interpolated_l);
                chunk.push(interpolated_r);

                input_resample_pos += input_resample_step;
            }

            input_resample_pos -= frames_in_buffer as f32;

            if !chunk.is_empty()
            {
                chunk_tx.try_send(chunk).ok();
            }
        }, |_| {}, None).unwrap();

        stream.play().unwrap();

        let mut out = vec![0u8; 4000];

        while running.load(Ordering::Relaxed)
        {
            match chunk_rx.recv_timeout(Duration::from_millis(100))
            {
                Ok(chunk) => input_accum.extend_from_slice(&chunk),
                Err(_) =>
                {
                    if !running.load(Ordering::Relaxed) { return; }
                    continue;
                },
            }

            //DRAIN ANY QUEUED CHUNKS
            while let Ok(chunk) = chunk_rx.try_recv()
            {
                input_accum.extend_from_slice(&chunk);
            }

            //ENCODE OPUS FRAMES
            while input_accum.len() >= opus_frame_samples
            {
                match encoder.encode_float(&input_accum[..opus_frame_samples], &mut out)
                {
                    Ok(len) =>
                    {
                        if tx.send(AudioFrame { data: out[..len].to_vec() }).is_err() { return; }
                    },

                    _ => {},
                }
                input_accum.drain(..opus_frame_samples);
            }
        }
    })
}

pub fn spawn_audio_playback
(
    rx: Receiver<AudioFrame>,
    running: Arc<AtomicBool>,
) -> thread::JoinHandle<()>
{
    thread::spawn(move ||
        {
        let host = cpal::default_host();
        let device = host.default_output_device().expect("No audio output device found");
        let mut config: StreamConfig = voice::configure_device(device.supported_output_configs().unwrap(), device.default_output_config().unwrap());
        config.buffer_size = cpal::BufferSize::Fixed(960);

        let mut decoder = Decoder::new(<SampleRate as TryFrom<i32>>::try_from(48000).unwrap(), Channels::Stereo).unwrap();
        let opus_frame_samples = 1920;

        let (chunk_tx, chunk_rx) = crossbeam_channel::bounded::<Vec<f32>>(3);

        let mut drain_buf: Vec<f32> = Vec::new();
        let mut drain_pos: usize = 0;

        let output_channels = config.channels as usize;
        let output_source_rate = consts::SAMPLE_RATE as f32;
        let output_target_rate = config.sample_rate as f32;
        let output_resample_step = output_source_rate / output_target_rate;

        let mut resample_pos = 0.;
        let mut current_frame = (0., 0.);
        let mut next_frame = (0., 0.);

        let stream = device.build_output_stream(config.clone(), move |data: &mut [f32], _: &cpal::OutputCallbackInfo|
        {
            let frames_to_write = data.len() / output_channels;
            for i in 0..frames_to_write
            {
                while resample_pos >= 1.
                {
                    current_frame = next_frame;

                    if drain_pos < drain_buf.len()
                    {
                        next_frame = (drain_buf[drain_pos], drain_buf[drain_pos + 1]);
                        drain_pos += 2;
                    } else
                    {
                        match chunk_rx.try_recv()
                        {
                            Ok(chunk) =>
                            {
                                drain_buf = chunk;
                                drain_pos = 0;

                                if drain_buf.len() >= 2
                                {
                                    next_frame = (drain_buf[drain_pos], drain_buf[drain_pos + 1]);
                                    drain_pos += 2;
                                } else
                                {
                                    next_frame = (0., 0.);
                                }
                            },

                            Err(_) =>
                            {
                                next_frame = (0., 0.);
                            }
                        }
                    }
                    resample_pos -= 1.;
                }

                let interpolated_l = current_frame.0 + (next_frame.0 - current_frame.0) * resample_pos;
                let interpolated_r = current_frame.1 + (next_frame.1 - current_frame.1) * resample_pos;
                resample_pos += output_resample_step;

                if output_channels >= 2
                {
                    data[i * output_channels] = interpolated_l;
                    data[i * output_channels + 1] = interpolated_r;
                    for c in 2..output_channels
                    {
                        data[i * output_channels + c] = 0.0;
                    }
                } else if output_channels == 1
                {
                    data[i] = (interpolated_l + interpolated_r) * 0.5;
                }
            }
        }, |_| {}, None).unwrap();

        stream.play().unwrap();

        let mut out = vec![0f32; opus_frame_samples];

        while running.load(Ordering::Relaxed)
        {
            match rx.recv_timeout(Duration::from_millis(50))
            {
                Ok(frame) =>
                {
                    match decoder.decode_float(Some(&frame.data[..]), &mut out, false)
                    {
                        Ok(len) =>
                        {
                            let decoded = out[..len * 2].to_vec();
                            if chunk_tx.send(decoded).is_err() { return; } //CHANNEL CLOSED
                        },
                        _ => {},
                    }
                },
                _ => {}
            }
        }
    })
}
