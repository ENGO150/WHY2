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

//! # H.264 Encoders
//!
//! Every encoder the share can use, behind one [`Backend`] trait: the platform's GPU encoder —
//! Vulkan Video on Linux, a hardware Media Foundation transform on Windows, VideoToolbox on
//! macOS — and openh264 on the CPU as the fallback ([`open`]).
//!
//! # The Wire Does Not Change
//! Viewers decode with openh264, whose decoder only handles **Constrained Baseline**, so every
//! backend asks for it: no CABAC, no B-frames, no 8×8 transform. A GPU-encoded share is therefore
//! decodable by every client already in the field. Every IDR also carries its SPS and PPS in front
//! of it, which the server's keyframe detection and attach cache rely on
//! ([`ParameterSets::complete`] adds them where an API leaves them out).
//!
//! # Input
//! Every backend takes the same tightly packed I420 [`Planes`]; GPU backends interleave them into
//! NV12 as they copy them into their own buffers ([`write_nv12`]).
//!
//! # Rate
//! openh264 holds its bitrate by skipping frames; hardware rate controls instead keep every frame
//! and overshoot on content the bitrate cannot carry. GPU backends therefore skip frames through
//! a one-second token bucket ([`Budget`]), never skipping a keyframe, so
//! [`H264_BITRATE`](crate::network::screen::consts::H264_BITRATE) stays a ceiling. The bucket may
//! owe up to `BUDGET_DEBT` seconds, so one large keyframe does not freeze the frames after it.
//! Every backend takes a new target mid-stream ([`Backend::set_bitrate`]).
//!
//! # The Software Encoder
//! openh264 runs in camera mode rather than screen-content mode: on real game footage screen mode
//! dropped a third of all frames to hold its budget, while camera mode kept all of them at higher
//! quality, and it is no worse on a desktop. Its timestamps are real time, since its rate control
//! is defined in time, and a new bitrate moves the layer's ceiling along with the target — the
//! ceiling it opened with otherwise makes it skip frames in runs.

//MODULES
#[cfg(target_os = "linux")]
mod vulkan;

#[cfg(target_os = "windows")]
mod media_foundation;

#[cfg(target_os = "macos")]
mod video_toolbox;

use std::
{
    os::raw::c_int,
    time::Instant,
};

use openh264::
{
    OpenH264API,
    Timestamp,
    formats::YUVSlices,
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

use crate::network::screen::consts;

//openh264 SetOption VALUES
/// openh264 option: the target bitrate.
const ENCODER_OPTION_BITRATE: c_int = 5;
/// openh264 option: the maximum bitrate.
const ENCODER_OPTION_MAX_BITRATE: c_int = 6;
/// openh264 spatial layer 0.
const SPATIAL_LAYER_0: c_int = 0;
/// openh264's "every spatial layer".
const SPATIAL_LAYER_ALL: c_int = 4;
/// How many seconds of bitrate a hardware encoder may owe before it skips frames.
const BUDGET_DEBT: f64 = 2.0;

//STRUCTS
/// One tightly packed I420 picture.
pub struct Planes<'a> //ONE TIGHTLY PACKED I420 PICTURE
{
    /// Width in pixels (even).
    pub width: u32,
    /// Height in pixels (even).
    pub height: u32,
    /// The Y plane, `width` bytes per row.
    pub y: &'a [u8],
    /// The U plane, `width / 2` bytes per row.
    pub u: &'a [u8],
    /// The V plane, `width / 2` bytes per row.
    pub v: &'a [u8],
}

/// What every backend is opened with.
#[derive(Clone, Copy)]
pub struct Settings //WHAT EVERY BACKEND IS ASKED FOR
{
    /// Picture width.
    pub width: u32,
    /// Picture height.
    pub height: u32,
    /// Frame rate.
    pub fps: u32,
    /// Initial bitrate.
    pub bitrate: u32,
    /// Frames between scheduled keyframes.
    pub keyframe_interval: u32,
}

impl Settings
{
    /// Creates settings with a scheduled keyframe every
    /// [`KEYFRAME_INTERVAL`](crate::network::screen::consts::KEYFRAME_INTERVAL).
    pub fn new(width: u32, height: u32, fps: u32, bitrate: u32) -> Self
    {
        let seconds = consts::KEYFRAME_INTERVAL.as_secs_f64();
        let keyframe_interval = (seconds * f64::from(fps)).round().max(1.0) as u32;

        Self { width, height, fps, bitrate, keyframe_interval }
    }
}

/// A one-second token bucket that skips frames over the bitrate.
///
/// With balance $\beta$ bits at target $R$ bits per second, each frame refills
/// $\beta \leftarrow \min(\beta + R\Delta t,\ R)$ and may be encoded while
/// $\beta \ge -R\,D$ or it is a keyframe, where $D$ is `BUDGET_DEBT` seconds; an encoded frame
/// of $n$ bytes then pays $\beta \leftarrow \beta - 8n$. The debt lets a keyframe's cost be repaid
/// over the following frames instead of skipping all of them at once.
pub struct Budget //SKIPS FRAMES OVER THE BITRATE
{
    /// Bits per second.
    rate: f64,
    /// Bits available; negative while overdrawn.
    balance: f64,
    /// When the bucket was last refilled.
    refilled: Instant,
}

impl Budget
{
    /// Creates a full bucket for a bitrate.
    pub fn new(bitrate: u32) -> Self
    {
        Self { rate: f64::from(bitrate), balance: f64::from(bitrate), refilled: Instant::now() }
    }

    /// Refills the bucket and returns whether a frame may be encoded; a keyframe always may.
    pub fn allows(&mut self, keyframe: bool) -> bool //MAY THIS FRAME BE ENCODED
    {
        let now = Instant::now();

        //REFILL, UP TO ONE SECOND
        self.balance = (self.balance + self.rate * (now - self.refilled).as_secs_f64()).min(self.rate);
        self.refilled = now;

        keyframe || self.balance >= -self.rate * BUDGET_DEBT
    }

    /// Pays for an encoded frame.
    pub fn spend(&mut self, bytes: usize)
    {
        self.balance -= bytes as f64 * 8.0;
    }

    /// Changes the bitrate, keeping the balance within one second of credit and
    /// `BUDGET_DEBT` seconds of debt.
    pub fn set_rate(&mut self, bitrate: u32)
    {
        self.rate = f64::from(bitrate);
        self.balance = self.balance.clamp(-self.rate * BUDGET_DEBT, self.rate);
    }
}

/// The last SPS and PPS seen, to put in front of an IDR that arrived without them.
#[derive(Default)]
pub struct ParameterSets //THE LAST SPS AND PPS SEEN
{
    /// The last SPS, with its start code.
    sps: Vec<u8>,
    /// The last PPS, with its start code.
    pps: Vec<u8>,
}

impl ParameterSets
{
    /// Records any parameter sets in an access unit, and prepends the last ones to a bare IDR.
    pub fn complete(&mut self, frame: Vec<u8>) -> Vec<u8> //SETS IN FRONT OF A BARE IDR
    {
        let mut idr = false;
        let mut sps = false;

        for unit in nal_units(&frame)
        {
            match unit.first().map(|header| header & 0x1f)
            {
                Some(5) => idr = true,

                Some(7) =>
                {
                    sps = true;
                    self.sps = annex_b(unit);
                },

                Some(8) => self.pps = annex_b(unit),
                _ => {},
            }
        }

        if !idr || sps || self.sps.is_empty() { return frame; }

        let mut complete = Vec::with_capacity(self.sps.len() + self.pps.len() + frame.len());
        complete.extend_from_slice(&self.sps);
        complete.extend_from_slice(&self.pps);
        complete.extend_from_slice(&frame);

        complete
    }
}

/// openh264, on the CPU.
struct Software //OPENH264, ON THE CPU
{
    /// The encoder.
    encoder: Encoder,
    /// Whether the encoder has been initialised by a first encode.
    started: bool,           //INITIALISED BY THE FIRST ENCODE
    /// A target set before then, applied after it.
    bitrate: Option<u32>,    //WAITING FOR THAT
    /// Where timestamps count from.
    epoch: Instant,          //WHERE TIMESTAMPS COUNT FROM
    /// The encoder's current target.
    applied: u32,            //THE ENCODER'S CURRENT TARGET
}

/// openh264's `SBitrateInfo`.
#[repr(C)]
struct BitrateInfo //openh264's SBitrateInfo
{
    /// The spatial layer.
    layer: c_int,
    /// The bitrate.
    bitrate: c_int,
}

impl Software
{
    /// Opens openh264 in real-time camera mode with frame skipping.
    fn new(settings: Settings) -> Result<Self, String>
    {
        let config = EncoderConfig::new()
            .max_frame_rate(FrameRate::from_hz(settings.fps as f32))
            .rate_control_mode(RateControlMode::Bitrate)
            .bitrate(BitRate::from_bps(settings.bitrate))
            .intra_frame_period(IntraFramePeriod::from_num_frames(settings.keyframe_interval))
            .complexity(Complexity::Low)
            .usage_type(UsageType::CameraVideoRealTime)
            .skip_frames(true)
            .adaptive_quantization(false)
            .background_detection(false);

        let encoder = Encoder::with_api_config(OpenH264API::from_source(), config).map_err(|error| error.to_string())?;

        Ok(Self { encoder, started: false, bitrate: None, epoch: Instant::now(), applied: settings.bitrate })
    }

    /// Sets the target and the ceiling, the ceiling on layer 0 and moved ahead of the target.
    fn apply_bitrate(&mut self, bitrate: u32)
    {
        //THE CEILING MOVES WITH THE TARGET, AHEAD OF IT
        let options = if bitrate > self.applied
        {
            [ENCODER_OPTION_MAX_BITRATE, ENCODER_OPTION_BITRATE]
        } else
        {
            [ENCODER_OPTION_BITRATE, ENCODER_OPTION_MAX_BITRATE]
        };

        for option in options
        {
            //CEILING ON THE LAYER ITSELF
            let layer = if option == ENCODER_OPTION_MAX_BITRATE { SPATIAL_LAYER_0 } else { SPATIAL_LAYER_ALL };
            let mut info = BitrateInfo { layer, bitrate: bitrate as c_int };

            //SAFETY: INITIALISED ENCODER, info OUTLIVES THE CALL
            unsafe { self.encoder.raw_api().set_option(option, (&raw mut info).cast()) };
        }

        self.applied = bitrate;
    }
}

impl Backend for Software
{
    fn encode(&mut self, planes: &Planes, keyframe: bool) -> Result<Vec<u8>, String>
    {
        if keyframe { self.encoder.force_intra_frame(); }

        //A TARGET SET BEFORE THE ENCODER EXISTED
        if self.started && let Some(bitrate) = self.bitrate.take() { self.apply_bitrate(bitrate); }

        let (width, height) = (planes.width as usize, planes.height as usize);
        let source = YUVSlices::new((planes.y, planes.u, planes.v), (width, height), (width, width / 2, width / 2));

        //REAL TIME FOR THE RATE CONTROL, NEVER ZERO
        let timestamp = Timestamp::from_millis(self.epoch.elapsed().as_millis() as u64 + 1);

        let frame = self.encoder.encode_at(&source, timestamp)
            .map(|bitstream| bitstream.to_vec())
            .map_err(|error| error.to_string());

        self.started = true;

        frame
    }

    fn set_bitrate(&mut self, bitrate: u32)
    {
        if self.started { self.apply_bitrate(bitrate) } else { self.bitrate = Some(bitrate) }
    }

    fn hardware(&self) -> bool
    {
        false
    }
}

//TRAITS
/// One H.264 encoder.
pub trait Backend //ONE H.264 ENCODER
{
    //ONE ACCESS UNIT, EMPTY IF SKIPPED
    /// Encodes one picture into one Annex B access unit.
    ///
    /// # Parameters
    /// - `planes`: The picture.
    /// - `keyframe`: Whether it must be an IDR.
    ///
    /// # Returns
    /// The access unit, empty if the frame was skipped, or `Err` with a description.
    fn encode(&mut self, planes: &Planes, keyframe: bool) -> Result<Vec<u8>, String>;

    //NEW TARGET, MID-STREAM
    /// Changes the target bitrate mid-stream.
    fn set_bitrate(&mut self, bitrate: u32);

    /// Returns whether this is a GPU encoder.
    fn hardware(&self) -> bool;
}

//PRIVATE
/// Opens the platform's GPU encoder.
#[allow(unreachable_code)]
fn open_hardware(settings: Settings) -> Result<Box<dyn Backend>, String> //THE PLATFORM'S GPU ENCODER
{
    #[cfg(target_os = "linux")]
    return vulkan::VulkanEncoder::new(settings).map(|encoder| Box::new(encoder) as Box<dyn Backend>);

    #[cfg(target_os = "windows")]
    return media_foundation::MediaFoundationEncoder::new(settings).map(|encoder| Box::new(encoder) as Box<dyn Backend>);

    #[cfg(target_os = "macos")]
    return video_toolbox::VideoToolboxEncoder::new(settings).map(|encoder| Box::new(encoder) as Box<dyn Backend>);

    let _ = settings;

    Err("no hardware encoder on this platform".to_owned())
}

/// Prefixes a NAL unit with a four-byte start code.
fn annex_b(unit: &[u8]) -> Vec<u8> //ONE NAL UNIT WITH ITS START CODE
{
    let mut framed = Vec::with_capacity(unit.len() + 4);
    framed.extend_from_slice(&[0, 0, 0, 1]);
    framed.extend_from_slice(unit);

    framed
}

//PUBLIC
/// Opens the best encoder available.
///
/// # Parameters
/// - `settings`: What to encode.
/// - `hardware`: Whether to try the GPU first; cleared if the GPU encoder will not open, so it is
///   not tried again for the rest of the capture.
///
/// # Returns
/// The encoder, or `Err` if even openh264 will not open.
pub fn open(settings: Settings, hardware: &mut bool) -> Result<Box<dyn Backend>, String> //THE BEST ENCODER THAT OPENS
{
    //GPU FIRST, openh264 IF IT WILL NOT OPEN
    if *hardware
    {
        match open_hardware(settings)
        {
            Ok(encoder) => return Ok(encoder),
            Err(_) => *hardware = false,
        }
    }

    Software::new(settings).map(|encoder| Box::new(encoder) as Box<dyn Backend>)
}

/// Iterates over the NAL units of an Annex B stream, start codes removed.
pub fn nal_units(data: &[u8]) -> impl Iterator<Item = &[u8]> //THE NAL UNITS OF AN ANNEX B STREAM
{
    let mut rest = data;

    std::iter::from_fn(move ||
    {
        let start = rest.windows(3).position(|window| window == [0, 0, 1])? + 3;
        rest = &rest[start..];

        let end = rest.windows(3).position(|window| window == [0, 0, 1]).unwrap_or(rest.len());
        let unit = &rest[..end];

        rest = &rest[end..];

        //DROP A FOUR-BYTE START CODE'S ZERO
        Some(unit.strip_suffix(&[0]).unwrap_or(unit))
    })
}

//I420 INTO NV12, EDGES PADDED
/// Interleaves I420 planes into NV12, padding the coded area past the picture by repeating the
/// edge pixels.
///
/// # Parameters
/// - `planes`: The picture.
/// - `luma`, `luma_stride`: The destination Y plane and its stride.
/// - `chroma`, `chroma_stride`: The destination interleaved UV plane and its stride.
/// - `coded`: The coded width and height, at least the picture's.
pub fn write_nv12
(
    planes: &Planes,
    luma: &mut [u8],
    luma_stride: usize,
    chroma: &mut [u8],
    chroma_stride: usize,
    coded: (usize, usize),
)
{
    let (width, height) = (planes.width as usize, planes.height as usize);
    let (coded_width, coded_height) = coded;

    for (row, line) in luma.chunks_mut(luma_stride).take(coded_height).enumerate()
    {
        let source = &planes.y[row.min(height - 1) * width..][..width];

        line[..width].copy_from_slice(source);
        line[width..coded_width].fill(source[width - 1]);
    }

    let half = width / 2;

    for (row, line) in chroma.chunks_mut(chroma_stride).take(coded_height / 2).enumerate()
    {
        let offset = row.min(height / 2 - 1) * half;
        let (u, v) = (&planes.u[offset..][..half], &planes.v[offset..][..half]);

        for (pair, (u, v)) in line[..half * 2].chunks_exact_mut(2).zip(u.iter().zip(v))
        {
            pair[0] = *u;
            pair[1] = *v;
        }

        for pair in line[half * 2..coded_width].chunks_exact_mut(2)
        {
            pair[0] = u[half - 1];
            pair[1] = v[half - 1];
        }
    }
}
