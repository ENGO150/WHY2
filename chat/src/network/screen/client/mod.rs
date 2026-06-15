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

//MODULES
pub mod audio;
pub mod capture;
pub mod compress;
pub mod display;
pub mod frame;

use std::
{
    thread,
    io::Write,
    sync::
    {
        Arc,
        Mutex,
        mpsc::Sender,
        atomic::{ AtomicBool, Ordering },
    },
};

use crate::
{
    options,
    network::
    {
        self,
        MessagePacket,
        client::{ self, ClientEvent },
    },
};

#[cfg(target_os = "linux")]
use winit::platform::
{
    wayland::EventLoopBuilderExtWayland,
    x11::EventLoopBuilderExtX11,
};

#[cfg(target_os = "windows")]
use winit::platform::windows::EventLoopBuilderExtWindows;

#[cfg(target_os = "macos")]
use winit::platform::macos::EventLoopBuilderExtMacOS;

pub fn screen_upload(token: [u8; 32], tx: Sender<ClientEvent>)
{
    //INIT FILE CONNECTION
    let mut stream = client::connect(options::get_server_address()).expect("Screen upload connection failed");

    //SEND TOKEN
    stream.write_all(&token).unwrap();

    //LOG
    tx.send(ClientEvent::ScreenUpload(true)).unwrap();
    tx.send(ClientEvent::Prompt).unwrap();

    //SHARED STATE
    let (tx, rx) = crossbeam_channel::bounded(2);
    let (audio_tx, audio_rx) = crossbeam_channel::bounded(2);

    let running = Arc::new(AtomicBool::new(true));

    //SPAWN CAPTURE THREAD
    let running_capture = running.clone();
    let _capture_thread = thread::spawn(move || capture::capture_loop(tx, running_capture, 30));
    let _audio_capture_thread = audio::spawn_audio_capture(audio_tx, running.clone());

    loop
    {
        //HANDLE VIDEO FRAMES (BLOCKING)
        let compressed_frame = match rx.recv()
        {
            Ok(f) => f,
            Err(_) => return,
        };

        network::send(&mut stream, MessagePacket
        {
            frame: Some(compressed_frame),
            ..Default::default()
        }, options::get_keys().as_ref());

        //HANDLE AUDIO FRAMES
        if let Ok(_audio_chunk) = audio_rx.try_recv()
        {
            //TODO
        }
    }
}

pub fn screen_download(token: [u8; 32])
{
    //INIT FILE CONNECTION
    let mut stream = client::connect(options::get_server_address()).expect("Screen download connection failed");

    //SEND TOKEN (HAHA, SLEEP TOKEN)
    stream.write_all(&token).unwrap();

    //SHARED STATE
    let (tx, rx) = crossbeam_channel::bounded(2);
    let running = Arc::new(AtomicBool::new(true));

    //CREATE EVENT LOOP
    let event_loop =
    {
        let mut builder = winit::event_loop::EventLoop::builder();

        #[cfg(target_os = "linux")]
        {
            EventLoopBuilderExtX11::with_any_thread(&mut builder, true);
            EventLoopBuilderExtWayland::with_any_thread(&mut builder, true);
        }

        #[cfg(target_os = "windows")]
        {
            builder.with_any_thread(true);
        }

        #[cfg(target_os = "macos")]
        {
            builder.with_any_thread(true);
        }

        builder.build().expect("Failed to create event loop")
    };
    let proxy = event_loop.create_proxy();

    //SPAWN NETWORK READER THREAD
    let running_net = running.clone();
    thread::spawn(move ||
    {
        let write_stream = Arc::new(Mutex::new(stream.try_clone().expect("Failed cloning stream")));
        let mut streams = (&mut stream, write_stream);
        let mut seq = 0usize;

        while running_net.load(Ordering::Relaxed)
        {
            let read = match network::receive(&mut streams, options::get_keys().as_ref(), Some(&mut seq))
            {
                Some(r) => r,
                None =>
                {
                    running_net.store(false, Ordering::Relaxed);
                    return;
                }
            };

            if let Some(frame) = read.frame
            {
                tx.send(frame).ok();
                proxy.send_event(()).ok();
            }
        }
    });

    //RUN DISPLAY ON CURRENT THREAD
    let mut app = display::App::new(rx, 1920, 1080, running);
    event_loop.run_app(&mut app).expect("Event loop terminated with error");
}
