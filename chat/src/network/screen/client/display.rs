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
    collections::HashMap,
    time::{ Duration, Instant },
    sync::
    {
        Arc,
        atomic::{ AtomicBool, Ordering },
    }
};

use tokio::sync::mpsc::{ Receiver, UnboundedSender };

use pixels::
{
    Pixels,
    SurfaceTexture,
    ScalingMode,
};

use winit::
{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ ActiveEventLoop, ControlFlow },
    window::
    {
        Window,
        WindowAttributes,
        WindowId,
    },
};

use openh264::
{
    decoder::Decoder,
    formats::YUVSource,
};

use crate::network::screen::
{
    consts,
    client::{ ScreenShareRequest, UserEvent },
};

//PRIVATE
//STRUCTS
struct Session
{
    window: Arc<Window>,
    pixels: Pixels<'static>,
    frame_rx: Receiver<Vec<u8>>,
    decoder: Decoder,
    last_width: u32,
    last_height: u32,
    frame_dirty: bool,
    running: Arc<AtomicBool>,
    deattach: UnboundedSender<()>,
    frame_count: u32,
    last_fps_time: Instant,
    close_requested_at: Option<Instant>,
}

//IMPLEMENTATIONS
impl Session
{
    fn process_pending_frames(&mut self) -> bool
    {
        let mut pending: Vec<Vec<u8>> = Vec::new();
        while let Ok(compressed) = self.frame_rx.try_recv()
        {
            pending.push(compressed);
        }

        if pending.is_empty() { return false; }

        let newest = pending.len() - 1;

        for (index, compressed) in pending.iter().enumerate()
        {
            //DECODE H.264 BITSTREAM - EVERY FRAME MUST BE DECODED, H.264 IS TEMPORALLY PREDICTED
            //AND SKIPPING ONE WOULD BREAK PREDICTION FOR THE FRAMES THAT FOLLOW
            let Ok(Some(yuv)) = self.decoder.decode(compressed) else { continue; };

            //ONLY THE NEWEST FRAME IS EVER SEEN, SO THE OLDER ONES SKIP THE YUV -> RGBA PASS AND THE UPLOAD
            if index != newest { continue; }

            let dim = yuv.dimensions();
            let w = dim.0 as u32;
            let h = dim.1 as u32;

            if self.last_width != w || self.last_height != h
            {
                self.last_width = w;
                self.last_height = h;

                //RESIZE PIXEL BUFFER TO MATCH DECODED DIMENSIONS
                self.pixels.resize_buffer(w, h).ok();
            }

            let frame_data = self.pixels.frame_mut();

            //WRITE RGBA DIRECTLY INTO THE PIXEL BUFFER
            let wanted = dim.0 * dim.1 * 4;
            if frame_data.len() == wanted
            {
                yuv.write_rgba8(frame_data);
            }

            self.frame_dirty = true;
        }

        true
    }

    fn redraw(&mut self)
    {
        let size = self.window.inner_size();
        self.pixels.resize_surface(size.width, size.height).ok();

        self.pixels.render().ok();
    }
}

//PUBLIC
//STRUCTS
pub struct ScreenShareApp //DISPATCHER
{
    sessions: HashMap<WindowId, Session>,
}

//IMPLEMENTATIONS
impl ScreenShareApp
{
    pub fn new() -> Self
    {
        Self
        {
            sessions: HashMap::new(),
        }
    }

    fn create_session(&mut self, event_loop: &ActiveEventLoop, request: ScreenShareRequest)
    {
        //CHECK IF ANOTHER ATTACH IS ALIVE (RECYCLE WINDOW)
        if let Some(session) = self.sessions.values_mut().next()
        {
            let old_running = session.running.clone();

            //RESET
            session.frame_rx = request.rx;
            session.running = request.running;
            session.deattach = request.deattach;
            session.decoder = Decoder::new().expect("Failed to create H.264 decoder");
            session.last_width = 0;
            session.last_height = 0;
            session.frame_dirty = false;
            session.frame_count = 0;
            session.last_fps_time = Instant::now();
            session.close_requested_at = None;

            session.window.request_redraw();
            old_running.store(false, Ordering::Relaxed);

            return;
        }

        let attrs = WindowAttributes::default()
            .with_title("WHY2 ScreenShare")
            .with_inner_size(PhysicalSize::new(consts::WINIT_SIZE.0, consts::WINIT_SIZE.1));

        let Ok(window) = event_loop.create_window(attrs) else { return; };
        let window = Arc::new(window);

        let size = window.inner_size();
        let surface_texture = SurfaceTexture::new(size.width, size.height, window.clone());
        let Ok(mut pixels) = Pixels::new(consts::WINIT_SIZE.0, consts::WINIT_SIZE.1, surface_texture) else { return; };
        pixels.set_scaling_mode(ScalingMode::Fill);

        let window_id = window.id();
        window.request_redraw();

        self.sessions.insert(window_id, Session
        {
            window,
            pixels,
            frame_rx: request.rx,
            decoder: Decoder::new().expect("Failed to create H.264 decoder"),
            last_width: 0,
            last_height: 0,
            frame_dirty: false,
            running: request.running,
            deattach: request.deattach,
            frame_count: 0,
            last_fps_time: Instant::now(),
            close_requested_at: None,
        });
    }
}

impl ApplicationHandler<UserEvent> for ScreenShareApp
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop)
    {
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + consts::FRAME_POLL_INTERVAL));
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent)
    {
        match event
        {
            UserEvent::NewSession(request) =>
            {
                self.create_session(event_loop, request);
            },
            UserEvent::NewFrame => {}, //WAKES UP EVENT LOOP
        }
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent)
    {
        match event
        {
            WindowEvent::RedrawRequested =>
            {
                if let Some(session) = self.sessions.get_mut(&window_id)
                {
                    session.redraw();
                }
            },

            WindowEvent::Resized(size) =>
            {
                if let Some(session) = self.sessions.get_mut(&window_id)
                {
                    session.pixels.resize_surface(size.width, size.height).ok();
                    session.window.request_redraw();
                }
            },

            WindowEvent::CloseRequested =>
            {
                if let Some(session) = self.sessions.remove(&window_id)
                {
                    session.running.store(false, Ordering::Relaxed);

                    //DEATTACH ON SERVER (HANDED OVER TO THE ASYNC SIDE)
                    session.deattach.send(()).ok();
                }
            },

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop)
    {
        self.sessions.retain(|_, session|
        {
            if session.running.load(Ordering::Relaxed)
            {
                session.close_requested_at = None;
                true
            } else
            {
                if session.close_requested_at.is_none()
                {
                    session.close_requested_at = Some(Instant::now());
                }

                session.close_requested_at.unwrap().elapsed() < Duration::from_millis(250)
            }
        });

        for session in self.sessions.values_mut()
        {
            if session.process_pending_frames()
            {
                session.frame_count += 1;
                if session.last_fps_time.elapsed() >= Duration::from_secs(1)
                {
                    session.window.set_title(&format!("WHY2 Screenshare ({} FPS)", session.frame_count));
                    session.frame_count = 0;
                    session.last_fps_time = Instant::now();
                }

                session.window.request_redraw();
            }
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + consts::FRAME_POLL_INTERVAL));
    }
}
