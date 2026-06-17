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
    slice,
    thread,
    time::{ Duration, Instant },
    sync::
    {
        Arc,
        atomic::{ AtomicBool, Ordering },
    },
};

use crossbeam_channel::Sender;

use xcap::Monitor;

use crate::network::
{
    CompressedFrame,
    screen::
    {
        consts,
        client::
        {
            compress,
            frame::Frame,
        },
    },
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
    frame_tx: Sender<CompressedFrame>,
    running: Arc<AtomicBool>,
    fps: u32,
) {
    if cfg!(target_os = "linux") &&
        (env::var("WAYLAND_DISPLAY").is_ok() || env::var("XDG_SESSION_TYPE").unwrap_or_default() == "wayland")
    {
        #[cfg(target_os = "linux")]
        {
            let monitor = get_primary_monitor();
            capture_loop_wayshot(&monitor, frame_tx, running, fps);
        }
        return;
    }

    capture_loop_xcap(get_primary_monitor(), frame_tx, running, fps);
}

fn capture_loop_xcap
(
    monitor: Monitor,
    frame_tx: Sender<CompressedFrame>,
    running: Arc<AtomicBool>,
    fps: u32,
)
{
    let target_interval = Duration::from_secs_f64(1.0 / fps as f64);
    let mut prev_data: Vec<u32> = Vec::new();
    let mut prev_was_jpeg = false;

    while running.load(Ordering::Relaxed)
    {
        let tick_start = Instant::now();

        if let Ok(image) = monitor.capture_image()
        {
            let compressed = compress
            (
                image.width(), image.height(), image.as_raw(),
                &mut prev_data, &mut prev_was_jpeg
            );

            let _ = frame_tx.send(compressed);
        }

        let tick_elapsed = tick_start.elapsed();
        if tick_elapsed < target_interval
        {
            thread::sleep(target_interval - tick_elapsed);
        }
    }
}

#[cfg(target_os = "linux")]
fn capture_loop_wayshot
(
    monitor: &Monitor,
    frame_tx: Sender<CompressedFrame>,
    running: Arc<AtomicBool>,
    fps: u32,
)
{
    let target_interval = Duration::from_secs_f64(1.0 / fps as f64);

    let mut prev_data: Vec<u32> = Vec::new();
    let mut prev_was_jpeg = false;

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

    let mut frame_count = 0;

    while running.load(Ordering::Relaxed)
    {
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

        let tick_start = Instant::now();

        if let Ok(image) = wayshot.screenshot_single_output(&target_output, true)
        {
            let rgba = image.into_rgba8();
            let rgba_raw = rgba.as_raw();

            let compressed = compress
            (
                rgba.width(), rgba.height(), rgba_raw,
                &mut prev_data, &mut prev_was_jpeg
            );

            let _ = frame_tx.send(compressed);
        }

        let tick_elapsed = tick_start.elapsed();
        if tick_elapsed < target_interval
        {
            thread::sleep(target_interval - tick_elapsed);
        }
    }
}

fn compress(w: u32, h: u32, rgba: &[u8], prev_data: &mut Vec<u32>, prev_was_jpeg: &mut bool) -> CompressedFrame
{
    let mut data = vec![0u32; (w * h) as usize];
    let mut differences = 0;

    let rgba_u32 = unsafe { slice::from_raw_parts(rgba.as_ptr() as *const u32, rgba.len() / 4) };

    if prev_data.len() == (w * h) as usize
    {
        for (i, &pixel) in rgba_u32.iter().enumerate()
        {
            let current = 0xFF000000 | (pixel.to_be() >> 8);

            let prev = prev_data[i];
            if prev != current { differences += 1; }

            if *prev_was_jpeg || prev != current
            {
                data[i] = current;
                prev_data[i] = current;
            }
        }
    } else
    {
        prev_data.clear();
        for (i, &pixel) in rgba_u32.iter().enumerate()
        {
            let current = 0xFF000000 | (pixel.to_be() >> 8);
            data[i] = current;
            prev_data.push(current);
        }
        differences = w * h;
    }

    if differences > (w * h) / (consts::COMPRESSION_TRESHOLD * 100)
    {
        *prev_was_jpeg = true;
        compress::compress_jpeg(w, h, rgba)
    } else
    {
        *prev_was_jpeg = false;

        let frame = Frame { width: w, height: h, data };
        compress::compress_zstd(&frame)
    }
}
