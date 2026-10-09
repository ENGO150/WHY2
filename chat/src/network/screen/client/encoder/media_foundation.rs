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
    collections::VecDeque,
    mem::ManuallyDrop,
    ptr,
    slice,
    thread,
    time::{ Duration, Instant },
};

use windows::
{
    core::Interface,
    Win32::
    {
        Media::MediaFoundation,
        System::
        {
            Com,
            Variant,
        },
    },
};

use crate::network::screen::client::encoder::
{
    self,
    Backend,
    Budget,
    ParameterSets,
    Planes,
    Settings,
};

const EVENT_TIMEOUT: Duration = Duration::from_secs(1); //ASYNC MFT STALL LIMIT
const EVENT_POLL: Duration = Duration::from_millis(1);
const TICKS_PER_SECOND: i64 = 10_000_000;              //100ns UNITS

//STRUCTS
struct Startup //MEDIA FOUNDATION AND COM
{
    com: bool,
}

impl Drop for Startup
{
    fn drop(&mut self)
    {
        //SAFETY: PAIRS start
        unsafe
        {
            MediaFoundation::MFShutdown().ok();

            if self.com { Com::CoUninitialize(); }
        }
    }
}

pub struct MediaFoundationEncoder //H.264 THROUGH A HARDWARE MFT
{
    transform: MediaFoundation::IMFTransform,
    activate: MediaFoundation::IMFActivate,
    events: Option<MediaFoundation::IMFMediaEventGenerator>,
    codec: Option<MediaFoundation::ICodecAPI>,
    output_info: MediaFoundation::MFT_OUTPUT_STREAM_INFO,
    inputs: u32,                 //PENDING INPUT REQUESTS
    pending: VecDeque<Vec<u8>>,  //QUEUED OUTPUT
    sets: ParameterSets,
    budget: Budget,
    width: u32,
    height: u32,
    started: Instant,
    duration: i64,
    last_time: i64,
    _startup: Startup,
}

impl MediaFoundationEncoder
{
    pub fn new(settings: Settings) -> Result<Self, String>
    {
        let startup = start()?;

        let input = MediaFoundation::MFT_REGISTER_TYPE_INFO
        {
            guidMajorType: MediaFoundation::MFMediaType_Video,
            guidSubtype: MediaFoundation::MFVideoFormat_NV12,
        };

        let output = MediaFoundation::MFT_REGISTER_TYPE_INFO
        {
            guidMajorType: MediaFoundation::MFMediaType_Video,
            guidSubtype: MediaFoundation::MFVideoFormat_H264,
        };

        let mut activates: *mut Option<MediaFoundation::IMFActivate> = ptr::null_mut();
        let mut count = 0u32;

        //SAFETY: ARRAY FREED BELOW
        let candidates: Vec<MediaFoundation::IMFActivate> = unsafe
        {
            MediaFoundation::MFTEnumEx
            (
                MediaFoundation::MFT_CATEGORY_VIDEO_ENCODER,
                MediaFoundation::MFT_ENUM_FLAG_HARDWARE | MediaFoundation::MFT_ENUM_FLAG_SORTANDFILTER,
                Some(&input),
                Some(&output),
                &mut activates,
                &mut count,
            ).map_err(|error| error.to_string())?;

            if activates.is_null() { return Err("no hardware H.264 encoder".to_owned()); }

            let taken = slice::from_raw_parts_mut(activates, count as usize).iter_mut().filter_map(Option::take).collect();
            Com::CoTaskMemFree(Some(activates as *const _));

            taken
        };

        let mut last = "no hardware H.264 encoder".to_owned();
        let mut startup = Some(startup);

        for activate in candidates
        {
            match open(activate, settings)
            {
                Ok(mut encoder) =>
                {
                    encoder._startup = startup.take().unwrap_or(Startup { com: false });
                    return Ok(encoder);
                },

                Err(error) => last = error,
            }
        }

        Err(last)
    }

    fn sample(&mut self, planes: &Planes) -> Result<MediaFoundation::IMFSample, String> //NV12 INTO A SAMPLE
    {
        let (width, height) = (self.width as usize, self.height as usize);
        let size = width * height * 3 / 2;

        //SAFETY: LOCKED size BYTES
        unsafe
        {
            let buffer = MediaFoundation::MFCreateMemoryBuffer(size as u32).map_err(|error| error.to_string())?;

            let mut data = ptr::null_mut();
            buffer.Lock(&mut data, None, None).map_err(|error| error.to_string())?;

            let (luma, chroma) = slice::from_raw_parts_mut(data, size).split_at_mut(width * height);
            encoder::write_nv12(planes, luma, width, chroma, width, (width, height));

            buffer.Unlock().map_err(|error| error.to_string())?;
            buffer.SetCurrentLength(size as u32).map_err(|error| error.to_string())?;

            let sample = MediaFoundation::MFCreateSample().map_err(|error| error.to_string())?;
            sample.AddBuffer(&buffer).map_err(|error| error.to_string())?;

            //TIMESTAMPS ONLY EVER RISE
            let time = ((self.started.elapsed().as_nanos() / 100) as i64).max(self.last_time + 1);
            self.last_time = time;

            sample.SetSampleTime(time).map_err(|error| error.to_string())?;
            sample.SetSampleDuration(self.duration).map_err(|error| error.to_string())?;

            Ok(sample)
        }
    }

    fn next_event(&self, events: &MediaFoundation::IMFMediaEventGenerator) -> Result<MediaFoundation::MF_EVENT_TYPE, String> //NEXT EVENT
    {
        let deadline = Instant::now() + EVENT_TIMEOUT;

        loop
        {
            //SAFETY: A LIVE EVENT GENERATOR
            match unsafe { events.GetEvent(MediaFoundation::MF_EVENT_FLAG_NO_WAIT) }
            {
                Ok(event) =>
                {
                    //SAFETY: A LIVE EVENT
                    let kind = unsafe { event.GetType() }.map_err(|error| error.to_string())?;
                    return Ok(MediaFoundation::MF_EVENT_TYPE(kind as i32));
                },

                Err(error) if error.code() == MediaFoundation::MF_E_NO_EVENTS_AVAILABLE && Instant::now() < deadline => thread::sleep(EVENT_POLL),
                Err(error) => return Err(error.to_string()),
            }
        }
    }

    fn output(&mut self) -> Result<Option<Vec<u8>>, String> //ONE ACCESS UNIT, IF READY
    {
        let provides = self.output_info.dwFlags
            & (MediaFoundation::MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 | MediaFoundation::MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0) as u32 != 0;

        //SAFETY: FIELDS TAKEN BACK BELOW
        unsafe
        {
            let sample = if provides
            {
                None
            } else
            {
                let size = self.output_info.cbSize.max(self.width * self.height * 3 / 2);
                let buffer = MediaFoundation::MFCreateMemoryBuffer(size).map_err(|error| error.to_string())?;
                let sample = MediaFoundation::MFCreateSample().map_err(|error| error.to_string())?;

                sample.AddBuffer(&buffer).map_err(|error| error.to_string())?;
                Some(sample)
            };

            let mut buffer = MediaFoundation::MFT_OUTPUT_DATA_BUFFER
            {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(sample),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            };

            let mut status = 0;
            let result = self.transform.ProcessOutput(0, slice::from_mut(&mut buffer), &mut status);

            let sample = ManuallyDrop::into_inner(buffer.pSample);
            drop(ManuallyDrop::into_inner(buffer.pEvents));

            match result
            {
                Ok(()) => {},

                Err(error) if error.code() == MediaFoundation::MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(None),

                //TAKE THE MFT'S OUTPUT TYPE
                Err(error) if error.code() == MediaFoundation::MF_E_TRANSFORM_STREAM_CHANGE =>
                {
                    let kind = self.transform.GetOutputAvailableType(0, 0).map_err(|error| error.to_string())?;
                    self.transform.SetOutputType(0, &kind, 0).map_err(|error| error.to_string())?;
                    self.output_info = self.transform.GetOutputStreamInfo(0).map_err(|error| error.to_string())?;

                    return Ok(None);
                },

                Err(error) => return Err(error.to_string()),
            }

            let Some(sample) = sample else { return Ok(None) };

            let buffer = sample.ConvertToContiguousBuffer().map_err(|error| error.to_string())?;

            let mut data = ptr::null_mut();
            let mut length = 0u32;
            buffer.Lock(&mut data, None, Some(&mut length)).map_err(|error| error.to_string())?;

            let bytes = slice::from_raw_parts(data, length as usize).to_vec();

            buffer.Unlock().map_err(|error| error.to_string())?;

            Ok((!bytes.is_empty()).then_some(bytes))
        }
    }

    fn run_async(&mut self, events: &MediaFoundation::IMFMediaEventGenerator, sample: &MediaFoundation::IMFSample) -> Result<(), String>
    {
        //WAIT FOR AN INPUT REQUEST
        while self.inputs == 0
        {
            match self.next_event(events)?
            {
                MediaFoundation::METransformNeedInput => self.inputs += 1,
                MediaFoundation::METransformHaveOutput => if let Some(frame) = self.output()? { self.pending.push_back(frame); },
                _ => {},
            }
        }

        //SAFETY: LIVE TRANSFORM
        unsafe { self.transform.ProcessInput(0, sample, 0) }.map_err(|error| error.to_string())?;
        self.inputs -= 1;

        //OUTPUT, OR ANOTHER INPUT REQUEST
        loop
        {
            match self.next_event(events)?
            {
                MediaFoundation::METransformHaveOutput =>
                {
                    if let Some(frame) = self.output()? { self.pending.push_back(frame); }
                    return Ok(());
                },

                MediaFoundation::METransformNeedInput =>
                {
                    self.inputs += 1;
                    return Ok(());
                },

                _ => {},
            }
        }
    }

    fn run_sync(&mut self, sample: &MediaFoundation::IMFSample) -> Result<(), String>
    {
        //SAFETY: LIVE TRANSFORM
        unsafe { self.transform.ProcessInput(0, sample, 0) }.map_err(|error| error.to_string())?;

        while let Some(frame) = self.output()?
        {
            self.pending.push_back(frame);
        }

        Ok(())
    }

    fn set(&self, key: &windows::core::GUID, value: u32) //BEST-EFFORT SETTING
    {
        if let Some(codec) = &self.codec
        {
            //SAFETY: LIVE ICodecAPI
            unsafe { codec.SetValue(key, &variant(value)).ok() };
        }
    }
}

impl Backend for MediaFoundationEncoder
{
    fn encode(&mut self, planes: &Planes, keyframe: bool) -> Result<Vec<u8>, String>
    {
        if !self.budget.allows(keyframe) { return Ok(Vec::new()); }

        if keyframe { self.set(&MediaFoundation::CODECAPI_AVEncVideoForceKeyFrame, 1); }

        let sample = self.sample(planes)?;

        match self.events.clone()
        {
            Some(events) => self.run_async(&events, &sample)?,
            None => self.run_sync(&sample)?,
        }

        //ONE ACCESS UNIT A CALL
        let Some(frame) = self.pending.pop_front() else { return Ok(Vec::new()) };

        let frame = self.sets.complete(frame);
        self.budget.spend(frame.len());

        Ok(frame)
    }

    fn set_bitrate(&mut self, bitrate: u32)
    {
        self.budget.set_rate(bitrate);
        self.set(&MediaFoundation::CODECAPI_AVEncCommonMeanBitRate, bitrate);
        self.set(&MediaFoundation::CODECAPI_AVEncCommonMaxBitRate, bitrate);
    }

    fn hardware(&self) -> bool
    {
        true
    }
}

impl Drop for MediaFoundationEncoder
{
    fn drop(&mut self)
    {
        //SAFETY: LIVE INTERFACES
        unsafe
        {
            self.transform.ProcessMessage(MediaFoundation::MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0).ok();
            self.transform.ProcessMessage(MediaFoundation::MFT_MESSAGE_NOTIFY_END_STREAMING, 0).ok();

            if let Ok(shutdown) = self.transform.cast::<MediaFoundation::IMFShutdown>() { shutdown.Shutdown().ok(); }

            self.activate.ShutdownObject().ok();
        }
    }
}

//FUNCTIONS
fn start() -> Result<Startup, String> //COM, THEN MEDIA FOUNDATION
{
    //SAFETY: PAIRED IN Drop
    unsafe
    {
        let com = Com::CoInitializeEx(None, Com::COINIT_MULTITHREADED).is_ok();

        if let Err(error) = MediaFoundation::MFStartup(MediaFoundation::MF_VERSION, MediaFoundation::MFSTARTUP_LITE)
        {
            if com { Com::CoUninitialize(); }
            return Err(error.to_string());
        }

        Ok(Startup { com })
    }
}

fn open(activate: MediaFoundation::IMFActivate, settings: Settings) -> Result<MediaFoundationEncoder, String> //ONE MFT, SET UP
{
    //SAFETY: LIVE INTERFACES
    unsafe
    {
        let transform: MediaFoundation::IMFTransform = activate.ActivateObject().map_err(|error| error.to_string())?;

        match configure(&transform, settings)
        {
            Ok((events, codec)) =>
            {
                let output_info = transform.GetOutputStreamInfo(0).map_err(|error| error.to_string())?;

                let mut sets = ParameterSets::default();

                //SEED THE PARAMETER SETS
                if let Ok(kind) = transform.GetOutputCurrentType(0)
                    && let Ok(size) = kind.GetBlobSize(&MediaFoundation::MF_MT_MPEG_SEQUENCE_HEADER)
                {
                    let mut header = vec![0; size as usize];

                    if kind.GetBlob(&MediaFoundation::MF_MT_MPEG_SEQUENCE_HEADER, &mut header, None).is_ok()
                    {
                        sets.complete(header);
                    }
                }

                Ok(MediaFoundationEncoder
                {
                    transform,
                    activate,
                    events,
                    codec,
                    output_info,
                    inputs: 0,
                    pending: VecDeque::new(),
                    sets,
                    budget: Budget::new(settings.bitrate),
                    width: settings.width,
                    height: settings.height,
                    started: Instant::now(),
                    duration: TICKS_PER_SECOND / i64::from(settings.fps.max(1)),
                    last_time: -1,
                    _startup: Startup { com: false },
                })
            },

            Err(error) =>
            {
                if let Ok(shutdown) = transform.cast::<MediaFoundation::IMFShutdown>() { shutdown.Shutdown().ok(); }
                activate.ShutdownObject().ok();

                Err(error)
            },
        }
    }
}

type Configured = (Option<MediaFoundation::IMFMediaEventGenerator>, Option<MediaFoundation::ICodecAPI>);

//SAFETY: LIVE TRANSFORM
unsafe fn configure(transform: &MediaFoundation::IMFTransform, settings: Settings) -> Result<Configured, String>
{
    unsafe
    {
        //UNLOCK AN ASYNC MFT
        let asynchronous = transform.GetAttributes().ok().filter(|attributes|
        {
            attributes.GetUINT32(&MediaFoundation::MF_TRANSFORM_ASYNC).unwrap_or(0) != 0
                && attributes.SetUINT32(&MediaFoundation::MF_TRANSFORM_ASYNC_UNLOCK, 1).is_ok()
        }).is_some();

        let events = if asynchronous
        {
            Some(transform.cast::<MediaFoundation::IMFMediaEventGenerator>().map_err(|error| error.to_string())?)
        } else
        {
            None
        };

        let codec = transform.cast::<MediaFoundation::ICodecAPI>().ok();

        //RATE AND LATENCY FIRST
        if let Some(codec) = &codec
        {
            let peak = variant(MediaFoundation::eAVEncCommonRateControlMode_PeakConstrainedVBR.0 as u32);

            if codec.SetValue(&MediaFoundation::CODECAPI_AVEncCommonRateControlMode, &peak).is_err()
            {
                codec.SetValue(&MediaFoundation::CODECAPI_AVEncCommonRateControlMode, &variant(MediaFoundation::eAVEncCommonRateControlMode_CBR.0 as u32)).ok();
            }

            codec.SetValue(&MediaFoundation::CODECAPI_AVEncCommonMeanBitRate, &variant(settings.bitrate)).ok();
            codec.SetValue(&MediaFoundation::CODECAPI_AVEncCommonMaxBitRate, &variant(settings.bitrate)).ok();
            codec.SetValue(&MediaFoundation::CODECAPI_AVEncMPVGOPSize, &variant(settings.keyframe_interval)).ok();
            codec.SetValue(&MediaFoundation::CODECAPI_AVEncMPVDefaultBPictureCount, &variant(0)).ok();
            codec.SetValue(&MediaFoundation::CODECAPI_AVLowLatencyMode, &variant_bool(true)).ok();
        }

        let size = (u64::from(settings.width) << 32) | u64::from(settings.height);
        let rate = (u64::from(settings.fps.max(1)) << 32) | 1;

        let output = MediaFoundation::MFCreateMediaType().map_err(|error| error.to_string())?;
        output.SetGUID(&MediaFoundation::MF_MT_MAJOR_TYPE, &MediaFoundation::MFMediaType_Video).map_err(|error| error.to_string())?;
        output.SetGUID(&MediaFoundation::MF_MT_SUBTYPE, &MediaFoundation::MFVideoFormat_H264).map_err(|error| error.to_string())?;
        output.SetUINT32(&MediaFoundation::MF_MT_AVG_BITRATE, settings.bitrate).map_err(|error| error.to_string())?;
        output.SetUINT64(&MediaFoundation::MF_MT_FRAME_SIZE, size).map_err(|error| error.to_string())?;
        output.SetUINT64(&MediaFoundation::MF_MT_FRAME_RATE, rate).map_err(|error| error.to_string())?;
        output.SetUINT64(&MediaFoundation::MF_MT_PIXEL_ASPECT_RATIO, (1 << 32) | 1).map_err(|error| error.to_string())?;
        output.SetUINT32(&MediaFoundation::MF_MT_INTERLACE_MODE, MediaFoundation::MFVideoInterlace_Progressive.0 as u32).map_err(|error| error.to_string())?;

        //CONSTRAINED BASELINE, ELSE BASELINE
        let profiled = [MediaFoundation::eAVEncH264VProfile_ConstrainedBase, MediaFoundation::eAVEncH264VProfile_Base]
            .into_iter()
            .any(|profile|
            {
                output.SetUINT32(&MediaFoundation::MF_MT_MPEG2_PROFILE, profile.0 as u32).is_ok()
                    && transform.SetOutputType(0, &output, 0).is_ok()
            });

        if !profiled { return Err("the MFT will not do baseline".to_owned()); }

        let input = MediaFoundation::MFCreateMediaType().map_err(|error| error.to_string())?;
        input.SetGUID(&MediaFoundation::MF_MT_MAJOR_TYPE, &MediaFoundation::MFMediaType_Video).map_err(|error| error.to_string())?;
        input.SetGUID(&MediaFoundation::MF_MT_SUBTYPE, &MediaFoundation::MFVideoFormat_NV12).map_err(|error| error.to_string())?;
        input.SetUINT64(&MediaFoundation::MF_MT_FRAME_SIZE, size).map_err(|error| error.to_string())?;
        input.SetUINT64(&MediaFoundation::MF_MT_FRAME_RATE, rate).map_err(|error| error.to_string())?;
        input.SetUINT64(&MediaFoundation::MF_MT_PIXEL_ASPECT_RATIO, (1 << 32) | 1).map_err(|error| error.to_string())?;
        input.SetUINT32(&MediaFoundation::MF_MT_INTERLACE_MODE, MediaFoundation::MFVideoInterlace_Progressive.0 as u32).map_err(|error| error.to_string())?;
        input.SetUINT32(&MediaFoundation::MF_MT_DEFAULT_STRIDE, settings.width).map_err(|error| error.to_string())?;

        transform.SetInputType(0, &input, 0).map_err(|error| error.to_string())?;

        transform.ProcessMessage(MediaFoundation::MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0).map_err(|error| error.to_string())?;
        transform.ProcessMessage(MediaFoundation::MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0).map_err(|error| error.to_string())?;

        Ok((events, codec))
    }
}

fn variant(value: u32) -> Variant::VARIANT //A VT_UI4
{
    let mut variant = Variant::VARIANT::default();

    //SAFETY: TAG MATCHES FIELD
    unsafe
    {
        let inner = &mut *variant.Anonymous.Anonymous;
        inner.vt = Variant::VT_UI4;
        inner.Anonymous.ulVal = value;
    }

    variant
}

fn variant_bool(value: bool) -> Variant::VARIANT //A VT_BOOL
{
    let mut variant = Variant::VARIANT::default();

    //SAFETY: TAG MATCHES FIELD
    unsafe
    {
        let inner = &mut *variant.Anonymous.Anonymous;
        inner.vt = Variant::VT_BOOL;
        inner.Anonymous.boolVal = value.into();
    }

    variant
}
