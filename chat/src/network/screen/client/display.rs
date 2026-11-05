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

//! # Screen Share Viewer Window
//!
//! The winit application that shows watched shares, one window per watched share.
//!
//! A batch is decoded from its newest keyframe, since H.264 frames reference each other, but only
//! the newest decoded frame of the batch is uploaded and drawn. A backlog with no keyframe older
//! than [`STALE_FRAME_AGE`](consts::STALE_FRAME_AGE) is dropped whole and a fresh one asked for,
//! rather than decoded into a picture that is already late. The window's title shows the frame
//! rate. A window the user closes detaches on the server; a session whose attachment ended is
//! kept briefly so its last frame is not torn down mid-draw.

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

use openh264::decoder::Decoder;

use crate::
{
    t,
    network::screen::
    {
        consts,
        client::{ ScreenShareRequest, UserEvent, ViewerReports, VideoFrame, video::VideoSurface },
    },
};

//PRIVATE
//STRUCTS
/// One watched share and its window.
struct Session
{
    /// The window.
    window: Arc<Window>,
    /// The renderer painting it.
    surface: VideoSurface,
    /// Encoded frames from the network reader.
    frame_rx: Receiver<VideoFrame>,
    /// What the sharer is told about this window.
    reports: Arc<ViewerReports>,
    /// The H.264 decoder.
    decoder: Decoder,
    /// Whether a gap was dropped, so only a keyframe can resume.
    awaiting_key: bool,
    /// Whether a new frame is waiting to be drawn.
    frame_dirty: bool,
    /// Cleared when the attachment ends.
    running: Arc<AtomicBool>,
    /// Signals the user closed the window.
    deattach: UnboundedSender<()>,
    /// Frames shown since the title was last updated.
    frame_count: u32,
    /// When the title was last updated.
    last_fps_time: Instant,
    /// When the session was found ended; it is dropped shortly after.
    close_requested_at: Option<Instant>,
}

//IMPLEMENTATIONS
impl Session
{
    /// Decodes the newest run of frames and uploads the newest picture.
    ///
    /// A backlog older than [`STALE_FRAME_AGE`](consts::STALE_FRAME_AGE) with no keyframe in it
    /// is dropped whole, keeping the last picture and asking for a new keyframe.
    ///
    /// # Returns
    /// `true` if a picture was uploaded.
    fn process_pending_frames(&mut self) -> bool
    {
        let mut pending: Vec<VideoFrame> = Vec::new();
        while let Ok(frame) = self.frame_rx.try_recv()
        {
            pending.push(frame);
        }

        if pending.is_empty() { return false; }

        self.reports.note_depth(pending.len());

        let keyframe_at = pending.iter().rposition(|frame| frame.keyframe);

        //AN OLD BACKLOG IS NOT WORTH DECODING WITHOUT A FRESH START
        if keyframe_at.is_none() && pending[0].received.elapsed() >= consts::STALE_FRAME_AGE
        {
            self.awaiting_key = true;
            self.reports.note_dropped(pending.len());
            self.reports.request_keyframe();

            return false;
        }

        //AFTER A GAP ONLY A KEYFRAME CAN RESUME THE PICTURE
        if self.awaiting_key && keyframe_at.is_none()
        {
            self.reports.note_dropped(pending.len());
            self.reports.request_keyframe();

            return false;
        }

        //A KEYFRAME MAKES EVERYTHING BEFORE IT USELESS
        let start = keyframe_at.unwrap_or(0);
        if start > 0 { self.reports.note_dropped(start); }

        let newest = pending.len() - 1;
        let mut uploaded = false;
        let mut decode_time = Duration::ZERO;

        for (index, frame) in pending[start..].iter().enumerate()
        {
            let began = Instant::now();
            let decoded = self.decoder.decode(&frame.data);
            decode_time = decode_time.max(began.elapsed());

            match decoded
            {
                Ok(Some(yuv)) =>
                {
                    if frame.keyframe
                    {
                        self.awaiting_key = false;
                        self.reports.keyframe_seen();
                    }

                    //ONLY THE NEWEST FRAME IS UPLOADED
                    if start + index != newest { continue; }

                    //THE PLANES GO UP AS THEY ARE
                    self.surface.upload(&yuv);

                    uploaded = true;
                },

                Ok(None) => {},

                //THE REFERENCES ARE GONE
                Err(_) =>
                {
                    self.awaiting_key = true;
                    self.reports.note_dropped(pending.len() - (start + index));
                    self.reports.request_keyframe();

                    break;
                }
            }
        }

        self.reports.note_decode(decode_time);

        if let Some(frame) = pending.last() { self.reports.note_age(frame.received.elapsed()); }

        self.frame_dirty = uploaded;

        uploaded
    }

    /// Resizes the surface to the window and draws.
    fn redraw(&mut self)
    {
        let size = self.window.inner_size();

        self.surface.resize(size.width, size.height);
        self.surface.render();
    }
}

//PUBLIC
//STRUCTS
/// The winit application holding every open viewer window.
pub struct ScreenShareApp //DISPATCHER
{
    /// Open sessions, by window.
    sessions: HashMap<WindowId, Session>,
}

//IMPLEMENTATIONS
impl ScreenShareApp
{
    /// Creates an application with no windows.
    pub fn new() -> Self
    {
        Self
        {
            sessions: HashMap::new(),
        }
    }

    /// Opens a window for a new viewer session.
    fn create_session(&mut self, event_loop: &ActiveEventLoop, request: ScreenShareRequest)
    {
        //CHECK IF ANOTHER ATTACH IS ALIVE
        if let Some(session) = self.sessions.values_mut().next()
        {
            let old_running = session.running.clone();

            //RESET
            session.frame_rx = request.rx;
            session.reports = request.reports;
            session.running = request.running;
            session.deattach = request.deattach;
            session.decoder = Decoder::new().expect("Failed to create H.264 decoder");
            session.awaiting_key = false;
            session.frame_dirty = false;
            session.frame_count = 0;
            session.last_fps_time = Instant::now();
            session.close_requested_at = None;

            session.window.request_redraw();
            old_running.store(false, Ordering::Relaxed);

            return;
        }

        let attrs = WindowAttributes::default()
            .with_title(t!("screen.window"))
            .with_inner_size(PhysicalSize::new(consts::WINIT_SIZE.0, consts::WINIT_SIZE.1));

        let Ok(window) = event_loop.create_window(attrs) else { return; };
        let window = Arc::new(window);

        let size = window.inner_size();

        //DROP A WINDOW WE CANNOT PAINT
        let Ok(surface) = VideoSurface::new(window.clone(), size.width, size.height) else { return; };

        let window_id = window.id();
        window.request_redraw();

        self.sessions.insert(window_id, Session
        {
            window,
            surface,
            frame_rx: request.rx,
            reports: request.reports,
            decoder: Decoder::new().expect("Failed to create H.264 decoder"),
            awaiting_key: false,
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
                    session.surface.resize(size.width, size.height);
                    session.window.request_redraw();
                }
            },

            WindowEvent::CloseRequested =>
            {
                if let Some(session) = self.sessions.remove(&window_id)
                {
                    session.running.store(false, Ordering::Relaxed);

                    //DEATTACH ON SERVER
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
                    session.window.set_title(&t!("screen.window_fps", fps = session.frame_count));
                    session.frame_count = 0;
                    session.last_fps_time = Instant::now();
                }

                session.window.request_redraw();
            }
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + consts::FRAME_POLL_INTERVAL));
    }
}
