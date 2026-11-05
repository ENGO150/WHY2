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

//! # Screen Capture
//!
//! The capture pipeline: picking a backend, grabbing frames, converting them to I420 and
//! encoding them. It is the one blocking CPU loop in the client and runs in `spawn_blocking`.
//!
//! # Backends
//! The backend is chosen at run time, never assumed:
//!
//! - **Recorder** — the OS-native streaming recorder: our own xdg-desktop-portal + PipeWire
//!   recorder on Wayland ([`PortalRecorder`]), and xcap's on X11 (a polling thread), Windows (DXGI
//!   Desktop Duplication) and macOS (`AVCaptureScreenInput`).
//! - **Polling** — the legacy path: libwayshot screencopy on Wayland,
//!   xcap screenshots elsewhere.
//!
//! # The Probe Never Gates the Share
//! The polling path starts immediately and the recorder is probed beside it on its own thread;
//! the recorder takes over only once it has proven itself (`UPGRADING`), at the cost of one
//! keyframe. Waiting for the probe first was wrong: on Wayland it is a portal request that blocks
//! until somebody answers the picker. A recorder must deliver an actual frame within
//! [`RECORDER_FIRST_FRAME`](consts::RECORDER_FIRST_FRAME) to count — except the portal, which is
//! accepted once its stream negotiates, since damage-driven compositors send nothing for a still
//! desktop. Only when the polling path cannot start at all does the probe block, for up to
//! [`RECORDER_PROBE_TIMEOUT`](consts::RECORDER_PROBE_TIMEOUT).
//!
//! `WHY2_CAPTURE_BACKEND` (`recorder` / `legacy`) pins a backend and
//! `WHY2_CAPTURE_PROBE_TIMEOUT` overrides the probe deadline.
//!
//! # Wayland Polling
//! libwayshot leaks a compositor-side buffer per capture until the client disconnects, so the
//! wayshot loop counts the bytes it has stranded and reconnects past
//! [`WAYLAND_LEAK_BUDGET`](consts::WAYLAND_LEAK_BUDGET). It also reads the compositor's buffer
//! itself through one reused memfd instead of letting libwayshot build an image
//! pixel by pixel.
//!
//! # Monitor Switching
//! [`capture_loop`] restarts the backend whenever the monitor pick changes mid-share, keeping the
//! socket, the token and the audio; the viewer pays one keyframe.
//!
//! # Encoding
//! The frame encoder converts each frame with openh264's SIMD RGBA/BGRA → I420 path and hands it to
//! a GPU encoder where one opens, falling back to software for the rest of the capture the moment
//! one fails. A frame is not encoded at all while the network channel is full, so a skipped frame
//! never breaks the reference chain, and a keyframe is forced at least every
//! [`KEYFRAME_INTERVAL`](consts::KEYFRAME_INTERVAL).

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

use openh264::formats::
{
    BgraSliceU8,
    RgbaSliceU8,
    YUVBuffer,
    YUVSource,
};

use crate::
{
    t,
    network::screen::
    {
        consts,
        client::
        {
            options,
            rate::{ self, RateControl },
            encoder::
            {
                self,
                Backend,
                Planes,
                Settings,
            },
        },
    },
};

#[cfg(target_os = "linux")]
use crate::network::screen::client::portal::PortalRecorder;

/// Returns a monitor's name, or a placeholder.
fn monitor_name(monitor: &Monitor) -> String
{
    monitor.name().unwrap_or_else(|_| t!("screen.unknown_monitor").to_owned())
}

/// Lists the monitors as the user may name them: `1 (DP-1), 2 (HDMI-A-1)`.
fn monitor_list(monitors: &[Monitor]) -> String //THE MONITORS AS THE USER MAY NAME THEM
{
    monitors.iter().enumerate()
        .map(|(index, monitor)| format!("{} ({})", index + 1, monitor_name(monitor)))
        .collect::<Vec<String>>()
        .join(", ")
}

//THE MONITOR ASKED FOR, BY 1-BASED INDEX OR NAME
/// Selects a monitor by 1-based index or by name (case-insensitive).
///
/// # Returns
/// `Err` naming the available monitors if none matches.
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

/// Returns the monitor to share: the picked one, or the primary.
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
/// Resolves a monitor selection (index or name) to the monitor's name.
///
/// # Returns
/// `Err` with a message for the user if no monitor matches.
pub fn resolve_monitor(selection: &str) -> Result<String, String>
{
    let monitors = Monitor::all().map_err(|error| t!("screen.error.enumerate", error))?;

    select_monitor(monitors, selection).map(|monitor| monitor_name(&monitor))
}

/// Returns the name of the monitor a share would capture right now.
pub fn current_monitor() -> Option<String> //WHAT A SHARE WOULD CAPTURE RIGHT NOW, BY NAME
{
    get_target_monitor().ok().map(|monitor| monitor_name(&monitor))
}

//THE NAMES THE PALETTE OFFERS, CACHED
/// Returns every monitor's name, cached for [`MONITOR_LIST_TTL`](consts::MONITOR_LIST_TTL).
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
/// Set once the OS-native recorder has proven itself; tells the polling loop to stand down.
static UPGRADING: AtomicBool = AtomicBool::new(false);

/// Returns whether the polling loop should stand down for the recorder.
fn upgrading() -> bool
{
    UPGRADING.load(Ordering::Relaxed)
}

/// Returns whether the session is Wayland.
#[cfg(target_os = "linux")]
fn wayland() -> bool
{
    env::var("WAYLAND_DISPLAY").is_ok() || env::var("XDG_SESSION_TYPE").unwrap_or_default() == "wayland"
}

/// Runs the polling path: libwayshot on Wayland, xcap elsewhere.
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

/// Captures and encodes until the share ends, restarting the backend on a monitor switch.
///
/// # Parameters
/// - `frame_tx`: Where encoded access units go.
/// - `running`: Cleared when the share ends.
/// - `fps`: The target frame rate.
///
/// # Returns
/// `Err` with a message for the user if capture failed.
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

/// Returns whether the monitor was picked again since `generation`.
fn switched(generation: usize) -> bool //THE MONITOR WAS PICKED AGAIN WHILE WE WERE CAPTURING
{
    options::monitor_generation() != generation
}

/// Picks a backend (see the module documentation) and captures on it until it stops.
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

/// A reusable I420 buffer.
struct YuvScratch //REUSABLE I420 SCRATCH BUFFER
{
    /// The planes.
    buffer: YUVBuffer,
    /// Width the buffer is sized for.
    width: u32,
    /// Height the buffer is sized for.
    height: u32,
}

impl YuvScratch
{
    /// Creates an empty buffer.
    fn new() -> Self
    {
        Self { buffer: YUVBuffer::new(0, 0), width: 0, height: 0 }
    }

    /// Converts one RGBA or BGRA picture to I420, reallocating only on a resolution change.
    fn fill(&mut self, width: u32, height: u32, pixels: &[u8], order: PixelOrder) -> Planes<'_>
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

        Planes { width, height, y: self.buffer.y(), u: self.buffer.u(), v: self.buffer.v() }
    }
}

/// Converts, encodes and dispatches frames, choosing and replacing the encoder backend.
struct FrameEncoder
{
    /// The open encoder, if any.
    encoder: Option<Box<dyn Backend>>,
    /// The I420 conversion buffer.
    scratch: YuvScratch,
    /// The target frame rate.
    fps: u32,
    /// The dimensions the encoder was opened for.
    dimensions: Option<(u32, u32)>,
    /// Whether a GPU encoder is still worth trying.
    hardware: bool, //GPU ENCODER STILL WORTH TRYING
    /// Whether the next frame must be a keyframe.
    keyframe: bool, //THE NEXT FRAME MUST STAND ALONE
    /// The bitrate controller.
    rate: RateControl,
    /// When the last keyframe was produced.
    last_keyframe: Instant,
}

impl FrameEncoder
{
    /// Creates an encoder for a frame rate; the backend opens on the first frame.
    fn new(fps: u32) -> Self
    {
        Self
        {
            encoder: None,
            scratch: YuvScratch::new(),
            fps,
            dimensions: None,
            hardware: true,
            keyframe: false,
            rate: RateControl::new(),
            last_keyframe: Instant::now(),
        }
    }

    //ENCODE AND SEND, FALSE IF NOTHING WENT
    /// Encodes one frame and hands it to the network task.
    ///
    /// Applies a new bitrate target first, skips the frame outright while the network channel is
    /// full, and forces a keyframe when [`KEYFRAME_INTERVAL`](consts::KEYFRAME_INTERVAL) has passed.
    ///
    /// # Returns
    /// - `Ok(true)` if a frame was sent.
    /// - `Ok(false)` if nothing went.
    /// - `Err` with a message for the user if encoding failed.
    fn submit(&mut self, frame_tx: &Sender<Vec<u8>>, width: u32, height: u32, pixels: &[u8], order: PixelOrder) -> Result<bool, String>
    {
        if let Some(bitrate) = self.rate.update() && let Some(backend) = self.encoder.as_mut()
        {
            backend.set_bitrate(bitrate);
        }

        //NETWORK BEHIND - SKIP BEFORE THE ENCODER SEES IT
        if frame_tx.capacity() == 0
        {
            return Ok(false);
        }

        if self.last_keyframe.elapsed() >= consts::KEYFRAME_INTERVAL { self.force_intra_frame(); }

        let Some(frame) = self.encode(width, height, pixels, order)? else { return Ok(false) };

        if encoder::nal_units(&frame).any(|unit| unit.first().is_some_and(|header| header & 0x1f == 5))
        {
            self.last_keyframe = Instant::now();
        }

        Ok(self.dispatch(frame_tx, frame))
    }

    /// Makes the next frame a keyframe.
    fn force_intra_frame(&mut self)
    {
        self.keyframe = true;
    }

    /// Converts and encodes one frame, reopening the encoder on a resize and falling back from the
    /// GPU to software on a failure.
    ///
    /// # Returns
    /// The access unit, `None` for an empty one, or `Err` on odd dimensions or a software failure.
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
        if self.dimensions != Some((width, height))
        {
            self.encoder = None;
            self.dimensions = Some((width, height));
        }

        let planes = self.scratch.fill(width, height, pixels, order);

        loop
        {
            if self.encoder.is_none()
            {
                self.encoder = Some(encoder::open(Settings::new(width, height, self.fps, self.rate.bitrate()), &mut self.hardware)
                    .map_err(|error| t!("screen.error.encoder", error))?);
            }

            let Some(backend) = self.encoder.as_mut() else { continue };

            let keyframe = std::mem::take(&mut self.keyframe);

            match backend.encode(&planes, keyframe)
            {
                //SKIP EMPTY FRAMES
                Ok(data) => return Ok((!data.is_empty()).then_some(data)),

                //FALL BACK TO openh264
                Err(_) if backend.hardware() =>
                {
                    self.encoder = None;
                    self.hardware = false;
                    self.keyframe = true;
                },

                Err(error) => return Err(t!("screen.error.encode", error)),
            }
        }
    }

    /// Hands an access unit to the network task; a full channel drops it and marks the chain broken.
    fn dispatch(&mut self, frame_tx: &Sender<Vec<u8>>, frame: Vec<u8>) -> bool //HAND A FRAME TO THE NETWORK TASK
    {
        //THIS FRAME IS GONE, SO THE NEXT CANNOT PREDICT IT
        if frame_tx.try_send(frame).is_err()
        {
            rate::request_keyframe();

            return false;
        }

        true
    }
}

/// Returns one capture slot at the current frame rate.
fn frame_interval() -> Duration
{
    Duration::from_secs_f64(1.0 / f64::from(rate::fps()))
}

/// Sleeps until the next frame slot.
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

/// Polls xcap screenshots of one monitor at the target rate.
fn capture_loop_xcap
(
    monitor: Monitor,
    frame_tx: Sender<Vec<u8>>,
    running: Arc<AtomicBool>,
    fps: u32,
) -> Result<(), String>
{
    let mut next_tick = Instant::now() + frame_interval();

    let generation = options::monitor_generation();

    let mut encoder = FrameEncoder::new(fps);

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

        //LET THE LINK DRAIN BEFORE ADDING MORE
        if rate::draining()
        {
            sleep_until_next_tick(&mut next_tick, frame_interval());

            continue;
        }

        let forced = rate::take_keyframe_request();
        if forced { encoder.force_intra_frame(); }

        if let Ok(image) = monitor.capture_image()
        {
            let force_encode = forced || last_encode_time.elapsed() >= consts::FORCED_FRAME_INTERVAL;

            //CHEAP WHEN THE SCREEN MOVED - memcmp EARLY-EXITS
            let changed = last_image.as_ref().is_none_or(|previous| previous.as_raw() != image.as_raw());

            if force_encode || changed
            {
                if encoder.submit(&frame_tx, image.width(), image.height(), image.as_raw(), PixelOrder::Rgba)?
                {
                    last_image = Some(image);
                    last_encode_time = Instant::now();
                } else if forced { rate::request_keyframe(); }
            }
        } else if forced { rate::request_keyframe(); }

        sleep_until_next_tick(&mut next_tick, frame_interval());
    }

    Ok(())
}

/// Picks the Wayland output to share: the picked monitor, else the primary, else the one at the
/// layout origin.
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
/// Opens a fresh Wayland connection onto the output named `name`.
#[cfg(target_os = "linux")]
fn reconnect_wayshot(name: &str) -> Option<(libwayshot::WayshotConnection, libwayshot::output::OutputInfo)>
{
    let connection = libwayshot::WayshotConnection::new().ok()?;
    let output = connection.get_all_outputs().iter().find(|output| output.name == name).cloned()?;

    Some((connection, output))
}

//WL_SHM FORMAT CODES
/// `wl_shm` format code of ARGB8888.
#[cfg(target_os = "linux")]
const SHM_ARGB8888: u32 = 0;
/// `wl_shm` format code of XRGB8888.
#[cfg(target_os = "linux")]
const SHM_XRGB8888: u32 = 1;
/// `wl_shm` format code of ABGR8888.
#[cfg(target_os = "linux")]
const SHM_ABGR8888: u32 = 0x34324241;
/// `wl_shm` format code of XBGR8888.
#[cfg(target_os = "linux")]
const SHM_XBGR8888: u32 = 0x34324258;

/// One reused memfd the compositor copies frames into.
#[cfg(target_os = "linux")]
struct ShmCapture //ONE REUSED BUFFER THE COMPOSITOR COPIES INTO
{
    /// The memfd.
    file: File,
    /// Its size.
    len: u64,
    /// Its mapping, created on first use.
    map: Option<Mmap>,
}

#[cfg(target_os = "linux")]
impl ShmCapture
{
    /// Creates a memfd of `bytes`; pages are only allocated when written.
    fn new(bytes: u64) -> Option<Self>
    {
        let fd = rustix::fs::memfd_create("why2-capture", rustix::fs::MemfdFlags::CLOEXEC).ok()?;
        let file = File::from(fd);

        file.set_len(bytes).ok()?;

        Some(Self { file, len: bytes, map: None })
    }

    //ONE FRAME INTO pixels, OR None WHERE ONLY libwayshot CAN READ IT
    /// Captures one frame into `pixels`, tightly packed.
    ///
    /// # Returns
    /// - `Ok(Some((width, height, order)))` on success.
    /// - `Ok(None)` for a frame only libwayshot can read (a rotated output, an unknown format, a
    ///   frame larger than the buffer).
    /// - `Err` if the compositor refused the capture.
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
/// Copies `width`×`height` RGBA pixels out of rows `stride` bytes apart into `pixels`.
///
/// # Returns
/// `false` if the dimensions do not fit the source.
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

/// Captures the Wayland output with libwayshot screencopy, recycling the connection on a memory
/// budget.
#[cfg(target_os = "linux")]
fn capture_loop_wayshot
(
    frame_tx: Sender<Vec<u8>>,
    running: Arc<AtomicBool>,
    fps: u32,
) -> Result<(), String>
{
    let generation = options::monitor_generation();

    let mut wayshot = libwayshot::WayshotConnection::new()
        .map_err(|error| t!("screen.error.wayland", error))?;

    let mut target_output = select_output(&wayshot)?;

    let mut encoder = FrameEncoder::new(fps);

    //PROBE ONCE SO A BAD COMPOSITOR REPORTS AN ERROR
    let first_image = wayshot.screenshot_single_output(&target_output, true)
        .map_err(|error| t!("screen.error.wayland_capture", output = target_output.name, error))?
        .into_rgba8();

    //ENCODE AND SEND FIRST FRAME
    encoder.submit(&frame_tx, first_image.width(), first_image.height(), first_image.as_raw(), PixelOrder::Rgba)?;

    //ROOM FOR THE PICTURE PLUS ANY ROW PADDING
    let reserve = first_image.height() as u64 * (first_image.width() as u64 * 4 + consts::SHM_ROW_SLACK);
    let mut shm = ShmCapture::new(reserve);

    //PREVIOUS FRAME, AND THE BUFFER THE NEXT ONE GOES INTO
    let mut last_raw = Some(first_image.into_raw());
    let mut pixels = Vec::new();

    let mut last_encode_time = Instant::now();

    let mut failures = 0u32;
    let mut next_tick = Instant::now() + frame_interval();

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

        //LET THE LINK DRAIN BEFORE ADDING MORE
        if rate::draining()
        {
            sleep_until_next_tick(&mut next_tick, frame_interval());

            continue;
        }

        let forced = rate::take_keyframe_request();
        if forced { encoder.force_intra_frame(); }

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

                let force_encode = forced || last_encode_time.elapsed() >= consts::FORCED_FRAME_INTERVAL;

                let changed = last_raw.as_ref().is_none_or(|previous| previous != &pixels);

                if force_encode || changed
                {
                    if encoder.submit(&frame_tx, width, height, &pixels, order)?
                    {
                        //SWAP RATHER THAN REALLOCATE
                        pixels = last_raw.replace(std::mem::take(&mut pixels)).unwrap_or_default();
                        last_encode_time = Instant::now();
                    } else if forced { rate::request_keyframe(); }
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

                if forced { rate::request_keyframe(); }

                if failures >= consts::WAYLAND_RECONNECT_FAILURES
                {
                    if let Some((connection, output)) = reconnect_wayshot(&target_output.name)
                    {
                        wayshot = connection;
                        target_output = output;

                        //FORCE A KEYFRAME - FRAMES WERE MISSED
                        rate::request_keyframe();
                        last_raw = None;
                    }

                    stranded = 0;
                    failures = 0;
                }
            },
        }

        sleep_until_next_tick(&mut next_tick, frame_interval());
    }

    Ok(())
}

//ENUMS
/// The byte order of a captured pixel.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PixelOrder //BYTE ORDER OF A CAPTURED PIXEL
{
    /// Red, green, blue, alpha.
    Rgba,
    /// Blue, green, red, alpha.
    Bgra,
}

/// The recorder delivering the frames.
enum Recorder //WHO IS DELIVERING THE FRAMES
{
    /// xcap's recorder.
    Xcap(VideoRecorder),

    /// Our xdg-desktop-portal recorder.
    #[cfg(target_os = "linux")]
    Portal(PortalRecorder),
}

//STRUCTS
/// One captured picture, tightly packed, four bytes per pixel.
pub struct CapturedFrame //ONE PICTURE, TIGHTLY PACKED
{
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Byte order of each pixel.
    pub order: PixelOrder,
    /// The pixels.
    pub data: Vec<u8>,
}

/// A one-slot handoff from a recorder to the encode loop.
///
/// Only the newest frame is kept, so the recorder never waits on the encoder and capture overlaps
/// encoding. Pixel buffers are recycled, at most [`SPARE_FRAMES`](consts::SPARE_FRAMES) of them.
pub struct LatestFrame //ONE-SLOT FRAME HANDOFF
{
    /// The waiting frame.
    slot: Mutex<Option<CapturedFrame>>,
    /// Buffers kept for reuse.
    spare: Mutex<Vec<Vec<u8>>>, //BUFFERS TO REUSE
    /// Signals a frame was put.
    ready: Condvar,
    /// Whether the encode loop still wants frames.
    draining: AtomicBool, //THE LOOP STILL WANTS FRAMES
    /// Whether the recorder stopped delivering.
    ended: AtomicBool,    //THE RECORDER STOPPED DELIVERING
}

impl LatestFrame
{
    /// Creates an empty handoff.
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

    /// Waits up to `timeout` for a frame and takes it.
    fn take(&self, timeout: Duration) -> Option<CapturedFrame> //THE NEWEST FRAME, OR NOTHING
    {
        let (mut slot, _) = self.ready
            .wait_timeout_while(self.slot.lock().unwrap(), timeout, |slot| slot.is_none())
            .unwrap();

        slot.take()
    }

    /// Takes the waiting frame, if any.
    fn try_take(&self) -> Option<CapturedFrame>
    {
        self.slot.lock().unwrap().take()
    }

    /// Puts a frame, replacing (and recycling) one still waiting.
    pub fn put(&self, frame: CapturedFrame) //REPLACE THE WAITING FRAME
    {
        let stale = self.slot.lock().unwrap().replace(frame);

        self.ready.notify_one();

        if let Some(stale) = stale { self.recycle(stale.data); }
    }

    /// Returns a recycled buffer, or a new one.
    pub fn buffer(&self) -> Vec<u8> //A USED BUFFER, OR A NEW ONE
    {
        self.spare.lock().unwrap().pop().unwrap_or_default()
    }

    /// Returns a buffer for reuse.
    pub fn recycle(&self, data: Vec<u8>)
    {
        let mut spare = self.spare.lock().unwrap();

        if spare.len() < consts::SPARE_FRAMES { spare.push(data); }
    }

    /// Marks the recorder as stopped and wakes the encode loop.
    pub fn end(&self)
    {
        self.ended.store(true, Ordering::Relaxed);
        self.ready.notify_one();
    }
}

//DRAIN THE RECORDER ALONGSIDE THE ENCODE
/// Moves xcap's frames into the handoff on a thread of their own, keeping only the newest.
///
/// xcap delivers over a rendezvous channel, so without this its capture thread would block for
/// the whole of every encode.
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

/// A started recorder and its frames. Dropping it stops the recorder.
struct RecorderSession //A STARTED OS-NATIVE RECORDER AND ITS FRAMES
{
    /// The recorder.
    recorder: Recorder,
    /// Its frame handoff.
    latest: Arc<LatestFrame>,
    /// The frame that proved it works, if any.
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
/// Starts the portal recorder at the share's frame rate.
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

/// Starts the platform's recorder. Blocking.
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

/// Waits for a recorder's first frame.
///
/// # Returns
/// `Err` if none arrives within [`RECORDER_FIRST_FRAME`](consts::RECORDER_FIRST_FRAME).
fn first_frame(recorder: Recorder, latest: Arc<LatestFrame>) -> Result<RecorderSession, String>
{
    let mut session = RecorderSession { recorder, latest, first: None };

    //DEMAND AN ACTUAL FRAME
    session.first = Some(session.latest.take(consts::RECORDER_FIRST_FRAME)
        .ok_or_else(|| t!("screen.error.recorder_silent").to_owned())?);

    Ok(session)
}

/// Returns the probe deadline, from `WHY2_CAPTURE_PROBE_TIMEOUT` or the default.
#[cfg(not(target_os = "macos"))]
fn probe_timeout() -> Duration
{
    env::var(consts::PROBE_TIMEOUT_VAR).ok()
        .and_then(|value| value.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(consts::RECORDER_PROBE_TIMEOUT)
}

//A macOS SESSION CANNOT CROSS A THREAD
/// Starts the recorder on this thread; a macOS session cannot cross threads.
#[cfg(target_os = "macos")]
fn start_recorder(fps: u32) -> Result<RecorderSession, String>
{
    open_recorder(fps)
}

/// Starts the recorder on its own thread, bounded by the probe deadline.
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

/// Encodes frames from a recorder until the share ends, the recorder stops, or the monitor
/// changes.
///
/// A frame that arrives before its slot waits for it rather than being dropped, and slots are
/// scheduled off each other rather than off the end of the encode, so the encode time is not
/// added to every period. A slot more than an interval late restarts the schedule.
fn run_recorder //EVENT-DRIVEN CAPTURE LOOP
(
    mut session: RecorderSession,
    frame_tx: Sender<Vec<u8>>,
    running: Arc<AtomicBool>,
    fps: u32,
) -> Result<(), String>
{
    let latest = session.latest.clone();

    let mut encoder = FrameEncoder::new(fps);

    let mut min_interval = frame_interval();

    let mut last_encode_time = Instant::now();

    //WHEN THE NEXT SLOT OPENS, FIRST ONE NOW
    let mut due = Instant::now();

    //THE FRAME LAST SENT, KEPT FOR A FORCED REPEAT
    let mut last: Option<CapturedFrame> = None;

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

        //LET THE LINK DRAIN BEFORE ADDING MORE
        if rate::draining()
        {
            thread::sleep(consts::RECORDER_POLL_INTERVAL);

            continue;
        }

        let forced = rate::take_keyframe_request();
        if forced { encoder.force_intra_frame(); }

        //THE TIMEOUT IS ONLY THERE TO OBSERVE running
        let mut frame = match pending.take()
        {
            Some(frame) => frame,

            None => match latest.take(consts::RECORDER_POLL_INTERVAL)
            {
                Some(frame) => frame,
                None if latest.ended.load(Ordering::Relaxed) => return Err(t!("screen.error.recorder_stopped").to_owned()),

                None =>
                {
                    //A STILL DESKTOP STILL ANSWERS A REQUEST
                    match (forced, last.as_ref())
                    {
                        (true, Some(previous)) =>
                        {
                            if encoder.submit(&frame_tx, previous.width, previous.height, &previous.data, previous.order)?
                            {
                                last_encode_time = Instant::now();
                                due = Instant::now() + min_interval;
                            } else { rate::request_keyframe(); }
                        },

                        //NOTHING TO REPEAT YET
                        (true, None) => rate::request_keyframe(),

                        _ => {}
                    }

                    continue;
                }
            },
        };

        let force_encode = forced || last_encode_time.elapsed() >= consts::FORCED_FRAME_INTERVAL;

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

        let changed = last.as_ref().is_none_or(|previous| previous.data != frame.data);

        if !(force_encode || changed)
        {
            latest.recycle(frame.data);
            continue;
        }

        //NOT SENT - THE NEXT FRAME TRIES AGAIN
        if !encoder.submit(&frame_tx, frame.width, frame.height, &frame.data, frame.order)?
        {
            latest.recycle(frame.data);

            if forced { rate::request_keyframe(); }

            continue;
        }

        //SLOTS FOLLOW SLOTS, NOT ENCODES
        let now = Instant::now();

        due = if now > due + min_interval { now } else { due + min_interval };
        min_interval = frame_interval();
        last_encode_time = now;

        if let Some(previous) = last.replace(frame) { latest.recycle(previous.data); }
    }
}

/// Captures on the recorder alone, with no fallback.
fn capture_loop_recorder //OS-NATIVE STREAMING CAPTURE, WITHOUT THE FALLBACK CHAIN
(
    frame_tx: Sender<Vec<u8>>,
    running: Arc<AtomicBool>,
    fps: u32,
) -> Result<(), String>
{
    run_recorder(start_recorder(fps)?, frame_tx, running, fps)
}
