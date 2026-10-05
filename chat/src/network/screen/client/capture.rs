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
    env,
    thread,
    time::{ Duration, Instant },
    sync::
    {
        Arc,
        Condvar,
        Mutex,
        RwLock,
        atomic::{ AtomicBool, Ordering },
        mpsc::Receiver,
    },
};

#[cfg(not(target_os = "macos"))]
use std::sync::mpsc;

#[cfg(target_os = "linux")]
use std::fs::File;

#[cfg(target_os = "linux")]
use memmap2::Mmap;

use std::sync::mpsc::RecvTimeoutError;

use tokio::sync::mpsc::Sender;

use xcap::{ Frame, Monitor, VideoRecorder };

use openh264::
{
    OpenH264API,
    formats::{ BgraSliceU8, RgbaSliceU8, YUVBuffer },
    encoder::
    {
        Encoder,
        EncoderConfig,
        BitRate,
        FrameRate,
        IntraFramePeriod,
        Complexity,
        UsageType,
        RateControlMode,
    },
};

use crate::
{
    t,
    network::screen::
    {
        consts,
        client::options,
    },
};

#[cfg(target_os = "linux")]
use crate::network::screen::client::portal::PortalRecorder;

fn monitor_name(monitor: &Monitor) -> String
{
    monitor.name().unwrap_or_else(|_| t!("screen.unknown_monitor").to_owned())
}

fn monitor_list(monitors: &[Monitor]) -> String //THE MONITORS AS THE USER MAY NAME THEM
{
    monitors.iter().enumerate()
        .map(|(index, monitor)| format!("{} ({})", index + 1, monitor_name(monitor)))
        .collect::<Vec<String>>()
        .join(", ")
}

//THE MONITOR ASKED FOR, BY 1-BASED INDEX OR NAME
fn select_monitor(monitors: Vec<Monitor>, selection: &str) -> Result<Monitor, String>
{
    if let Ok(index) = selection.parse::<usize>()
        && let Some(monitor) = index.checked_sub(1).and_then(|index| monitors.get(index))
    {
        return Ok(monitor.clone());
    }

    monitors.iter()
        .find(|monitor| monitor_name(monitor).eq_ignore_ascii_case(selection))
        .cloned()
        .ok_or_else(|| t!("screen.error.no_monitor", selection, available = monitor_list(&monitors)))
}

fn get_target_monitor() -> Result<Monitor, String> //THE MONITOR TO SHARE
{
    let monitors = Monitor::all().map_err(|error| t!("screen.error.enumerate", error))?;

    if monitors.is_empty() { return Err(t!("screen.error.no_monitors").to_owned()); }

    match options::get_monitor()
    {
        Some(selection) => select_monitor(monitors, &selection),

        None => Ok(monitors.iter()
            .find(|m| m.is_primary().unwrap_or(false))
            .cloned()
            .unwrap_or_else(|| monitors.into_iter().next().unwrap())),
    }
}

//THE NAME selection RESOLVES TO
pub fn resolve_monitor(selection: &str) -> Result<String, String>
{
    let monitors = Monitor::all().map_err(|error| t!("screen.error.enumerate", error))?;

    select_monitor(monitors, selection).map(|monitor| monitor_name(&monitor))
}

pub fn current_monitor() -> Option<String> //WHAT A SHARE WOULD CAPTURE RIGHT NOW, BY NAME
{
    get_target_monitor().ok().map(|monitor| monitor_name(&monitor))
}

//THE NAMES THE PALETTE OFFERS, CACHED
pub fn monitor_names() -> Vec<String>
{
    static CACHE: RwLock<Option<(Instant, Vec<String>)>> = RwLock::new(None);

    if let Some((taken, names)) = CACHE.read().unwrap().as_ref()
        && taken.elapsed() < consts::MONITOR_LIST_TTL
    {
        return names.clone();
    }

    let names = Monitor::all().map(|monitors| monitors.iter().map(monitor_name).collect::<Vec<String>>())
        .unwrap_or_default();

    *CACHE.write().unwrap() = Some((Instant::now(), names.clone()));

    names
}

//SET ONCE THE OS-NATIVE RECORDER HAS PROVEN ITSELF
static UPGRADING: AtomicBool = AtomicBool::new(false);

fn upgrading() -> bool
{
    UPGRADING.load(Ordering::Relaxed)
}

#[cfg(target_os = "linux")]
fn wayland() -> bool
{
    env::var("WAYLAND_DISPLAY").is_ok() || env::var("XDG_SESSION_TYPE").unwrap_or_default() == "wayland"
}

fn legacy_capture_loop //THE PRE-RECORDER POLLING PATH, KEPT AS THE LAST FALLBACK
(
    frame_tx: Sender<Vec<u8>>,
    running: Arc<AtomicBool>,
    fps: u32,
) -> Result<(), String>
{
    #[cfg(target_os = "linux")]
    if wayland()
    {
        return capture_loop_wayshot(frame_tx, running, fps);
    }

    capture_loop_xcap(get_target_monitor()?, frame_tx, running, fps)
}

pub fn capture_loop //CAPTURE LOOP
(
    frame_tx: Sender<Vec<u8>>,
    running: Arc<AtomicBool>,
    fps: u32,
) -> Result<(), String>
{
    loop
    {
        let generation = options::monitor_generation();

        let outcome = capture_backend(frame_tx.clone(), running.clone(), fps);

        //THE MONITOR CHANGED - START OVER ON THE NEW ONE
        if !switched(generation) || !running.load(Ordering::Relaxed) || !options::get_use_screen()
        {
            return outcome;
        }
    }
}

fn switched(generation: usize) -> bool //THE MONITOR WAS PICKED AGAIN WHILE WE WERE CAPTURING
{
    options::monitor_generation() != generation
}

fn capture_backend //PICK A BACKEND AND CAPTURE ON IT UNTIL IT STOPS
(
    frame_tx: Sender<Vec<u8>>,
    running: Arc<AtomicBool>,
    fps: u32,
) -> Result<(), String>
{
    //AN EXPLICIT OVERRIDE PINS A BACKEND
    match env::var(consts::BACKEND_OVERRIDE_VAR).unwrap_or_default().to_lowercase().as_str()
    {
        "recorder" => return capture_loop_recorder(frame_tx, running, fps),
        "legacy" | "xcap" | "wayshot" => return legacy_capture_loop(frame_tx, running, fps),
        _ => {},
    }

    //ON WAYLAND A PICKED MONITOR PINS THE POLLING PATH, WHERE THERE IS ONE
    #[cfg(target_os = "linux")]
    if wayland() && options::get_monitor().is_some()
    {
        let outcome = legacy_capture_loop(frame_tx.clone(), running.clone(), fps);

        if outcome.is_ok() || !running.load(Ordering::Relaxed) { return outcome; }
    }

    //SOME OBJECTIVE-C BULLSHIT ON MAC
    #[cfg(target_os = "macos")]
    return match open_recorder(fps)
    {
        Ok(session) => run_recorder(session, frame_tx, running, fps),
        Err(_) => legacy_capture_loop(frame_tx, running, fps),
    };

    #[cfg(not(target_os = "macos"))]
    {
        UPGRADING.store(false, Ordering::Relaxed);

        let (probe_tx, probe_rx) = mpsc::channel();

        thread::spawn(move ||
        {
            let session = open_recorder(fps);

            //THE FLAG GOES UP BEFORE THE SEND
            if session.is_ok() { UPGRADING.store(true, Ordering::Relaxed); }

            probe_tx.send(session).ok();
        });

        let outcome = legacy_capture_loop(frame_tx.clone(), running.clone(), fps);

        //ENDED ON ITS OWN TERMS
        if !upgrading() && (outcome.is_ok() || !running.load(Ordering::Relaxed)) { return outcome; }

        let probed = if upgrading()
        {
            probe_rx.recv().ok()
        } else
        {
            //THE POLLING PATH COULD NOT RUN AT ALL
            probe_rx.recv_timeout(probe_timeout()).ok()
        };

        UPGRADING.store(false, Ordering::Relaxed);

        match probed
        {
            //TAKE A PROVEN RECORDER EVEN AFTER A POLLING ERROR
            Some(Ok(session)) if running.load(Ordering::Relaxed) => run_recorder(session, frame_tx, running, fps),

            //NEITHER BACKEND RAN
            Some(Err(recorder)) => outcome.map_err(|polling| t!("screen.error.no_backend", recorder, polling)),
            None => outcome.map_err(|polling| t!("screen.error.no_backend", recorder = t!("screen.error.recorder_timeout"), polling)),

            _ => outcome,
        }
    }
}

fn create_encoder(fps: f32) -> Result<Encoder, String>
{
    let config = EncoderConfig::new()
        .max_frame_rate(FrameRate::from_hz(fps))
        .rate_control_mode(RateControlMode::Bitrate)
        .bitrate(BitRate::from_bps(consts::H264_BITRATE))
        .intra_frame_period(IntraFramePeriod::from_num_frames((fps * 2.0) as u32))
        .complexity(Complexity::Low)
        .usage_type(UsageType::CameraVideoRealTime)
        .skip_frames(true)
        .adaptive_quantization(false)
        .background_detection(false);

    Encoder::with_api_config(OpenH264API::from_source(), config)
        .map_err(|error| t!("screen.error.encoder", error))
}

struct YuvScratch //REUSABLE I420 SCRATCH BUFFER
{
    buffer: YUVBuffer,
    width: u32,
    height: u32,
}

impl YuvScratch
{
    fn new() -> Self
    {
        Self { buffer: YUVBuffer::new(0, 0), width: 0, height: 0 }
    }

    fn fill(&mut self, width: u32, height: u32, pixels: &[u8], order: PixelOrder) -> &YUVBuffer
    {
        //RESIZE ONLY ON A REAL RESOLUTION CHANGE
        if self.width != width || self.height != height
        {
            self.buffer = YUVBuffer::new(width as usize, height as usize);
            self.width = width;
            self.height = height;
        }

        let dimensions = (width as usize, height as usize);

        //SIMD WHERE THE CPU HAS IT
        match order
        {
            PixelOrder::Rgba => self.buffer.read_rgba8(RgbaSliceU8::new(pixels, dimensions)),
            PixelOrder::Bgra => self.buffer.read_bgra8(BgraSliceU8::new(pixels, dimensions)),
        }

        &self.buffer
    }
}

struct FrameEncoder
{
    encoder: Encoder,
    scratch: YuvScratch,
    fps: f32,
    dimensions: Option<(u32, u32)>,
}

impl FrameEncoder
{
    fn new(fps: f32) -> Result<Self, String>
    {
        Ok(Self { encoder: create_encoder(fps)?, scratch: YuvScratch::new(), fps, dimensions: None })
    }

    fn force_intra_frame(&mut self)
    {
        self.encoder.force_intra_frame();
    }

    fn encode(&mut self, width: u32, height: u32, pixels: &[u8], order: PixelOrder) -> Result<Option<Vec<u8>>, String>
    {
        //I420 CONVERSION PANICS ON ODD DIMENSIONS
        if width % 2 != 0 || height % 2 != 0
        {
            return Err(t!("screen.error.resolution", width, height));
        }

        //EXACTLY ONE PICTURE OF PIXELS
        let Some(pixels) = pixels.get(..width as usize * height as usize * 4) else
        {
            return Err(t!("screen.error.resolution", width, height));
        };

        //A RESIZE NEEDS A FRESH ENCODER
        if self.dimensions.is_some_and(|previous| previous != (width, height))
        {
            self.encoder = create_encoder(self.fps)?;
        }

        self.dimensions = Some((width, height));

        let yuv = self.scratch.fill(width, height, pixels, order);

        let data = self.encoder.encode(yuv)
            .map_err(|error| t!("screen.error.encode", error))?
            .to_vec();

        //SKIP EMPTY FRAMES
        if data.is_empty()
        {
            return Ok(None);
        }

        Ok(Some(data))
    }

    fn dispatch(&mut self, frame_tx: &Sender<Vec<u8>>, frame: Vec<u8>) //HAND A FRAME TO THE NETWORK TASK
    {
        //THIS FRAME IS GONE, SO THE NEXT CANNOT PREDICT IT
        if frame_tx.try_send(frame).is_err()
        {
            self.force_intra_frame();
        }
    }
}

fn sleep_until_next_tick(next_tick: &mut Instant, target_interval: Duration)
{
    let now = Instant::now();
    if *next_tick > now
    {
        thread::sleep(*next_tick - now);
    } else
    {
        *next_tick = now;
    }

    *next_tick += target_interval;
}

fn capture_loop_xcap
(
    monitor: Monitor,
    frame_tx: Sender<Vec<u8>>,
    running: Arc<AtomicBool>,
    fps: u32,
) -> Result<(), String>
{
    let target_interval = Duration::from_secs_f64(1.0 / fps as f64);
    let mut next_tick = Instant::now() + target_interval;

    let generation = options::monitor_generation();

    let mut encoder = FrameEncoder::new(fps as f32)?;

    //PREVIOUS FRAME, KEPT BY MOVE
    let mut last_image: Option<xcap::image::RgbaImage> = None;
    let mut last_encode_time = Instant::now();

    while running.load(Ordering::Relaxed) && !upgrading() && !switched(generation)
    {
        //EXIT ON DISABLED SCREEN
        if !options::get_use_screen()
        {
            running.store(false, Ordering::Relaxed);
            return Ok(());
        }

        if let Ok(image) = monitor.capture_image()
        {
            let force_encode = last_encode_time.elapsed() >= consts::FORCED_INTRA_INTERVAL;

            //CHEAP WHEN THE SCREEN MOVED - memcmp EARLY-EXITS
            let changed = last_image.as_ref().is_none_or(|previous| previous.as_raw() != image.as_raw());

            if force_encode || changed
            {
                if let Some(compressed) = encoder.encode(image.width(), image.height(), image.as_raw(), PixelOrder::Rgba)?
                {
                    encoder.dispatch(&frame_tx, compressed);
                }

                last_image = Some(image);
                last_encode_time = Instant::now();
            }
        }

        sleep_until_next_tick(&mut next_tick, target_interval);
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn select_output(wayshot: &libwayshot::WayshotConnection) -> Result<libwayshot::output::OutputInfo, String> //PICK THE OUTPUT TO SHARE
{
    let outputs = wayshot.get_all_outputs();

    if outputs.is_empty() { return Err(t!("screen.error.no_outputs").to_owned()); }

    //FIND THE PICKED OUTPUT, FAILING IF IT IS GONE
    let picked = options::get_monitor().is_some();

    match get_target_monitor().and_then(|m| m.name().map_err(|e| e.to_string()))
    {
        Ok(name) => match outputs.iter().find(|o| o.name == name)
        {
            Some(output) => return Ok(output.clone()),
            None if picked => return Err(t!("screen.error.no_output", name)),
            None => {},
        },

        Err(reason) if picked => return Err(reason),
        Err(_) => {},
    }

    //FALLBACK: THE OUTPUT AT THE LAYOUT ORIGIN
    Ok(outputs.iter()
        .find(|o| o.logical_region.inner.position.x == 0 && o.logical_region.inner.position.y == 0)
        .unwrap_or(&outputs[0])
        .clone())
}

//A FRESH CONNECTION ONTO THE SAME OUTPUT
#[cfg(target_os = "linux")]
fn reconnect_wayshot(name: &str) -> Option<(libwayshot::WayshotConnection, libwayshot::output::OutputInfo)>
{
    let connection = libwayshot::WayshotConnection::new().ok()?;
    let output = connection.get_all_outputs().iter().find(|output| output.name == name).cloned()?;

    Some((connection, output))
}

//WL_SHM FORMAT CODES
#[cfg(target_os = "linux")]
const SHM_ARGB8888: u32 = 0;
#[cfg(target_os = "linux")]
const SHM_XRGB8888: u32 = 1;
#[cfg(target_os = "linux")]
const SHM_ABGR8888: u32 = 0x34324241;
#[cfg(target_os = "linux")]
const SHM_XBGR8888: u32 = 0x34324258;

#[cfg(target_os = "linux")]
struct ShmCapture //ONE REUSED BUFFER THE COMPOSITOR COPIES INTO
{
    file: File,
    len: u64,
    map: Option<Mmap>,
}

#[cfg(target_os = "linux")]
impl ShmCapture
{
    fn new(bytes: u64) -> Option<Self>
    {
        let fd = rustix::fs::memfd_create("why2-capture", rustix::fs::MemfdFlags::CLOEXEC).ok()?;
        let file = File::from(fd);

        file.set_len(bytes).ok()?;

        Some(Self { file, len: bytes, map: None })
    }

    //ONE FRAME INTO pixels, OR None WHERE ONLY libwayshot CAN READ IT
    fn grab
    (
        &mut self,
        wayshot: &libwayshot::WayshotConnection,
        output: &libwayshot::OutputInfo,
        pixels: &mut Vec<u8>,
    ) -> Result<Option<(u32, u32, PixelOrder)>, libwayshot::Error>
    {
        if output.transform != libwayshot::reexport::Transform::Normal { return Ok(None); }

        let (format, guard) = wayshot.capture_output_frame_shm_fd(&output.wl_output, 1, &self.file, None)?;

        let order = match u32::from(format.format)
        {
            SHM_XRGB8888 | SHM_ARGB8888 => PixelOrder::Bgra,
            SHM_XBGR8888 | SHM_ABGR8888 => PixelOrder::Rgba,
            _ => return Ok(None),
        };

        if format.byte_size() > self.len { return Ok(None); }

        if self.map.is_none()
        {
            //SAFETY: OUR OWN MEMFD
            self.map = Some(unsafe { Mmap::map(&self.file) }?);
        }

        let Some(map) = &self.map else { return Ok(None) };

        let (width, height) = (format.size.width, format.size.height);

        if !pack_rows(map, width as usize, height as usize, format.stride as usize, pixels) { return Ok(None); }

        drop(guard);

        Ok(Some((width, height, order)))
    }
}

//COPY width x height PIXELS OUT OF ROWS stride BYTES APART
pub fn pack_rows(source: &[u8], width: usize, height: usize, stride: usize, pixels: &mut Vec<u8>) -> bool
{
    let row = width * 4;

    if width == 0 || height == 0 || stride < row || source.len() < stride * (height - 1) + row { return false; }

    pixels.clear();

    if stride == row
    {
        pixels.extend_from_slice(&source[..row * height]);
    } else
    {
        for line in source.chunks(stride).take(height) { pixels.extend_from_slice(&line[..row]); }
    }

    true
}

#[cfg(target_os = "linux")]
fn capture_loop_wayshot
(
    frame_tx: Sender<Vec<u8>>,
    running: Arc<AtomicBool>,
    fps: u32,
) -> Result<(), String>
{
    let target_interval = Duration::from_secs_f64(1.0 / fps as f64);

    let generation = options::monitor_generation();

    let mut wayshot = libwayshot::WayshotConnection::new()
        .map_err(|error| t!("screen.error.wayland", error))?;

    let mut target_output = select_output(&wayshot)?;

    let mut encoder = FrameEncoder::new(fps as f32)?;

    //PROBE ONCE SO A BAD COMPOSITOR REPORTS AN ERROR
    let first_image = wayshot.screenshot_single_output(&target_output, true)
        .map_err(|error| t!("screen.error.wayland_capture", output = target_output.name, error))?
        .into_rgba8();

    //ENCODE AND SEND FIRST FRAME
    if let Some(compressed) = encoder.encode(first_image.width(), first_image.height(), first_image.as_raw(), PixelOrder::Rgba)?
    {
        encoder.dispatch(&frame_tx, compressed);
    }

    //ROOM FOR THE PICTURE PLUS ANY ROW PADDING
    let reserve = first_image.height() as u64 * (first_image.width() as u64 * 4 + consts::SHM_ROW_SLACK);
    let mut shm = ShmCapture::new(reserve);

    //PREVIOUS FRAME, AND THE BUFFER THE NEXT ONE GOES INTO
    let mut last_raw = Some(first_image.into_raw());
    let mut pixels = Vec::new();

    let mut last_encode_time = Instant::now();

    let mut failures = 0u32;
    let mut next_tick = Instant::now() + target_interval;

    //COUNT THE STRANDED BYTES AND RECONNECT
    let mut stranded = 0u64;

    while running.load(Ordering::Relaxed) && !upgrading() && !switched(generation)
    {
        //EXIT ON DISABLED SCREEN
        if !options::get_use_screen()
        {
            running.store(false, Ordering::Relaxed);
            return Ok(());
        }

        let grabbed = match shm.as_mut().map(|shm| shm.grab(&wayshot, &target_output, &mut pixels))
        {
            Some(Ok(Some(frame))) => Ok(frame),
            Some(Err(error)) => Err(error),

            //libwayshot's OWN CONVERSION FROM HERE ON
            _ =>
            {
                shm = None;

                wayshot.screenshot_single_output(&target_output, true).map(|image|
                {
                    let image = image.into_rgba8();
                    let (width, height) = image.dimensions();

                    pixels = image.into_raw();

                    (width, height, PixelOrder::Rgba)
                })
            },
        };

        match grabbed
        {
            Ok((width, height, order)) =>
            {
                failures = 0;

                stranded += pixels.len() as u64;

                let force_encode = last_encode_time.elapsed() >= consts::FORCED_INTRA_INTERVAL;

                let changed = last_raw.as_ref().is_none_or(|previous| previous != &pixels);

                if force_encode || changed
                {
                    if let Some(compressed) = encoder.encode(width, height, &pixels, order)?
                    {
                        encoder.dispatch(&frame_tx, compressed);
                    }

                    //SWAP RATHER THAN REALLOCATE
                    pixels = last_raw.replace(std::mem::take(&mut pixels)).unwrap_or_default();
                    last_encode_time = Instant::now();
                }

                //NOTHING WAS MISSED
                if stranded >= consts::WAYLAND_LEAK_BUDGET
                {
                    if let Some((connection, output)) = reconnect_wayshot(&target_output.name)
                    {
                        wayshot = connection;
                        target_output = output;
                    }

                    stranded = 0;
                }
            },

            //RECONNECT ONLY WHEN CAPTURE BREAKS
            Err(_) =>
            {
                failures += 1;

                if failures >= consts::WAYLAND_RECONNECT_FAILURES
                {
                    if let Some((connection, output)) = reconnect_wayshot(&target_output.name)
                    {
                        wayshot = connection;
                        target_output = output;

                        //FORCE A KEYFRAME - FRAMES WERE MISSED
                        encoder.force_intra_frame();
                        last_raw = None;
                    }

                    stranded = 0;
                    failures = 0;
                }
            },
        }

        sleep_until_next_tick(&mut next_tick, target_interval);
    }

    Ok(())
}

//ENUMS
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PixelOrder //BYTE ORDER OF A CAPTURED PIXEL
{
    Rgba,
    Bgra,
}

enum Recorder //WHO IS DELIVERING THE FRAMES
{
    Xcap(VideoRecorder),

    #[cfg(target_os = "linux")]
    Portal(PortalRecorder),
}

//STRUCTS
pub struct CapturedFrame //ONE PICTURE, TIGHTLY PACKED
{
    pub width: u32,
    pub height: u32,
    pub order: PixelOrder,
    pub data: Vec<u8>,
}

pub struct LatestFrame //ONE-SLOT FRAME HANDOFF
{
    slot: Mutex<Option<CapturedFrame>>,
    spare: Mutex<Vec<Vec<u8>>>, //BUFFERS TO REUSE
    ready: Condvar,
    draining: AtomicBool, //THE LOOP STILL WANTS FRAMES
    ended: AtomicBool,    //THE RECORDER STOPPED DELIVERING
}

impl LatestFrame
{
    fn new() -> Self
    {
        Self
        {
            slot: Mutex::new(None),
            spare: Mutex::new(Vec::new()),
            ready: Condvar::new(),
            draining: AtomicBool::new(true),
            ended: AtomicBool::new(false),
        }
    }

    fn take(&self, timeout: Duration) -> Option<CapturedFrame> //THE NEWEST FRAME, OR NOTHING
    {
        let (mut slot, _) = self.ready
            .wait_timeout_while(self.slot.lock().unwrap(), timeout, |slot| slot.is_none())
            .unwrap();

        slot.take()
    }

    fn try_take(&self) -> Option<CapturedFrame>
    {
        self.slot.lock().unwrap().take()
    }

    pub fn put(&self, frame: CapturedFrame) //REPLACE THE WAITING FRAME
    {
        let stale = self.slot.lock().unwrap().replace(frame);

        self.ready.notify_one();

        if let Some(stale) = stale { self.recycle(stale.data); }
    }

    pub fn buffer(&self) -> Vec<u8> //A USED BUFFER, OR A NEW ONE
    {
        self.spare.lock().unwrap().pop().unwrap_or_default()
    }

    pub fn recycle(&self, data: Vec<u8>)
    {
        let mut spare = self.spare.lock().unwrap();

        if spare.len() < consts::SPARE_FRAMES { spare.push(data); }
    }

    pub fn end(&self)
    {
        self.ended.store(true, Ordering::Relaxed);
        self.ready.notify_one();
    }
}

//DRAIN THE RECORDER ALONGSIDE THE ENCODE
fn drain_frames(frames: Receiver<Frame>, latest: Arc<LatestFrame>)
{
    thread::spawn(move ||
    {
        while latest.draining.load(Ordering::Relaxed)
        {
            match frames.recv_timeout(consts::RECORDER_POLL_INTERVAL)
            {
                //KEEP ONLY THE NEWEST
                Ok(frame) => latest.put(CapturedFrame
                {
                    width: frame.width,
                    height: frame.height,
                    order: PixelOrder::Rgba,
                    data: frame.raw,
                }),

                Err(RecvTimeoutError::Timeout) => {},
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        latest.end();
    });
}

struct RecorderSession //A STARTED OS-NATIVE RECORDER AND ITS FRAMES
{
    recorder: Recorder,
    latest: Arc<LatestFrame>,
    first: Option<CapturedFrame>,
}

impl Drop for RecorderSession
{
    fn drop(&mut self)
    {
        self.latest.draining.store(false, Ordering::Relaxed);

        match &mut self.recorder
        {
            Recorder::Xcap(recorder) => { recorder.stop().ok(); },

            #[cfg(target_os = "linux")]
            Recorder::Portal(recorder) => recorder.stop(),
        }
    }
}

//THE PORTAL, ASKED FOR NO MORE FRAMES THAN WE ENCODE
#[cfg(target_os = "linux")]
fn open_portal(fps: u32) -> Result<RecorderSession, String>
{
    let latest = Arc::new(LatestFrame::new());

    let recorder = PortalRecorder::start(latest.clone(), fps)
        .map_err(|error| t!("screen.error.recorder_unavailable", error))?;

    //A STREAMING PORTAL IS PROOF ENOUGH
    let first = latest.try_take();

    Ok(RecorderSession { recorder: Recorder::Portal(recorder), latest, first })
}

fn open_recorder(fps: u32) -> Result<RecorderSession, String> //THE BLOCKING HALF OF THE PROBE
{
    #[cfg(target_os = "linux")]
    if wayland() { return open_portal(fps); }

    #[cfg(not(target_os = "linux"))]
    let _ = fps;

    let monitor = get_target_monitor()?;

    let (recorder, frames) = monitor.video_recorder()
        .map_err(|error| t!("screen.error.recorder_unavailable", error))?;

    recorder.start()
        .map_err(|error| t!("screen.error.recorder_start", error))?;

    let latest = Arc::new(LatestFrame::new());

    drain_frames(frames, latest.clone());

    first_frame(Recorder::Xcap(recorder), latest)
}

fn first_frame(recorder: Recorder, latest: Arc<LatestFrame>) -> Result<RecorderSession, String>
{
    let mut session = RecorderSession { recorder, latest, first: None };

    //DEMAND AN ACTUAL FRAME
    session.first = Some(session.latest.take(consts::RECORDER_FIRST_FRAME)
        .ok_or_else(|| t!("screen.error.recorder_silent").to_owned())?);

    Ok(session)
}

#[cfg(not(target_os = "macos"))]
fn probe_timeout() -> Duration
{
    env::var(consts::PROBE_TIMEOUT_VAR).ok()
        .and_then(|value| value.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(consts::RECORDER_PROBE_TIMEOUT)
}

//A macOS SESSION CANNOT CROSS A THREAD
#[cfg(target_os = "macos")]
fn start_recorder(fps: u32) -> Result<RecorderSession, String>
{
    open_recorder(fps)
}

#[cfg(not(target_os = "macos"))]
fn start_recorder(fps: u32) -> Result<RecorderSession, String> //PROBE THE OS-NATIVE RECORDER, BOUNDED
{
    //THE PROBE CAN BLOCK, SO IT GETS ITS OWN THREAD
    let (probe_tx, probe_rx) = mpsc::channel();

    thread::spawn(move ||
    {
        //DROP A LATE SESSION, RELEASING THE PORTAL
        probe_tx.send(open_recorder(fps)).ok();
    });

    match probe_rx.recv_timeout(probe_timeout())
    {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => Err(t!("screen.error.recorder_timeout").to_owned()),
        Err(RecvTimeoutError::Disconnected) => Err(t!("screen.error.recorder_probe").to_owned()),
    }
}

fn run_recorder //EVENT-DRIVEN CAPTURE LOOP
(
    mut session: RecorderSession,
    frame_tx: Sender<Vec<u8>>,
    running: Arc<AtomicBool>,
    fps: u32,
) -> Result<(), String>
{
    let latest = session.latest.clone();

    let mut encoder = FrameEncoder::new(fps as f32)?;

    let min_interval = Duration::from_secs_f64(1.0 / fps as f64);

    let mut last_encode_time = Instant::now();

    //WHEN THE NEXT SLOT OPENS, FIRST ONE NOW
    let mut due = Instant::now();

    //THE PREVIOUS FRAME'S BYTES
    let mut last_raw: Option<Vec<u8>> = None;

    //SEND THE FRAME THE PROBE ALREADY PAID FOR
    let mut pending = session.first.take();

    let generation = options::monitor_generation();

    loop
    {
        if !running.load(Ordering::Relaxed) { return Ok(()); }

        //HAND THE RECORDER BACK ON A MONITOR SWITCH
        if switched(generation) { return Ok(()); }

        //EXIT ON DISABLED SCREEN
        if !options::get_use_screen()
        {
            running.store(false, Ordering::Relaxed);
            return Ok(());
        }

        //THE TIMEOUT IS ONLY THERE TO OBSERVE running
        let mut frame = match pending.take()
        {
            Some(frame) => frame,

            None => match latest.take(consts::RECORDER_POLL_INTERVAL)
            {
                Some(frame) => frame,
                None if latest.ended.load(Ordering::Relaxed) => return Err(t!("screen.error.recorder_stopped").to_owned()),
                None => continue,
            },
        };

        let force_encode = last_encode_time.elapsed() >= consts::FORCED_INTRA_INTERVAL;

        //EARLY FRAMES WAIT FOR THEIR SLOT
        if !force_encode && let Some(wait) = due.checked_duration_since(Instant::now())
        {
            thread::sleep(wait);

            //PREFER WHATEVER CAME IN MEANWHILE
            if let Some(newer) = latest.try_take()
            {
                latest.recycle(std::mem::replace(&mut frame, newer).data);
            }
        }

        let changed = last_raw.as_ref().is_none_or(|previous| previous != &frame.data);

        if !(force_encode || changed)
        {
            latest.recycle(frame.data);
            continue;
        }

        if let Some(compressed) = encoder.encode(frame.width, frame.height, &frame.data, frame.order)?
        {
            encoder.dispatch(&frame_tx, compressed);
        }

        //SLOTS FOLLOW SLOTS, NOT ENCODES
        let now = Instant::now();

        due = if now > due + min_interval { now } else { due + min_interval };
        last_encode_time = now;

        if let Some(previous) = last_raw.replace(frame.data) { latest.recycle(previous); }
    }
}

fn capture_loop_recorder //OS-NATIVE STREAMING CAPTURE, WITHOUT THE FALLBACK CHAIN
(
    frame_tx: Sender<Vec<u8>>,
    running: Arc<AtomicBool>,
    fps: u32,
) -> Result<(), String>
{
    run_recorder(start_recorder(fps)?, frame_tx, running, fps)
}
