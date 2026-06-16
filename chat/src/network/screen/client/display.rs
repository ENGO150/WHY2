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
    num,
    collections::HashMap,
    time::{ Duration, Instant },
    sync::
    {
        Arc,
        atomic::{ AtomicBool, Ordering },
    }
};

use crossbeam_channel::Receiver;

use softbuffer::{ Surface, Context };

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

use crate::network::
{
    CompressedFrame,
    screen::client::
    {
        frame::Frame,
        compress::{ self, DecompressedFrame },
        ScreenShareRequest,
        UserEvent,
    },
};

//PRIVATE
const FRAME_POLL_INTERVAL: Duration = Duration::from_millis(33); //~30 FPS POLL

//STRUCTS
struct GraphicsContext
{
    window: Arc<Window>,
    _context: Context<Arc<Window>>,
    surface: Surface<Arc<Window>, Arc<Window>>,
}

struct Session
{
    gfx: GraphicsContext,
    frame_rx: Receiver<CompressedFrame>,
    last_frame: Option<Frame>,
    running: Arc<AtomicBool>,
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
                        }
                    }
                },

                DecompressedFrame::JpegFull(full_frame) =>
                {
                    self.last_frame = Some(full_frame);
                },
            }
        }

        true
    }

    fn redraw(&mut self)
    {
        let size = self.gfx.window.inner_size();
        let nw_u32 = size.width;
        let nh_u32 = size.height;

        let (Some(nw), Some(nh)) = (num::NonZeroU32::new(nw_u32), num::NonZeroU32::new(nh_u32))
            else { return; };

        let Some(frame) = self.last_frame.as_ref()
            else
            {
                if let Ok(()) = self.gfx.surface.resize(nw, nh)
                {
                    if let Ok(mut buffer) = self.gfx.surface.buffer_mut()
                    {
                        buffer.fill(0);
                        let _ = buffer.present();
                    }
                }

                return;
            };

        let w = frame.width;
        let h = frame.height;

        if self.gfx.surface.resize(nw, nh).is_err() { return; }

        match self.gfx.surface.buffer_mut()
        {
            Ok(mut buffer) =>
            {
                //COPY 1:1 IF POSSIBLE
                if nw_u32 == w && nh_u32 == h
                {
                    buffer.copy_from_slice(&frame.data);
                } else
                {
                    let scale_w = nw_u32 as f32 / w as f32;
                    let scale_h = nh_u32 as f32 / h as f32;
                    let scale = scale_w.min(scale_h);

                    let dst_w = (w as f32 * scale).round() as u32;
                    let dst_h = (h as f32 * scale).round() as u32;

                    let offset_x = (nw_u32 - dst_w) / 2;
                    let offset_y = (nh_u32 - dst_h) / 2;

                    buffer.fill(0);

                    //INTEGER BILINEAR INTERPOLATION
                    let scale_x = (w << 16) / dst_w;
                    let scale_y = (h << 16) / dst_h;

                    for dy in 0..dst_h
                    {
                        let sy_fix = dy * scale_y + (scale_y >> 1);
                        let sy0 = (sy_fix >> 16).min(h - 1);
                        let sy1 = (sy0 + 1).min(h - 1);
                        let fy = ((sy_fix & 0xFFFF) >> 8) as u32;

                        let dst_row = (dy + offset_y) * nw_u32 + offset_x;
                        let src_row0 = sy0 * w;
                        let src_row1 = sy1 * w;

                        for dx in 0..dst_w
                        {
                            let sx_fix = dx * scale_x + (scale_x >> 1);
                            let sx0 = (sx_fix >> 16).min(w - 1);
                            let sx1 = (sx0 + 1).min(w - 1);
                            let fx = ((sx_fix & 0xFFFF) >> 8) as u32;

                            let p00 = frame.data[(src_row0 + sx0) as usize];
                            let p10 = frame.data[(src_row0 + sx1) as usize];
                            let p01 = frame.data[(src_row1 + sx0) as usize];
                            let p11 = frame.data[(src_row1 + sx1) as usize];

                            let r00 = (p00 >> 16) & 0xFF;
                            let r10 = (p10 >> 16) & 0xFF;
                            let r01 = (p01 >> 16) & 0xFF;
                            let r11 = (p11 >> 16) & 0xFF;
                            let g00 = (p00 >> 8) & 0xFF;
                            let g10 = (p10 >> 8) & 0xFF;
                            let g01 = (p01 >> 8) & 0xFF;
                            let g11 = (p11 >> 8) & 0xFF;
                            let b00 = p00 & 0xFF;
                            let b10 = p10 & 0xFF;
                            let b01 = p01 & 0xFF;
                            let b11 = p11 & 0xFF;

                            let r_top = r00 + ((r10 as i32 - r00 as i32) * fx as i32 / 256) as u32;
                            let r_bot = r01 + ((r11 as i32 - r01 as i32) * fx as i32 / 256) as u32;
                            let r = r_top + ((r_bot as i32 - r_top as i32) * fy as i32 / 256) as u32;

                            let g_top = g00 + ((g10 as i32 - g00 as i32) * fx as i32 / 256) as u32;
                            let g_bot = g01 + ((g11 as i32 - g01 as i32) * fx as i32 / 256) as u32;
                            let g = g_top + ((g_bot as i32 - g_top as i32) * fy as i32 / 256) as u32;

                            let b_top = b00 + ((b10 as i32 - b00 as i32) * fx as i32 / 256) as u32;
                            let b_bot = b01 + ((b11 as i32 - b01 as i32) * fx as i32 / 256) as u32;
                            let b = b_top + ((b_bot as i32 - b_top as i32) * fy as i32 / 256) as u32;

                            buffer[(dst_row + dx) as usize] = 0xFF000000 | (r << 16) | (g << 8) | b;
                        }
                    }
                }

                buffer.present().ok();
            }
            _ => {},
        }
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
        Self { sessions: HashMap::new() }
    }

    fn create_session(&mut self, event_loop: &ActiveEventLoop, request: ScreenShareRequest)
    {
        let attrs = WindowAttributes::default()
            .with_title("WHY2 ScreenShare")
            .with_inner_size(PhysicalSize::new(1920u32, 1080u32));

        let Ok(window) = event_loop.create_window(attrs) else { return; };
        let window = Arc::new(window);

        let Ok(context) = Context::new(window.clone()) else { return; };
        let Ok(surface) = Surface::new(&context, window.clone()) else { return; };

        let window_id = window.id();
        window.request_redraw();

        self.sessions.insert(window_id, Session
        {
            gfx: GraphicsContext { window, _context: context, surface },
            frame_rx: request.rx,
            last_frame: None,
            running: request.running,
            frame_count: 0,
            last_fps_time: Instant::now(),
        });
    }
}

impl ApplicationHandler<UserEvent> for ScreenShareApp
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop)
    {
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + FRAME_POLL_INTERVAL));
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent)
    {
        match event
        {
            UserEvent::NewSession(request) => self.create_session(event_loop, request),
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

            WindowEvent::CloseRequested =>
            {
                if let Some(session) = self.sessions.remove(&window_id)
                {
                    session.running.store(false, Ordering::Relaxed);
                }
            },

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop)
    {
        self.sessions.retain(|_, session| session.running.load(Ordering::Relaxed));

        for session in self.sessions.values_mut()
        {
            if session.process_pending_frames()
            {
                session.frame_count += 1;
                if session.last_fps_time.elapsed() >= std::time::Duration::from_secs(1)
                {
                    session.gfx.window.set_title(&format!("WHY2 Screenshare ({} FPS)", session.frame_count));
                    session.frame_count = 0;
                    session.last_fps_time = Instant::now();
                }

                session.gfx.window.request_redraw();
            }
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + FRAME_POLL_INTERVAL));
    }
}
