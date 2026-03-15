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
    thread,
    process,
    time::Duration,
    os::unix::io::AsFd,
    sync::{ Arc, Mutex },
    fs::
    {
        self,
        File,
        OpenOptions,
    },
};

use memmap2::MmapMut;
use wayland_client::
{
    delegate_noop,
    Connection,
    Dispatch,
    QueueHandle,
    WEnum,
    protocol::
    {
        wl_buffer,
        wl_output,
        wl_registry,
        wl_shm,
        wl_shm_pool
    },
};

use wayland_protocols_wlr::screencopy::v1::client::
{
    zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
    zwlr_screencopy_frame_v1::{ self, ZwlrScreencopyFrameV1 },
};

use crate::network::screen::client::
{
    CaptureInfo,
    SharedFrame,
    compress,
};

//WL STATE
#[derive(Clone, Copy)]
struct BufInfo
{
    format: wl_shm::Format,
    width:  u32,
    height: u32,
    stride: u32,
}

struct WlState
{
    shm: Option<wl_shm::WlShm>,
    manager: Option<ZwlrScreencopyManagerV1>,
    outputs: Vec<wl_output::WlOutput>,
    buf_info: Option<BufInfo>,
    frame_done: bool,
    frame_failed: bool,
}

impl WlState
{
    fn new() -> Self
    {
        Self
        {
            shm: None,
            manager: None,
            outputs: Vec::new(),
            buf_info: None,
            frame_done: false,
            frame_failed: false,
        }
    }

    fn reset_frame(&mut self)
    {
        self.buf_info = None;
        self.frame_done = false;
        self.frame_failed = false;
    }
}

//DISPATCH IMPLEMENTATION
impl Dispatch<wl_registry::WlRegistry, ()> for WlState
{
    fn event
    (
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event, _: &(), _: &Connection,
        qh: &QueueHandle<Self>,
    )
    {
        if let wl_registry::Event::Global { name, interface, version } = event
        {
            match interface.as_str()
            {
                "wl_shm" =>
                {
                    state.shm = Some(registry.bind(name, 1, qh, ()));
                },

                "zwlr_screencopy_manager_v1" =>
                {
                    state.manager = Some(registry.bind(name, 1, qh, ()));
                },

                "wl_output" =>
                {
                    let o: wl_output::WlOutput = registry.bind(name, version.min(4), qh, ());
                    state.outputs.push(o);
                },

                _ => {}
            }
        }
    }
}

impl Dispatch<ZwlrScreencopyFrameV1, ()> for WlState
{
    fn event
    (
        state: &mut Self, _: &ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>
    )
    {
        match event
        {
            zwlr_screencopy_frame_v1::Event::Buffer { format, width, height, stride } =>
            {
                if let WEnum::Value(fmt) = format
                {
                    if matches!(fmt, wl_shm::Format::Xrgb8888 | wl_shm::Format::Argb8888)
                    {
                        state.buf_info = Some(BufInfo { format: fmt, width, height, stride });
                    }
                }
            },

            zwlr_screencopy_frame_v1::Event::Ready { .. }  => { state.frame_done  = true; },
            zwlr_screencopy_frame_v1::Event::Failed => { state.frame_failed = true; },
            _ => {}
        }
    }
}

delegate_noop!(WlState: ignore wl_shm::WlShm);
delegate_noop!(WlState: ignore wl_output::WlOutput);
delegate_noop!(WlState: ignore ZwlrScreencopyManagerV1);
delegate_noop!(WlState: ignore wl_shm_pool::WlShmPool);
delegate_noop!(WlState: ignore wl_buffer::WlBuffer);

//SHM
fn create_shm_file(size: usize) -> File
{
    for dir in ["/dev/shm", "/tmp"]
    {
        let path = format!("{}/screen-mirror-{}", dir, process::id());
        if let Ok(f) = OpenOptions::new().read(true).write(true).create(true).open(&path)
        {
            let _ = fs::remove_file(&path);
            f.set_len(size as u64).expect("Failed to set SHM file len");
            return f;
        }
    }

    panic!("Failed to create SHM file");
}

//MAIN
pub fn start(monitor_idx: usize) -> (CaptureInfo, SharedFrame)
{
    let conn = Connection::connect_to_env().expect("Failed to connect to wayland");

    let mut eq = conn.new_event_queue::<WlState>();
    let qh = eq.handle();
    conn.display().get_registry(&qh, ());

    let mut st = WlState::new();
    eq.roundtrip(&mut st).unwrap();

    //PROTOCOL CHECK
    let manager = st.manager.take().expect("Unsupported protocol");
    let shm = st.shm.take().expect("wl_shm not found");

    let output = st.outputs[monitor_idx].clone();

    //PROBE FRAME
    let probe = manager.capture_output(1, &output, &qh, ()); //1 = OVERLAY CURSOR
    while st.buf_info.is_none() && !st.frame_failed
    {
        eq.blocking_dispatch(&mut st).unwrap();
    }
    probe.destroy();

    let info = st.buf_info.unwrap();
    let src_w = info.width  as usize;
    let src_h = info.height as usize;
    let src_stride = info.stride as usize;
    let buf_size= src_stride * src_h;

    //SHM BUFFER
    let shm_file = create_shm_file(buf_size);
    let mmap = unsafe { MmapMut::map_mut(&shm_file).unwrap() };

    let pool = shm.create_pool(shm_file.as_fd(), buf_size as i32, &qh, ());
    let wl_buf = pool.create_buffer
    (
        0,
        src_w as i32,
        src_h as i32,
        src_stride as i32,
        info.format,
        &qh,
        (),
    );

    //SHARED BUFFER
    let shared: SharedFrame = Arc::new(Mutex::new((Vec::new(), false)));
    let shared_cap = shared.clone();

    //CAPTURE THREAD
    thread::spawn(move ||
    {
        let mut local_raw: Vec<u8> = vec![0u8; buf_size];

        loop
        {
            st.reset_frame();

            //OVERLAY CURSOR
            let frame = manager.capture_output(1, &output, &qh, ());

            while st.buf_info.is_none() && !st.frame_failed
            {
                if eq.blocking_dispatch(&mut st).is_err() { return; }
            }

            if st.frame_failed
            {
                frame.destroy();
                thread::sleep(Duration::from_millis(50));
                continue;
            }

            frame.copy(&wl_buf);

            while !st.frame_done && !st.frame_failed
            {
                if eq.blocking_dispatch(&mut st).is_err() { return; }
            }
            frame.destroy();

            if st.frame_done
            {
                //RAW DATA
                local_raw.copy_from_slice(&mmap[..buf_size]);

                //COMPRESS
                let local_compressed = compress(&local_raw);

                //SWAP
                let mut g = shared_cap.lock().unwrap();
                g.0 = local_compressed;
                g.1 = true;
            }
        }
    });

    (CaptureInfo { width: src_w, height: src_h, stride: src_stride }, shared)
}
