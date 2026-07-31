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
pub mod display;
pub mod options;

use std::
{
    thread,
    io::Write,
    net::TcpStream,
    sync::
    {
        Arc,
        Mutex,
        RwLock,
        atomic::{ AtomicBool, Ordering },
    },
};

use crossbeam_channel::Receiver;

use winit::event_loop::EventLoopProxy;

use crate::
{
    crypto,
    options as chat_options,
    network::
    {
        client,
        screen::
        {
            self,
            consts,
            ScreenPacketCode,
            client::audio::AudioFrame,
        },
    },
};

//STRUCTS
pub struct ScreenShareRequest
{
    pub rx: Receiver<Vec<u8>>,
    pub running: Arc<AtomicBool>,
    pub main_stream: Arc<Mutex<TcpStream>>
}

//ENUMS
pub enum UserEvent //CUSTOM WINIT EVENTS
{
    NewSession(ScreenShareRequest),
    NewFrame,
}

//GLOBAL VARIABLES
pub static SCREEN_SHARE_PROXY: RwLock<Option<EventLoopProxy<UserEvent>>> = RwLock::new(None);

pub fn screen(token: [u8; 32])
{
    //INIT FILE CONNECTION
    let mut stream = client::connect(chat_options::get_server_address()).expect("Screen upload connection failed");

    //SEND TOKEN
    stream.write_all(&token).unwrap();

    //SHARED STATE
    let (tx, rx) = crossbeam_channel::bounded(consts::MULTIPLEX_CHANNEL_BOUND);
    let (audio_tx, audio_rx) = crossbeam_channel::bounded(consts::MULTIPLEX_CHANNEL_BOUND);

    let running = Arc::new(AtomicBool::new(true));

    //SPAWN CAPTURE THREAD
    let running_capture = running.clone();
    let running_audio = running.clone();
    thread::spawn(move || capture::capture_loop(tx, running_capture, consts::TARGET_FPS));
    thread::spawn(move || audio::spawn_audio_capture(audio_tx, running_audio));

    //LOCAL SEQ COUNTER
    let mut seq = 0usize;

    //INIT REX STREAM
    let mut rex_stream = crypto::init_rex_stream(chat_options::get_keys().as_ref().unwrap(), &token).unwrap();

    //LOOP SENDING FRAMES
    loop
    {
        //EXIT ON DISABLED SCREEN
        if !options::get_use_screen()
        {
            running.store(false, Ordering::Relaxed);
            return;
        }

        crossbeam_channel::select!
        {
            //VIDEO FRAME
            recv(rx) -> msg =>
            {
                let compressed_frame = match msg
                {
                    Ok(f) => f,
                    Err(_) => return,
                };

                screen::send_frame(&mut stream,
                    ScreenPacketCode::Video { data: compressed_frame }, &mut rex_stream, Some(&mut seq));
            },

            //AUDIO FRAME
            recv(audio_rx) -> msg =>
            {
                let audio_frame = match msg
                {
                    Ok(f) => f,
                    Err(_) => return,
                };

                screen::send_frame(&mut stream,
                    ScreenPacketCode::Audio { data: audio_frame.data }, &mut rex_stream, Some(&mut seq));
            }
        }
    }
}

pub fn attach(token: [u8; 32], main_stream: Arc<Mutex<TcpStream>>)
{
    //INIT FILE CONNECTION
    let mut stream = client::connect(chat_options::get_server_address()).expect("Screen download connection failed");

    //SEND TOKEN (HAHA, SLEEP TOKEN)
    stream.write_all(&token).unwrap();

    //SHARED STATE
    let (tx, rx) = crossbeam_channel::bounded(consts::MULTIPLEX_CHANNEL_BOUND);
    let (audio_tx, audio_rx) = crossbeam_channel::bounded(consts::NETWORK_CHANNEL_BOUND);
    let running = Arc::new(AtomicBool::new(true));

    let running_audio = running.clone();
    thread::spawn(move || audio::spawn_audio_playback(audio_rx, running_audio));

    //INIT REX STREAM
    let mut rex_stream = crypto::init_rex_stream(chat_options::get_keys().as_ref().unwrap(), &token).unwrap();

    //SPAWN NETWORK READER THREAD
    let running_net = running.clone();
    thread::spawn(move ||
    {
        let write_stream = Arc::new(Mutex::new(stream.try_clone().expect("Failed cloning stream")));
        let mut streams = (&mut stream, write_stream);
        let mut seq = 0usize;

        while running_net.load(Ordering::Relaxed)
        {
            //EXIT ON DISABLED ATTACH
            if !options::get_attach_screen()
            {
                running_net.store(false, Ordering::Relaxed);
                return;
            }

            let read = match screen::receive_frame(&mut streams, &mut rex_stream, &mut seq)
            {
                Some(r) => r,
                None =>
                {
                    running_net.store(false, Ordering::Relaxed);
                    return;
                }
            };

            match read
            {
                ScreenPacketCode::Video { data } =>
                {
                    tx.send(data).ok();
                    if let Some(proxy) = SCREEN_SHARE_PROXY.read().unwrap().as_ref()
                    {
                        proxy.send_event(UserEvent::NewFrame).ok();
                    }
                },

                ScreenPacketCode::Audio { data } =>
                {
                    audio_tx.send(AudioFrame { data }).ok();
                },
            }
        }
    });

    if let Some(proxy) = SCREEN_SHARE_PROXY.read().unwrap().as_ref()
    {
        proxy.send_event(UserEvent::NewSession(ScreenShareRequest
        {
            rx,
            running,
            main_stream,
        })).ok();
    }
}
