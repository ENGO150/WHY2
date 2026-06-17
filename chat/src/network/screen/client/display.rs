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
    net::TcpStream,
    collections::HashMap,
    time::{ Duration, Instant },
    sync::
    {
        Arc,
        Mutex,
        atomic::{ AtomicBool, Ordering },
    }
};

use crossbeam_channel::Receiver;

use pixels::{ Pixels, SurfaceTexture };

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

use crate::
{
    options,
    network::
    {
        self,
        MessageCode,
        MessagePacket,
        CompressedFrame,
        screen::
        {
            consts,
            client::
            {
                frame::Frame,
                compress::{ self, DecompressedFrame },
                ScreenShareRequest,
                UserEvent,
            },
        },
    },
};

//PRIVATE
//STRUCTS
struct GraphicsContext
{
    window: Arc<Window>,
    pixels: Pixels<'static>,
}

struct Session
{
    gfx: GraphicsContext,
    frame_rx: Receiver<CompressedFrame>,
    last_frame: Option<Frame>,
    frame_dirty: bool,
    running: Arc<AtomicBool>,
    main_stream: Arc<Mutex<TcpStream>>,
    frame_count: u32,
    last_fps_time: Instant,
}

//IMPLEMENTATIONS
impl Session
{
    fn process_pending_frames(&mut self) -> bool
    {
        let mut pending: Vec<CompressedFrame> = Vec::new();
        while let Ok(compressed) = self.frame_rx.try_recv()
        {
            pending.push(compressed);
        }

        if pending.is_empty() { return false; }

        //SKIP FRAMES BEFORE LAST JPEG KEYFRAME
        let start = pending.iter()
            .rposition(|f| !f.compressed_data.is_empty() && f.compressed_data[0] == 1)
            .unwrap_or(0);

        for compressed in &pending[start..]
        {
            match compress::decompress(compressed)
            {
                DecompressedFrame::ZstdDiff(diff_frame) =>
                {
                    if let Some(ref mut last) = self.last_frame
                    {
                        if last.width == diff_frame.width && last.height == diff_frame.height
                        {
                            for (l, d) in last.data.iter_mut().zip(diff_frame.data.iter())
                            {
                                if *d & 0xFF000000 != 0
                                {
                                    *l = *d;
                                }
                            }
                            self.frame_dirty = true;
                        }
                    }
                },

                DecompressedFrame::JpegFull(full_frame) =>
                {
                    self.last_frame = Some(full_frame);
                    self.frame_dirty = true;
                },
            }
        }

        true
    }

    fn redraw(&mut self)
    {
        let size = self.gfx.window.inner_size();
        self.gfx.pixels.resize_surface(size.width, size.height).ok();

        if let Some(frame) = self.last_frame.as_ref()
        {
            let w = frame.width;
            let h = frame.height;
            self.gfx.pixels.resize_buffer(w, h).ok();

            if self.frame_dirty
            {
                let frame_data = self.gfx.pixels.frame_mut();

                //CHECK IF LENGTH MATCHES TO PREVENT PANIC
                if frame_data.len() == (w * h * 4) as usize && frame.data.len() == (w * h) as usize
                {
                    for (i, &pixel) in frame.data.iter().enumerate()
                    {
                        let idx = i * 4;
                        frame_data[idx]     = ((pixel >> 16) & 0xFF) as u8; // R
                        frame_data[idx + 1] = ((pixel >> 8) & 0xFF) as u8;  // G
                        frame_data[idx + 2] = (pixel & 0xFF) as u8;         // B
                        frame_data[idx + 3] = 255;                          // A
                    }
                }
                self.frame_dirty = false;
            }
        }

        self.gfx.pixels.render().ok();
    }
}

//PUBLIC
//STRUCTS
pub struct ScreenShareApp //DISPATCHER
{
    sessions: HashMap<WindowId, Session>,
    close_pending: bool,
}

//IMPLEMENTATIONS
impl ScreenShareApp
{
    pub fn new() -> Self
    {
        Self
        {
            sessions: HashMap::new(),
            close_pending: false,
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
            session.last_frame = None;
            session.frame_dirty = false;
            session.frame_count = 0;
            session.last_fps_time = Instant::now();

            session.gfx.window.request_redraw();
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
        pixels.set_scaling_mode(pixels::ScalingMode::Fill);

        let window_id = window.id();
        window.request_redraw();

        self.sessions.insert(window_id, Session
        {
            gfx: GraphicsContext { window, pixels },
            frame_rx: request.rx,
            last_frame: None,
            frame_dirty: false,
            running: request.running,
            main_stream: request.main_stream,
            frame_count: 0,
            last_fps_time: Instant::now(),
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
                self.close_pending = false;
                self.create_session(event_loop, request);
            },

            UserEvent::Closed =>
            {
                self.close_pending = true;
            },
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
                    session.gfx.pixels.resize_surface(size.width, size.height).ok();
                    session.gfx.window.request_redraw();
                }
            },

            WindowEvent::CloseRequested =>
            {
                if let Some(session) = self.sessions.remove(&window_id)
                {
                    session.running.store(false, Ordering::Relaxed);

                    //DEATTACH ON SERVER
                    network::send(&mut session.main_stream.lock().unwrap(), MessagePacket
                    {
                        code: Some(MessageCode::Deattach),
                        ..Default::default()
                    }, options::get_keys().as_ref());
                }
            },

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop)
    {
        if self.close_pending
        {
            self.sessions.retain(|_, session| session.running.load(Ordering::Relaxed));
            self.close_pending = false;
        }

        for session in self.sessions.values_mut()
        {
            if session.process_pending_frames()
            {
                session.frame_count += 1;
                if session.last_fps_time.elapsed() >= Duration::from_secs(1)
                {
                    session.gfx.window.set_title(&format!("WHY2 Screenshare ({} FPS)", session.frame_count));
                    session.frame_count = 0;
                    session.last_fps_time = Instant::now();
                }

                session.gfx.window.request_redraw();
            }
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + consts::FRAME_POLL_INTERVAL));
    }
}
