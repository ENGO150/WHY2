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

//openh264 SetOption VALUES
const ENCODER_OPTION_BITRATE: c_int = 5;
const ENCODER_OPTION_MAX_BITRATE: c_int = 6;
const SPATIAL_LAYER_0: c_int = 0;
const SPATIAL_LAYER_ALL: c_int = 4;

//STRUCTS
pub struct Planes<'a> //ONE TIGHTLY PACKED I420 PICTURE
{
    pub width: u32,
    pub height: u32,
    pub y: &'a [u8],
    pub u: &'a [u8],
    pub v: &'a [u8],
}

#[derive(Clone, Copy)]
pub struct Settings //WHAT EVERY BACKEND IS ASKED FOR
{
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u32,
    pub keyframe_interval: u32,
}

impl Settings
{
    pub fn new(width: u32, height: u32, fps: u32, bitrate: u32) -> Self
    {
        Self { width, height, fps, bitrate, keyframe_interval: fps * 2 }
    }
}

pub struct Budget //SKIPS FRAMES OVER THE BITRATE
{
    rate: f64,
    balance: f64,
    refilled: Instant,
}

impl Budget
{
    pub fn new(bitrate: u32) -> Self
    {
        Self { rate: f64::from(bitrate), balance: f64::from(bitrate), refilled: Instant::now() }
    }

    pub fn allows(&mut self, keyframe: bool) -> bool //MAY THIS FRAME BE ENCODED
    {
        let now = Instant::now();

        //REFILL, UP TO ONE SECOND
        self.balance = (self.balance + self.rate * (now - self.refilled).as_secs_f64()).min(self.rate);
        self.refilled = now;

        keyframe || self.balance >= 0.0
    }

    pub fn spend(&mut self, bytes: usize)
    {
        self.balance -= bytes as f64 * 8.0;
    }

    pub fn set_rate(&mut self, bitrate: u32)
    {
        self.rate = f64::from(bitrate);
        self.balance = self.balance.min(self.rate);
    }
}

#[derive(Default)]
pub struct ParameterSets //THE LAST SPS AND PPS SEEN
{
    sps: Vec<u8>,
    pps: Vec<u8>,
}

impl ParameterSets
{
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

struct Software //OPENH264, ON THE CPU
{
    encoder: Encoder,
    started: bool,           //INITIALISED BY THE FIRST ENCODE
    bitrate: Option<u32>,    //WAITING FOR THAT
    epoch: Instant,          //WHERE TIMESTAMPS COUNT FROM
    applied: u32,            //THE ENCODER'S CURRENT TARGET
}

#[repr(C)]
struct BitrateInfo //openh264's SBitrateInfo
{
    layer: c_int,
    bitrate: c_int,
}

impl Software
{
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
pub trait Backend //ONE H.264 ENCODER
{
    //ONE ACCESS UNIT, EMPTY IF SKIPPED
    fn encode(&mut self, planes: &Planes, keyframe: bool) -> Result<Vec<u8>, String>;

    //NEW TARGET, MID-STREAM
    fn set_bitrate(&mut self, bitrate: u32);

    fn hardware(&self) -> bool;
}

//PRIVATE
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

fn annex_b(unit: &[u8]) -> Vec<u8> //ONE NAL UNIT WITH ITS START CODE
{
    let mut framed = Vec::with_capacity(unit.len() + 4);
    framed.extend_from_slice(&[0, 0, 0, 1]);
    framed.extend_from_slice(unit);

    framed
}

//PUBLIC
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
