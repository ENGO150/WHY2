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
    net::TcpStream,
    sync::
    {
        Arc,
        Mutex,
        OnceLock,
        mpsc::Sender,
        atomic::{ AtomicBool, Ordering },
    },
};

use crossbeam_channel::Receiver;

use winit::event_loop::EventLoopProxy;

use crate::
{
    options,
    network::
    {
        self,
        MessagePacket,
        CompressedFrame,
        client::{ self, ClientEvent },
    },
};

//STRUCTS
pub struct ScreenShareRequest
{
    pub rx: Receiver<CompressedFrame>,
    pub running: Arc<AtomicBool>,
    pub main_stream: Arc<Mutex<TcpStream>>
}

//ENUMS
pub enum UserEvent //CUSTOM WINIT EVENTS
{
    NewSession(ScreenShareRequest),
}

//GLOBAL VARIABLES
pub static SCREEN_SHARE_PROXY: OnceLock<EventLoopProxy<UserEvent>> = OnceLock::new();

pub fn screen(token: [u8; 32], tx: Sender<ClientEvent>)
{
    //INIT FILE CONNECTION
    let mut stream = client::connect(options::get_server_address()).expect("Screen upload connection failed");

    //SEND TOKEN
    stream.write_all(&token).unwrap();

    //LOG
    tx.send(ClientEvent::Screen(true)).unwrap();
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

pub fn attach(token: [u8; 32], main_stream: Arc<Mutex<TcpStream>>)
{
    //INIT FILE CONNECTION
    let mut stream = client::connect(options::get_server_address()).expect("Screen download connection failed");

    //SEND TOKEN (HAHA, SLEEP TOKEN)
    stream.write_all(&token).unwrap();

    //SHARED STATE
    let (tx, rx) = crossbeam_channel::bounded(2);
    let running = Arc::new(AtomicBool::new(true));

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
            }
        }
    });

    if let Some(proxy) = SCREEN_SHARE_PROXY.get()
    {
        proxy.send_event(UserEvent::NewSession(ScreenShareRequest
        {
            rx,
            running,
            main_stream,
        })).ok();
    }
}
