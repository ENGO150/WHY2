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
pub mod gpu;
pub mod options;
pub mod video;

use std::
{
    sync::
    {
        Arc,
        RwLock,
        atomic::{ AtomicBool, Ordering },
    },
};

use tokio::
{
    task,
    io::AsyncWriteExt,
    net::tcp::OwnedWriteHalf,
    sync::
    {
        Mutex,
        mpsc::
        {
            self,
            Sender,
            Receiver,
            UnboundedSender,
        },
    },
};

use winit::event_loop::EventLoopProxy;

use crate::
{
    crypto,
    options as chat_options,
    network::
    {
        self,
        client::{ self, ClientEvent },
        codes::PacketCode,
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
    pub deattach: UnboundedSender<()>, //DEATTACH REQUEST FROM THE WINDOW (SENT TO THE SERVER BY A TASK)
}

//ENUMS
pub enum UserEvent //CUSTOM WINIT EVENTS
{
    NewSession(ScreenShareRequest),
    NewFrame,
}

//GLOBAL VARIABLES
pub static SCREEN_SHARE_PROXY: RwLock<Option<EventLoopProxy<UserEvent>>> = RwLock::new(None);

pub async fn screen(token: [u8; 32], events: Sender<ClientEvent>)
{
    //INIT FILE CONNECTION
    let (_read_stream, mut write_stream) = client::connect(chat_options::get_server_address()).await
        .expect("Screen upload connection failed");

    //KEEP THE UPLOAD'S BACKLOG WHERE THE ENCODER CAN SEE IT
    screen::cap_socket_buffers(write_stream.as_ref());

    //SEND TOKEN
    write_stream.write_all(&token).await.unwrap();

    //SHARED STATE
    let (tx, mut rx) = mpsc::channel(consts::MULTIPLEX_CHANNEL_BOUND);
    let (audio_tx, mut audio_rx) = mpsc::channel(consts::MULTIPLEX_CHANNEL_BOUND);

    let running = Arc::new(AtomicBool::new(true));

    //SPAWN CAPTURE TASKS (CAPTURE IS A BLOCKING CPU LOOP, KEEP IT OFF THE RUNTIME)
    let running_capture = running.clone();
    let running_audio = running.clone();
    let capture = task::spawn_blocking(move || capture::capture_loop(tx, running_capture, consts::TARGET_FPS));
    tokio::spawn(audio::spawn_audio_capture(audio_tx, running_audio));

    //LOCAL SEQ COUNTER
    let mut seq = 0usize;

    //INIT REX STREAM
    let mut rex_stream = crypto::init_rex_stream(chat_options::get_keys().as_ref().unwrap(), &token).unwrap();

    //LOOP SENDING FRAMES
    loop
    {
        //EXIT ON DISABLED SCREEN
        if !options::get_use_screen() { break; }

        tokio::select!
        {
            //VIDEO FRAME
            msg = rx.recv() =>
            {
                let compressed_frame = match msg
                {
                    Some(f) => f,
                    None => break,
                };

                screen::send_frame(&mut write_stream,
                    ScreenPacketCode::Video { data: compressed_frame }, &mut rex_stream, Some(&mut seq)).await;
            },

            //AUDIO FRAME
            msg = audio_rx.recv() =>
            {
                let audio_frame = match msg
                {
                    Some(f) => f,
                    None => break,
                };

                screen::send_frame(&mut write_stream,
                    ScreenPacketCode::Audio { data: audio_frame.data }, &mut rex_stream, Some(&mut seq)).await;
            }
        }
    }

    //STOP THE CAPTURE LOOP AND REPORT WHY IT ENDED (THE SERVER ONLY EVER SEES A DEAD SOCKET)
    running.store(false, Ordering::Relaxed);

    let reason = match capture.await
    {
        Ok(Err(reason)) => Some(reason),
        Err(_) => Some("screen capture crashed".to_owned()), //PANICKED OR CANCELLED
        Ok(Ok(())) => None,
    };

    if let Some(reason) = reason
    {
        events.send(ClientEvent::ScreenFailed(reason)).await.ok();
    }
}

pub async fn attach(token: [u8; 32], main_stream: Arc<Mutex<OwnedWriteHalf>>)
{
    //INIT FILE CONNECTION
    let (mut read_stream, mut write_stream) = client::connect(chat_options::get_server_address()).await
        .expect("Screen download connection failed");

    //KEEP THE DOWNLOAD'S BACKLOG WHERE IT CAN STILL BE SHED
    screen::cap_socket_buffers(read_stream.as_ref());

    //SEND TOKEN (HAHA, SLEEP TOKEN)
    write_stream.write_all(&token).await.unwrap();

    //SHARED STATE
    let (tx, rx) = mpsc::channel(consts::MULTIPLEX_CHANNEL_BOUND);
    let (audio_tx, audio_rx) = mpsc::channel(consts::NETWORK_CHANNEL_BOUND);
    let running = Arc::new(AtomicBool::new(true));

    let running_audio = running.clone();
    tokio::spawn(audio::spawn_audio_playback(audio_rx, running_audio));

    //INIT REX STREAM
    let mut rex_stream = crypto::init_rex_stream(chat_options::get_keys().as_ref().unwrap(), &token).unwrap();

    //BRIDGE THE WINIT EVENT LOOP (NOT ASYNC) BACK TO THE SERVER
    let (deattach_tx, mut deattach_rx) = mpsc::unbounded_channel::<()>();
    tokio::spawn(async move
    {
        while deattach_rx.recv().await.is_some()
        {
            //DEATTACH ON SERVER
            network::send(&mut *main_stream.lock().await,
                PacketCode::Deattach { username: None }, chat_options::get_keys().as_ref()).await;
        }
    });

    //SPAWN NETWORK READER TASK
    let running_net = running.clone();
    tokio::spawn(async move
    {
        let mut streams = (&mut read_stream, Arc::new(Mutex::new(write_stream)));
        let mut seq = 0usize;

        while running_net.load(Ordering::Relaxed)
        {
            //EXIT ON DISABLED ATTACH
            if !options::get_attach_screen()
            {
                running_net.store(false, Ordering::Relaxed);
                return;
            }

            let read = match screen::receive_frame(&mut streams, &mut rex_stream, &mut seq).await
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
                    tx.send(data).await.ok();
                    if let Some(proxy) = SCREEN_SHARE_PROXY.read().unwrap().as_ref()
                    {
                        proxy.send_event(UserEvent::NewFrame).ok();
                    }
                },

                ScreenPacketCode::Audio { data } =>
                {
                    //VIDEO AND AUDIO SHARE ONE TCP STREAM, SO WAITING HERE STOPS THE PICTURE TOO -
                    //A PLAYBACK PATH THAT HAS FALLEN BEHIND WOULD HOLD THE READER, CLOSE THE
                    //RECEIVE WINDOW AND BACK THE WHOLE SHARE UP. 20 ms OF SOUND IS THE CHEAPER LOSS
                    audio_tx.try_send(AudioFrame { data }).ok();
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
            deattach: deattach_tx,
        })).ok();
    }
}
