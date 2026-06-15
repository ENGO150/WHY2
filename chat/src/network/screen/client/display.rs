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
    event_loop::ActiveEventLoop,
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
    },
};

struct GraphicsContext
{
    window: Arc<Window>,
    _context: Context<Arc<Window>>,
    surface: Surface<Arc<Window>, Arc<Window>>,
}

pub struct App
{
    gfx: Option<GraphicsContext>,
    frame_rx: Receiver<CompressedFrame>,
    last_frame: Option<Frame>,
    capture_width: u32,
    capture_height: u32,
    running: Arc<AtomicBool>,
}

impl App
{
    pub fn new
    (
        frame_rx: Receiver<CompressedFrame>,
        capture_width: u32,
        capture_height: u32,
        running: Arc<AtomicBool>,
    ) -> Self
    {
        Self
        {
            gfx: None,
            frame_rx,
            last_frame: None,
            capture_width,
            capture_height,
            running,
        }
    }

    fn process_pending_frames(&mut self)
    {
        let mut got_new_frame = false;
        while let Ok(compressed) = self.frame_rx.try_recv()
        {
            match compress::decompress(&compressed)
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

            got_new_frame = true;
        }

        if got_new_frame
        {
            if let Some(gfx) = &self.gfx
            {
                gfx.window.request_redraw();
            }
        }
    }
}

impl ApplicationHandler<()> for App
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop)
    {
        //CREATE WINDOW ONLY ONCE
        if self.gfx.is_some() { return; }

        let attrs = WindowAttributes::default()
            .with_title("WHY2 ScreenShare")
            .with_inner_size(PhysicalSize::new(self.capture_width, self.capture_height));

        let window = Arc::new(event_loop.create_window(attrs).expect("Failed to create window"));

        let context = Context::new(window.clone()).expect("Failed to create softbuffer context");
        let surface = Surface::new(&context, window.clone()).expect("Failed to create softbuffer surface");

        self.gfx = Some(GraphicsContext
        {
            window,
            _context: context,
            surface,
        });

        //REQUEST INITIAL REDRAW
        if let Some(gfx) = &self.gfx
        {
            gfx.window.request_redraw();
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ())
    {
        self.process_pending_frames();
    }

    fn window_event
    (
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event
        {
            WindowEvent::RedrawRequested =>
            {
                let Some(gfx) = self.gfx.as_mut() else { return; };

                let size = gfx.window.inner_size();
                let nw_u32 = size.width;
                let nh_u32 = size.height;

                let (Some(nw), Some(nh)) = (num::NonZeroU32::new(nw_u32), num::NonZeroU32::new(nh_u32))
                    else { return; };

                let Some(frame) = self.last_frame.as_ref()
                    else
                    {
                        if let Ok(()) = gfx.surface.resize(nw, nh)
                        {
                            if let Ok(mut buffer) = gfx.surface.buffer_mut()
                            {
                                buffer.fill(0);
                                let _ = buffer.present();
                            }
                        }

                        return;
                    };

                let w = frame.width;
                let h = frame.height;

                if gfx.surface.resize(nw, nh).is_err() { return; }

                match gfx.surface.buffer_mut()
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

                            //BILINEAR INTERPOLATION
                            let src_w = w as f32;
                            let src_h = h as f32;
                            let dst_wf = dst_w as f32;
                            let dst_hf = dst_h as f32;

                            for dy in 0..dst_h
                            {
                                //MAP PIXELS
                                let sy = (dy as f32 + 0.5) * src_h / dst_hf - 0.5;
                                let sy0 = sy.floor() as i32;
                                let sy1 = sy0 + 1;
                                let fy = sy - sy0 as f32;

                                let sy0 = sy0.clamp(0, h as i32 - 1) as u32;
                                let sy1 = sy1.clamp(0, h as i32 - 1) as u32;

                                let dst_row = (dy + offset_y) * nw_u32 + offset_x;

                                for dx in 0..dst_w
                                {
                                    let sx = (dx as f32 + 0.5) * src_w / dst_wf - 0.5;
                                    let sx0 = sx.floor() as i32;
                                    let sx1 = sx0 + 1;
                                    let fx = sx - sx0 as f32;

                                    let sx0 = sx0.clamp(0, w as i32 - 1) as u32;
                                    let sx1 = sx1.clamp(0, w as i32 - 1) as u32;

                                    let p00 = frame.data[(sy0 * w + sx0) as usize];
                                    let p10 = frame.data[(sy0 * w + sx1) as usize];
                                    let p01 = frame.data[(sy1 * w + sx0) as usize];
                                    let p11 = frame.data[(sy1 * w + sx1) as usize];

                                    let r = blerp
                                    (
                                        ((p00 >> 16) & 0xFF) as f32,
                                        ((p10 >> 16) & 0xFF) as f32,
                                        ((p01 >> 16) & 0xFF) as f32,
                                        ((p11 >> 16) & 0xFF) as f32,
                                        fx, fy,
                                    ) as u32;
                                    let g = blerp
                                    (
                                        ((p00 >> 8) & 0xFF) as f32,
                                        ((p10 >> 8) & 0xFF) as f32,
                                        ((p01 >> 8) & 0xFF) as f32,
                                        ((p11 >> 8) & 0xFF) as f32,
                                        fx, fy,
                                    ) as u32;
                                    let b = blerp
                                    (
                                        (p00 & 0xFF) as f32,
                                        (p10 & 0xFF) as f32,
                                        (p01 & 0xFF) as f32,
                                        (p11 & 0xFF) as f32,
                                        fx, fy,
                                    ) as u32;

                                    buffer[(dst_row + dx) as usize] = 0xFF000000 | (r << 16) | (g << 8) | b;
                                }
                            }
                        }

                        buffer.present().ok();
                    }
                    _ => {},
                }
            }

            WindowEvent::CloseRequested =>
            {
                self.running.store(false, Ordering::Relaxed);
                event_loop.exit();
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop)
    {
        self.process_pending_frames();
    }
}

#[inline(always)]
fn blerp //BILINEAR INTERPOLATION BETWEEN 4 CORNER VALUES
(
    c00: f32,
    c10: f32,
    c01: f32,
    c11: f32,
    fx: f32,
    fy: f32,
) -> f32
{
    let top = c00 + (c10 - c00) * fx;
    let bot = c01 + (c11 - c01) * fx;
    top + (bot - top) * fy
}
