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
    env,
    thread,
    time::{ Duration, Instant },
    sync::
    {
        Arc,
        atomic::{ AtomicBool, Ordering },
    },
};

use tokio::sync::mpsc::Sender;

use xcap::Monitor;

use openh264::
{
    OpenH264API,
    formats::{ RgbaSliceU8, YUVBuffer },
    encoder::
    {
        Encoder,
        EncoderConfig,
        BitRate,
        FrameRate,
        IntraFramePeriod,
        Complexity,
        UsageType,
        RateControlMode,
    },
};

use crate::network::screen::
{
    consts,
    client::options,
};

pub fn get_primary_monitor() -> Monitor
{
    let monitors = Monitor::all().expect("Failed to enumerate monitors");

    if monitors.is_empty()
    {
        panic!("No monitors found");
    }

    monitors
        .into_iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .unwrap_or_else(||
        {
            Monitor::all().unwrap().into_iter().next().unwrap()
        })
}

pub fn capture_loop //CAPTURE LOOP
(
    frame_tx: Sender<Vec<u8>>,
    running: Arc<AtomicBool>,
    fps: u32,
)
{
    if cfg!(target_os = "linux") &&
        (env::var("WAYLAND_DISPLAY").is_ok() || env::var("XDG_SESSION_TYPE").unwrap_or_default() == "wayland")
    {
        #[cfg(target_os = "linux")]
        {
            capture_loop_wayshot(&get_primary_monitor(), frame_tx, running, fps);
        }
        return;
    }

    capture_loop_xcap(get_primary_monitor(), frame_tx, running, fps);
}

fn create_encoder(fps: f32) -> Encoder
{
    let config = EncoderConfig::new()
        .max_frame_rate(FrameRate::from_hz(fps))
        .rate_control_mode(RateControlMode::Bitrate)
        .bitrate(BitRate::from_bps(consts::H264_BITRATE))
        .intra_frame_period(IntraFramePeriod::from_num_frames((fps * 2.0) as u32))
        .complexity(Complexity::Low)
        .usage_type(UsageType::ScreenContentRealTime)
        .skip_frames(true)
        .adaptive_quantization(false)
        .background_detection(false);

    Encoder::with_api_config(OpenH264API::from_source(), config).expect("Failed to create H.264 encoder")
}

fn encode_rgba(encoder: &mut Encoder, width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>>
{
    let src = RgbaSliceU8::new(rgba, (width as usize, height as usize));
    let yuv = YUVBuffer::from_rgb_source(src);

    let bitstream = encoder.encode(&yuv).expect("H.264 encode failed");

    let data = bitstream.to_vec();

    //SKIP EMPTY FRAMES (ENCODER MAY DECIDE NO DATA IS NEEDED)
    if data.is_empty() { return None; }

    Some(data)
}

fn sleep_until_next_tick(next_tick: &mut Instant, target_interval: Duration)
{
    let now = Instant::now();
    if *next_tick > now
    {
        thread::sleep(*next_tick - now);
    } else
    {
        *next_tick = now;
    }

    *next_tick += target_interval;
}

fn capture_loop_xcap
(
    monitor: Monitor,
    frame_tx: Sender<Vec<u8>>,
    running: Arc<AtomicBool>,
    fps: u32,
)
{
    let target_interval = Duration::from_secs_f64(1.0 / fps as f64);
    let mut next_tick = Instant::now() + target_interval;

    let mut encoder = create_encoder(fps as f32);

    let mut last_raw = Vec::new();
    let mut last_encode_time = Instant::now();

    while running.load(Ordering::Relaxed)
    {
        //EXIT ON DISABLED SCREEN
        if !options::get_use_screen()
        {
            running.store(false, Ordering::Relaxed);
            return;
        }

        if let Ok(image) = monitor.capture_image()
        {
            let raw = image.as_raw();
            let force_encode = last_encode_time.elapsed() >= Duration::from_secs(2);

            if force_encode || raw != &last_raw
            {
                if let Some(compressed) = encode_rgba(&mut encoder, image.width(), image.height(), raw)
                {
                    frame_tx.try_send(compressed).ok();
                }

                last_raw.clear();
                last_raw.extend_from_slice(raw);
                last_encode_time = Instant::now();
            }
        }

        sleep_until_next_tick(&mut next_tick, target_interval);
    }
}

#[cfg(target_os = "linux")]
fn capture_loop_wayshot
(
    monitor: &Monitor,
    frame_tx: Sender<Vec<u8>>,
    running: Arc<AtomicBool>,
    fps: u32,
)
{
    let target_interval = Duration::from_secs_f64(1.0 / fps as f64);

    let mut wayshot = match libwayshot::WayshotConnection::new()
    {
        Ok(w) => w,
        Err(_) => return,
    };

    let mut target_output = match wayshot.get_all_outputs().iter()
        .find(|o| o.name == monitor.name().unwrap_or_default()).cloned()
    {
        Some(o) => o,
        None => return,
    };

    let mut encoder = create_encoder(fps as f32);

    //GET INITIAL DIMENSIONS FOR ENCODER
    let first_image = match wayshot.screenshot_single_output(&target_output, true)
    {
        Ok(img) => img.into_rgba8(),
        Err(_) => return,
    };

    //ENCODE AND SEND FIRST FRAME
    if let Some(compressed) = encode_rgba(&mut encoder, first_image.width(), first_image.height(), first_image.as_raw())
    {
        frame_tx.try_send(compressed).ok();
    }

    let mut frame_count = 0;
    let mut next_tick = Instant::now() + target_interval;

    while running.load(Ordering::Relaxed)
    {
        //EXIT ON DISABLED SCREEN
        if !options::get_use_screen()
        {
            running.store(false, Ordering::Relaxed);
            return;
        }

        if frame_count > (consts::TARGET_FPS * 10)
        {
            if let Ok(w) = libwayshot::WayshotConnection::new()
            {
                let o_opt = w.get_all_outputs().iter().find(|o| o.name == monitor.name().unwrap_or_default()).cloned();
                if let Some(o) = o_opt
                {
                    wayshot = w;
                    target_output = o;
                }
            }
            frame_count = 0;
        }
        frame_count += 1;

        if let Ok(image) = wayshot.screenshot_single_output(&target_output, true)
        {
            let image = image.into_rgba8();

            if let Some(compressed) = encode_rgba(&mut encoder, image.width(), image.height(), image.as_raw())
            {
                frame_tx.try_send(compressed).ok();
            }
        }

        sleep_until_next_tick(&mut next_tick, target_interval);
    }
}
