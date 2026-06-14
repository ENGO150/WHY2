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

use crossbeam_channel::Sender;


#[cfg(target_os = "linux")]
use libwayshot::WayshotConnection;
use xcap::Monitor;

use crate::network::screen::client::
{
    compress,
    frame::{ Frame, CompressedFrame },
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

    let wayshot = WayshotConnection::new().expect("Failed to connect to wayland via libwayshot");
    let outputs = wayshot.get_all_outputs();
    let mut target_output = outputs.first().expect("No wayland outputs found").clone();

    if let Ok(name) = monitor.name()
    {
        for out in outputs
        {
            if out.name == name
            {
                target_output = out.clone();
                break;
            }
        }
    }

    while running.load(Ordering::Relaxed)
    {
        let tick_start = Instant::now();

        if let Ok(image) = wayshot.screenshot_single_output(&target_output, true)
        {
            let rgba = image.to_rgba8();
            let compressed = compress
            (
                image.width(), image.height(), &rgba,
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
    let mut data = Vec::with_capacity((w * h) as usize);
    let mut differences = 0;
    let mut rgb_data = Vec::with_capacity((w * h * 3) as usize);

    if prev_data.len() == (w * h) as usize
    {
        for (i, chunk) in rgba.chunks_exact(4).enumerate()
        {
            let r = chunk[0] as u32;
            let g = chunk[1] as u32;
            let b = chunk[2] as u32;
            rgb_data.push(chunk[0]);
            rgb_data.push(chunk[1]);
            rgb_data.push(chunk[2]);

            let current = 0xFF000000 | (r << 16) | (g << 8) | b;

            if prev_data[i] != current { differences += 1; }

            if *prev_was_jpeg || prev_data[i] != current
            {
                data.push(current);
                prev_data[i] = current;
            } else { data.push(0); }
        }
    } else
    {
        prev_data.clear();
        for chunk in rgba.chunks_exact(4)
        {
            let r = chunk[0] as u32;
            let g = chunk[1] as u32;
            let b = chunk[2] as u32;
            rgb_data.push(chunk[0]);
            rgb_data.push(chunk[1]);
            rgb_data.push(chunk[2]);

            let current = 0xFF000000 | (r << 16) | (g << 8) | b;
            data.push(current);
            prev_data.push(current);
        }

        differences = w * h;
    }

    *prev_was_jpeg = differences > (w * h) / 2;

    let compressed = if *prev_was_jpeg
    {
        compress::compress_jpeg(w, h, &rgb_data)
    } else
    {
        let frame = Frame { width: w, height: h, data };
        compress::compress_zstd(&frame)
    };

    compressed
}
