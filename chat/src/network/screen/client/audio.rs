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
    time::Duration,
    sync::
    {
        Arc,
        atomic::{ AtomicBool, Ordering },
    },
};

use tokio::
{
    time,
    sync::mpsc::
    {
        self,
        Sender,
        Receiver,
    },
};

use cpal::
{
    BufferSize,
    StreamConfig,
    InputCallbackInfo,
    OutputCallbackInfo,
    traits::
    {
        DeviceTrait,
        HostTrait,
        StreamTrait,
    },
};

use audiopus::
{
    SampleRate,
    Channels,
    Application,
    TryFrom,
    coder::{ Encoder, Decoder },
};

use crate::network::
{
    screen::
    {
        client::options,
        consts as screen_consts,
    },
    voice::
    {
        consts,
        client as voice,
        client::aec,
    },
};

//STRUCTS
pub struct AudioFrame
{
    pub data: Vec<u8>,
}

//FUNCTIONS
pub async fn spawn_audio_capture(tx: Sender<AudioFrame>, running: Arc<AtomicBool>) //CAPTURE AUDIO
{
    if cfg!(target_os = "macos")
    {
        while running.load(Ordering::Relaxed)
        {
            time::sleep(Duration::from_millis(100)).await;
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
        voice::configure_device(&device, device.supported_output_configs().unwrap(), device.default_output_config().unwrap(), true)
    } else
    {
        voice::configure_device(&device, device.supported_input_configs().unwrap(), device.default_input_config().unwrap(), true)
    };

    config.buffer_size = BufferSize::Fixed(screen_consts::BUFFER_SIZE);

    let encoder = Encoder::new
    (
        <SampleRate as TryFrom<i32>>::try_from(consts::SAMPLE_RATE as i32).unwrap(),
        Channels::Stereo,
        Application::LowDelay,
    ).unwrap();

    let (chunk_tx, mut chunk_rx) = mpsc::channel::<Vec<f32>>(screen_consts::CAPTURE_CHANNEL_BOUND);

    let input_channels = config.channels as usize;
    let input_source_rate = config.sample_rate as f32;
    let input_target_rate = consts::SAMPLE_RATE as f32;
    let input_resample_step = input_source_rate / input_target_rate;
    let mut input_resample_pos = 0.;

    let mut input_accum: Vec<f32> = Vec::with_capacity(screen_consts::FRAME_SAMPLES * 2);

    let stream = device.build_input_stream(config.clone(), move |data: &[f32], _: &InputCallbackInfo|
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
            //A CHUNK THE CHANNEL COULD NOT HOLD IS DROPPED - THE CAPTURE TASK IS BLOCKED ON THE NETWORK,
            //AND A REALTIME CALLBACK CANNOT WAIT FOR IT. THE CANCELLER LINES THE REFERENCE UP AGAINST THE
            //CAPTURE BY COUNT, THOUGH, SO IT HAS TO BE TOLD: FRAMES THAT VANISH HERE AND NOWHERE ELSE
            //SHIFT ITS ALIGNMENT BY EXACTLY THIS MANY, WHICH IS WHY THE ECHO COMES BACK ON A BAD LINK.
            if let Err(error) = chunk_tx.try_send(chunk)
            {
                aec::skip_reference(error.into_inner().len() / 2);
            }
        }
    }, |_| {}, None).unwrap();

    stream.play().unwrap();

    //TAKE OUR OWN VOICE PLAYBACK BACK OUT OF THE LOOPBACK, SO A VIEWER IN THE SAME CHANNEL DOES NOT HEAR
    //THEMSELVES. DROPPING THIS UNINSTALLS THE TAP, INCLUDING ON EVERY EARLY RETURN BELOW.
    let mut canceller = aec::start();

    let mut out = vec![0u8; screen_consts::MAX_PACKET_SIZE];

    while running.load(Ordering::Relaxed)
    {
        //EXIT ON DISABLED SCREEN
        if !options::get_use_screen()
        {
            running.store(false, Ordering::Relaxed);
            return;
        }

        match time::timeout(Duration::from_millis(100), chunk_rx.recv()).await
        {
            Ok(Some(mut chunk)) =>
            {
                if let Some(canceller) = canceller.as_mut() { canceller.process(&mut chunk); }

                input_accum.extend_from_slice(&chunk);
            },

            _ =>
            {
                if !running.load(Ordering::Relaxed) { return; }
                continue;
            },
        }

        //DRAIN ANY QUEUED CHUNKS
        while let Ok(mut chunk) = chunk_rx.try_recv()
        {
            if let Some(canceller) = canceller.as_mut() { canceller.process(&mut chunk); }

            input_accum.extend_from_slice(&chunk);
        }

        //ENCODE OPUS FRAMES
        while input_accum.len() >= screen_consts::FRAME_SAMPLES
        {
            match encoder.encode_float(&input_accum[..screen_consts::FRAME_SAMPLES], &mut out)
            {
                Ok(len) =>
                {
                    if tx.send(AudioFrame { data: out[..len].to_vec() }).await.is_err() { return; }
                },

                _ => {},
            }
            input_accum.drain(..screen_consts::FRAME_SAMPLES);
        }
    }
}

pub async fn spawn_audio_playback(mut rx: Receiver<AudioFrame>, running: Arc<AtomicBool>)
{
    let host = cpal::default_host();
    let device = host.default_output_device().expect("No audio output device found");
    let mut config: StreamConfig = voice::configure_device(&device, device.supported_output_configs().unwrap(), device.default_output_config().unwrap(), false);
    config.buffer_size = BufferSize::Fixed(screen_consts::BUFFER_SIZE);

    let mut decoder = Decoder::new
    (
        <SampleRate as TryFrom<i32>>::try_from(consts::SAMPLE_RATE as i32).unwrap(),
        Channels::Stereo,
    ).unwrap();

    let (chunk_tx, mut chunk_rx) = mpsc::channel::<Vec<f32>>(screen_consts::PLAYBACK_CHANNEL_BOUND);

    let mut drain_buf: Vec<f32> = Vec::new();
    let mut drain_pos: usize = 0;

    let output_channels = config.channels as usize;
    let output_source_rate = consts::SAMPLE_RATE as f32;
    let output_target_rate = config.sample_rate as f32;
    let output_resample_step = output_source_rate / output_target_rate;

    let mut resample_pos = 0.;
    let mut current_frame = (0., 0.);
    let mut next_frame = (0., 0.);

    let stream = device.build_output_stream(config.clone(), move |data: &mut [f32], _: &OutputCallbackInfo|
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

    let mut out = vec![0f32; screen_consts::FRAME_SAMPLES];

    while running.load(Ordering::Relaxed)
    {
        //EXIT ON DISABLED ATTACH
        if !options::get_attach_screen()
        {
            running.store(false, Ordering::Relaxed);
            return;
        }

        match time::timeout(Duration::from_millis(50), rx.recv()).await
        {
            Ok(Some(mut frame)) =>
            {
                //A QUEUE THAT HAS GROWN NEVER SHRINKS ON ITS OWN. THE CONSUMER IS A SOUND CARD AND
                //THE PRODUCER IS THE SHARER'S, SO THE TWO RUN AT THE SAME RATE FOREVER: WHATEVER
                //DEPTH ONE BAD MOMENT ON THE LINK PUSHES IN STAYS IN, AS PERMANENT LATENCY ON THE
                //AUDIO *AND* ON THE VIDEO BEHIND IT IN THE SAME TCP STREAM. SO THE BACKLOG IS
                //THROWN AWAY RATHER THAN PLAYED THROUGH - IT COSTS A CLICK, ONCE, AND BUYS THE
                //DIFFERENCE BACK. ONLY THE NEWEST FRAME IS DECODED; THE OLDER ONES ARE NOT
                //CONCEALED, BECAUSE NOBODY IS GOING TO HEAR THEM EITHER WAY.
                //`AUDIO_BACKLOG_TARGET` IS NOT ZERO ON PURPOSE: THE QUEUE IS ALSO THE JITTER BUFFER,
                //AND DRAINING IT FLAT WOULD TRADE THE LATENCY FOR A GAP IN EVERY LATE PACKET. THE
                //CHANNEL'S OWN BOUND IS THE CEILING THIS REPLACES, NOT THE DEPTH IT SHOULD SIT AT
                while rx.len() > screen_consts::AUDIO_BACKLOG_TARGET
                {
                    match rx.try_recv()
                    {
                        Ok(newer) => frame = newer,
                        Err(_) => break,
                    }
                }

                match decoder.decode_float(Some(&frame.data[..]), &mut out, false)
                {
                    Ok(len) =>
                    {
                        let decoded = out[..len * 2].to_vec();
                        if chunk_tx.send(decoded).await.is_err() { return; } //CHANNEL CLOSED
                    },
                    _ => {},
                }
            },
            _ => {}
        }
    }
}
