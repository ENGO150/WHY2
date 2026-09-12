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
        RwLock,
        atomic::{ AtomicBool, Ordering },
        mpsc::Receiver,
    },
};

#[cfg(not(target_os = "macos"))]
use std::sync::mpsc;

use std::sync::mpsc::RecvTimeoutError;

use tokio::sync::mpsc::Sender;

use xcap::{ Frame, Monitor, VideoRecorder };

use openh264::
{
    OpenH264API,
    formats::{ RgbaSliceU8, YUVBuffer },
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

use crate::network::screen::
{
    consts,
    client::{ gpu::GpuConverter, options },
};

fn monitor_name(monitor: &Monitor) -> String
{
    monitor.name().unwrap_or_else(|_| "unknown".to_owned())
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
        .ok_or_else(|| format!("no monitor called '{selection}' - available: {}", monitor_list(&monitors)))
}

fn get_target_monitor() -> Result<Monitor, String> //THE MONITOR TO SHARE
{
    let monitors = Monitor::all().map_err(|e| format!("failed to enumerate monitors ({e})"))?;

    if monitors.is_empty() { return Err("no monitors found".to_owned()); }

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
    let monitors = Monitor::all().map_err(|e| format!("failed to enumerate monitors ({e})"))?;

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

    //ON WAYLAND A PICKED MONITOR PINS THE POLLING PATH
    #[cfg(target_os = "linux")]
    if wayland() && options::get_monitor().is_some()
    {
        return legacy_capture_loop(frame_tx, running, fps);
    }

    //SOME OBJECTIVE-C BULLSHIT ON MAC
    #[cfg(target_os = "macos")]
    return match open_recorder()
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
            let session = open_recorder();

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
        .usage_type(UsageType::ScreenContentRealTime)
        .skip_frames(true)
        .adaptive_quantization(false)
        .background_detection(false);

    Encoder::with_api_config(OpenH264API::from_source(), config)
        .map_err(|e| format!("failed to create H.264 encoder ({e})"))
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

    fn fill(&mut self, width: u32, height: u32, rgba: &[u8]) -> &YUVBuffer
    {
        //RESIZE ONLY ON A REAL RESOLUTION CHANGE
        if self.width != width || self.height != height
        {
            self.buffer = YUVBuffer::new(width as usize, height as usize);
            self.width = width;
            self.height = height;
        }

        self.buffer.read_rgb(RgbaSliceU8::new(rgba, (width as usize, height as usize)));

        &self.buffer
    }
}

enum Converter //RGBA -> I420, ON THE GPU WHERE THAT IS POSSIBLE
{
    Gpu(Box<GpuConverter>),
    Cpu(YuvScratch),
}

impl Converter
{
    fn select() -> Self
    {
        //AN EXPLICIT "cpu" PINS THE OLD PATH
        if env::var(consts::CONVERTER_OVERRIDE_VAR).unwrap_or_default().eq_ignore_ascii_case("cpu")
        {
            return Converter::Cpu(YuvScratch::new());
        }

        match GpuConverter::new()
        {
            Ok(converter) => Converter::Gpu(Box::new(converter)),

            //NO ADAPTER OR DRIVER - USE THE CPU PATH
            Err(_) => Converter::Cpu(YuvScratch::new()),
        }
    }
}

struct FrameEncoder
{
    encoder: Encoder,
    converter: Converter,
    fps: f32,
    dimensions: Option<(u32, u32)>,
}

impl FrameEncoder
{
    fn new(fps: f32) -> Result<Self, String>
    {
        Ok(Self { encoder: create_encoder(fps)?, converter: Converter::select(), fps, dimensions: None })
    }

    fn force_intra_frame(&mut self)
    {
        self.encoder.force_intra_frame();
    }

    fn encode(&mut self, width: u32, height: u32, rgba: &[u8]) -> Result<Option<Vec<u8>>, String>
    {
        //I420 CONVERSION PANICS ON ODD DIMENSIONS
        if width % 2 != 0 || height % 2 != 0
        {
            return Err(format!("unsupported capture resolution {width}x{height} (must be even)"));
        }

        //A RESIZE NEEDS A FRESH ENCODER
        if self.dimensions.is_some_and(|previous| previous != (width, height))
        {
            self.encoder = create_encoder(self.fps)?;
        }

        self.dimensions = Some((width, height));

        //A GPU THAT FAILS DROPS BACK TO THE CPU FOR GOOD
        if let Converter::Gpu(_) = &self.converter
            && !GpuConverter::supports(width, height)
        {
            self.converter = Converter::Cpu(YuvScratch::new());
        }

        let mut fallback = None;

        let bitstream = match &mut self.converter
        {
            Converter::Gpu(converter) => match converter.convert(width, height, rgba)
            {
                Ok(frame) =>
                {
                    let bitstream = self.encoder.encode(frame)
                        .map_err(|e| format!("H.264 encode failed ({e})"))?;

                    Some(bitstream.to_vec())
                },

                Err(reason) =>
                {
                    fallback = Some(reason);
                    None
                },
            },

            Converter::Cpu(scratch) =>
            {
                let yuv = scratch.fill(width, height, rgba);

                let bitstream = self.encoder.encode(yuv)
                    .map_err(|e| format!("H.264 encode failed ({e})"))?;

                Some(bitstream.to_vec())
            },
        };

        //SWITCH TO THE CPU AND REDO THE FRAME
        let data = match bitstream
        {
            Some(result) => result,

            None =>
            {
                debug_assert!(fallback.is_some(), "the GPU path only yields None after refusing a frame");

                self.converter = Converter::Cpu(YuvScratch::new());

                let Converter::Cpu(scratch) = &mut self.converter else { unreachable!() };

                let yuv = scratch.fill(width, height, rgba);

                let bitstream = self.encoder.encode(yuv)
                    .map_err(|e| format!("H.264 encode failed ({e})"))?;

                bitstream.to_vec()
            },
        };

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
                if let Some(compressed) = encoder.encode(image.width(), image.height(), image.as_raw())?
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

    if outputs.is_empty() { return Err("compositor reported no outputs".to_owned()); }

    //FIND THE PICKED OUTPUT, FAILING IF IT IS GONE
    let picked = options::get_monitor().is_some();

    match get_target_monitor().and_then(|m| m.name().map_err(|e| e.to_string()))
    {
        Ok(name) => match outputs.iter().find(|o| o.name == name)
        {
            Some(output) => return Ok(output.clone()),
            None if picked => return Err(format!("the compositor knows no output called '{name}'")),
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
        .map_err(|e| format!("wayland screen capture is unavailable ({e})"))?;

    let mut target_output = select_output(&wayshot)?;

    let mut encoder = FrameEncoder::new(fps as f32)?;

    //PROBE ONCE SO A BAD COMPOSITOR REPORTS AN ERROR
    let first_image = wayshot.screenshot_single_output(&target_output, true)
        .map_err(|e| format!("capturing {} failed ({e}) - your compositor must support \
            ext-image-copy-capture-v1 or wlr-screencopy-v1", target_output.name))?
        .into_rgba8();

    //ENCODE AND SEND FIRST FRAME
    if let Some(compressed) = encoder.encode(first_image.width(), first_image.height(), first_image.as_raw())?
    {
        encoder.dispatch(&frame_tx, compressed);
    }

    //PREVIOUS FRAME, KEPT BY MOVE (SEE capture_loop_xcap)
    let mut last_image = Some(first_image);

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

        match wayshot.screenshot_single_output(&target_output, true)
        {
            Ok(image) =>
            {
                failures = 0;

                let image = image.into_rgba8();

                stranded += image.as_raw().len() as u64;

                let force_encode = last_encode_time.elapsed() >= consts::FORCED_INTRA_INTERVAL;

                let changed = last_image.as_ref().is_none_or(|previous| previous.as_raw() != image.as_raw());

                if force_encode || changed
                {
                    if let Some(compressed) = encoder.encode(image.width(), image.height(), image.as_raw())?
                    {
                        encoder.dispatch(&frame_tx, compressed);
                    }

                    last_image = Some(image);
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
                        last_image = None;
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

//STRUCTS
struct RecorderSession //A STARTED OS-NATIVE RECORDER, ITS FRAME CHANNEL
{
    recorder: VideoRecorder,
    frames: Receiver<Frame>,
    first: Frame,
}

fn open_recorder() -> Result<RecorderSession, String> //THE BLOCKING HALF OF THE PROBE
{
    let monitor = get_target_monitor()?;

    let (recorder, frames) = monitor.video_recorder()
        .map_err(|e| format!("the OS screen recorder is unavailable ({e})"))?;

    recorder.start()
        .map_err(|e| format!("starting the OS screen recorder failed ({e})"))?;

    //DEMAND AN ACTUAL FRAME
    let first = frames.recv_timeout(consts::RECORDER_FIRST_FRAME)
        .map_err(|_| "the OS screen recorder started but delivered no frames".to_owned())?;

    Ok(RecorderSession { recorder, frames, first })
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
fn start_recorder() -> Result<RecorderSession, String>
{
    open_recorder()
}

#[cfg(not(target_os = "macos"))]
fn start_recorder() -> Result<RecorderSession, String> //PROBE THE OS-NATIVE RECORDER, BOUNDED
{
    //THE PROBE CAN BLOCK, SO IT GETS ITS OWN THREAD
    let (probe_tx, probe_rx) = mpsc::channel();

    thread::spawn(move ||
    {
        //DROP A LATE SESSION, RELEASING THE PORTAL
        probe_tx.send(open_recorder()).ok();
    });

    match probe_rx.recv_timeout(probe_timeout())
    {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => Err("the OS screen recorder did not answer in time".to_owned()),
        Err(RecvTimeoutError::Disconnected) => Err("the OS screen recorder probe died".to_owned()),
    }
}

fn run_recorder //EVENT-DRIVEN CAPTURE LOOP
(
    session: RecorderSession,
    frame_tx: Sender<Vec<u8>>,
    running: Arc<AtomicBool>,
    fps: u32,
) -> Result<(), String>
{
    let RecorderSession { recorder, frames, first } = session;

    let mut encoder = FrameEncoder::new(fps as f32)?;

    let min_interval = Duration::from_secs_f64(1.0 / fps as f64);

    let mut last_encode_time = Instant::now();

    //DO NOT HOLD THE FIRST FRAME FOR THE FPS BUDGET
    let mut last_dispatch = Instant::now() - min_interval;

    //THE PREVIOUS FRAME'S BYTES
    let mut last_raw: Option<Vec<u8>> = None;

    //SEND THE FRAME THE PROBE ALREADY PAID FOR
    let mut pending = Some(first);

    let generation = options::monitor_generation();

    let outcome = loop
    {
        if !running.load(Ordering::Relaxed) { break Ok(()); }

        //HAND THE RECORDER BACK ON A MONITOR SWITCH
        if switched(generation) { break Ok(()); }

        //EXIT ON DISABLED SCREEN
        if !options::get_use_screen()
        {
            running.store(false, Ordering::Relaxed);
            break Ok(());
        }

        //THE TIMEOUT IS ONLY THERE TO OBSERVE running
        let mut frame = match pending.take()
        {
            Some(frame) => frame,

            None => match frames.recv_timeout(consts::RECORDER_POLL_INTERVAL)
            {
                Ok(frame) => frame,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break Err("the OS screen recorder stopped delivering frames".to_owned()),
            },
        };

        //KEEP THE NEWEST FRAME AND DROP THE REST
        while let Ok(newer) = frames.try_recv()
        {
            frame = newer;
        }

        let force_encode = last_encode_time.elapsed() >= consts::FORCED_INTRA_INTERVAL;

        //FPS BUDGET
        if !force_encode && last_dispatch.elapsed() < min_interval
        {
            continue;
        }

        let changed = last_raw.as_ref().is_none_or(|previous| previous != &frame.raw);

        if !(force_encode || changed)
        {
            continue;
        }

        if let Some(compressed) = encoder.encode(frame.width, frame.height, &frame.raw)?
        {
            encoder.dispatch(&frame_tx, compressed);
        }

        last_dispatch = Instant::now();
        last_encode_time = last_dispatch;
        last_raw = Some(frame.raw);
    };

    recorder.stop().ok();

    outcome
}

fn capture_loop_recorder //OS-NATIVE STREAMING CAPTURE, WITHOUT THE FALLBACK CHAIN
(
    frame_tx: Sender<Vec<u8>>,
    running: Arc<AtomicBool>,
    fps: u32,
) -> Result<(), String>
{
    run_recorder(start_recorder()?, frame_tx, running, fps)
}
