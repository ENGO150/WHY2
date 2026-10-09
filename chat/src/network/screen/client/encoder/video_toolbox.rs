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
    ffi::c_void,
    ptr::
    {
        self,
        NonNull,
    },
    slice,
    sync::Mutex,
    time::Instant,
};

use objc2_core_foundation::
{
    CFBoolean,
    CFDictionary,
    CFNumber,
    CFRetained,
    CFString,
    CFType,
};

use objc2_core_media::
{
    self as media,
    CMFormatDescription,
    CMSampleBuffer,
    CMTime,
};

use objc2_core_video::
{
    self as video,
    CVPixelBuffer,
    CVPixelBufferLockFlags,
    CVPixelBufferPool,
};

use objc2_video_toolbox::
{
    self as toolbox,
    VTCompressionSession,
    VTEncodeInfoFlags,
};

use crate::network::screen::client::encoder::
{
    self,
    Backend,
    Budget,
    Planes,
    Settings,
};

const MICROSECONDS: i32 = 1_000_000; //TIMESTAMP SCALE

//STRUCTS
#[derive(Default)]
struct Output //CALLBACK RESULTS
{
    frames: Mutex<Vec<Result<Vec<u8>, i32>>>,
}

pub struct VideoToolboxEncoder //H.264 THROUGH VIDEOTOOLBOX
{
    session: CFRetained<VTCompressionSession>,
    output: Box<Output>,
    budget: Budget,
    started: Instant,
    last_time: i64,
}

impl VideoToolboxEncoder
{
    pub fn new(settings: Settings) -> Result<Self, String>
    {
        let output = Box::new(Output::default());

        //SAFETY: output OUTLIVES THE SESSION
        unsafe
        {
            let yes: &CFType = CFBoolean::new(true).as_ref();

            let specification = CFDictionary::<CFString, CFType>::from_slices
            (
                &[toolbox::kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder],
                &[yes],
            );

            let format = CFNumber::new_i32(video::kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange as i32);
            let width = CFNumber::new_i32(settings.width as i32);
            let height = CFNumber::new_i32(settings.height as i32);
            let surface = CFDictionary::<CFString, CFType>::empty();

            let attributes = CFDictionary::<CFString, CFType>::from_slices
            (
                &[
                    video::kCVPixelBufferPixelFormatTypeKey,
                    video::kCVPixelBufferWidthKey,
                    video::kCVPixelBufferHeightKey,
                    video::kCVPixelBufferIOSurfacePropertiesKey,
                ],
                &[format.as_ref(), width.as_ref(), height.as_ref(), surface.as_ref()],
            );

            let mut raw: *mut VTCompressionSession = ptr::null_mut();

            let status = VTCompressionSession::create
            (
                None,
                settings.width as i32,
                settings.height as i32,
                media::kCMVideoCodecType_H264,
                Some(specification.as_opaque()),
                Some(attributes.as_opaque()),
                None,
                Some(compressed),
                &*output as *const Output as *mut c_void,
                NonNull::from(&mut raw),
            );

            let session = match NonNull::new(raw)
            {
                Some(session) if status == 0 => CFRetained::from_raw(session),
                _ => return Err(format!("no hardware H.264 encoder ({status})")),
            };

            let encoder = Self
            {
                session,
                output,
                budget: Budget::new(settings.bitrate),
                started: Instant::now(),
                last_time: -1,
            };

            encoder.set(toolbox::kVTCompressionPropertyKey_RealTime, yes)?;
            encoder.set(toolbox::kVTCompressionPropertyKey_AllowFrameReordering, CFBoolean::new(false).as_ref())?;

            //CONSTRAINED BASELINE, ELSE BASELINE
            if encoder.set(toolbox::kVTCompressionPropertyKey_ProfileLevel, toolbox::kVTProfileLevel_H264_ConstrainedBaseline_AutoLevel.as_ref()).is_err()
            {
                encoder.set(toolbox::kVTCompressionPropertyKey_ProfileLevel, toolbox::kVTProfileLevel_H264_Baseline_AutoLevel.as_ref())?;
            }

            encoder.set(toolbox::kVTCompressionPropertyKey_AverageBitRate, CFNumber::new_i32(settings.bitrate as i32).as_ref())?;
            encoder.set(toolbox::kVTCompressionPropertyKey_MaxKeyFrameInterval, CFNumber::new_i32(settings.keyframe_interval as i32).as_ref())?;
            encoder.set(toolbox::kVTCompressionPropertyKey_ExpectedFrameRate, CFNumber::new_i32(settings.fps as i32).as_ref()).ok();

            let status = encoder.session.prepare_to_encode_frames();
            if status != 0 { return Err(format!("the encoder will not start ({status})")); }

            Ok(encoder)
        }
    }

    fn set(&self, key: &CFString, value: &CFType) -> Result<(), String> //ONE SESSION PROPERTY
    {
        //SAFETY: LIVE SESSION
        let status = unsafe { toolbox::VTSessionSetProperty(&self.session, key, Some(value)) };

        if status == 0 { Ok(()) } else { Err(format!("the encoder refused a setting ({status})")) }
    }

    fn picture(&self, planes: &Planes) -> Result<CFRetained<CVPixelBuffer>, String> //NV12 FROM THE POOL
    {
        //SAFETY: LOCKED WHILE WRITTEN
        unsafe
        {
            let pool = self.session.pixel_buffer_pool().ok_or_else(|| "the encoder has no buffer pool".to_owned())?;

            let mut raw: *mut CVPixelBuffer = ptr::null_mut();
            let status = CVPixelBufferPool::create_pixel_buffer(None, &pool, NonNull::from(&mut raw));

            let buffer = match NonNull::new(raw)
            {
                Some(buffer) if status == 0 => CFRetained::from_raw(buffer),
                _ => return Err(format!("no pixel buffer ({status})")),
            };

            if video::CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags(0)) != 0 { return Err("the pixel buffer will not lock".to_owned()); }

            let (width, height) = (planes.width as usize, planes.height as usize);

            let luma_stride = video::CVPixelBufferGetBytesPerRowOfPlane(&buffer, 0);
            let chroma_stride = video::CVPixelBufferGetBytesPerRowOfPlane(&buffer, 1);
            let luma = video::CVPixelBufferGetBaseAddressOfPlane(&buffer, 0) as *mut u8;
            let chroma = video::CVPixelBufferGetBaseAddressOfPlane(&buffer, 1) as *mut u8;

            if !luma.is_null() && !chroma.is_null()
            {
                encoder::write_nv12
                (
                    planes,
                    slice::from_raw_parts_mut(luma, luma_stride * height),
                    luma_stride,
                    slice::from_raw_parts_mut(chroma, chroma_stride * height / 2),
                    chroma_stride,
                    (width, height),
                );
            }

            video::CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags(0));

            if luma.is_null() || chroma.is_null() { return Err("the pixel buffer has no planes".to_owned()); }

            Ok(buffer)
        }
    }
}

impl Backend for VideoToolboxEncoder
{
    fn encode(&mut self, planes: &Planes, keyframe: bool) -> Result<Vec<u8>, String>
    {
        if !self.budget.allows(keyframe) { return Ok(Vec::new()); }

        let buffer = self.picture(planes)?;

        //TIMESTAMPS ONLY EVER RISE
        let time = (self.started.elapsed().as_micros() as i64).max(self.last_time + 1);
        self.last_time = time;

        //SAFETY: LIVE SESSION AND BUFFER
        let status = unsafe
        {
            let force = CFDictionary::<CFString, CFType>::from_slices(&[toolbox::kVTEncodeFrameOptionKey_ForceKeyFrame], &[CFBoolean::new(true).as_ref()]);
            let properties = keyframe.then(|| force.as_opaque());

            let status = self.session.encode_frame(&buffer, CMTime::new(time, MICROSECONDS), media::kCMTimeInvalid, properties, ptr::null_mut(), ptr::null_mut());

            if status == 0 { self.session.complete_frames(media::kCMTimeInvalid) } else { status }
        };

        if status != 0 { return Err(format!("the encode failed ({status})")); }

        let frames: Vec<Result<Vec<u8>, i32>> = self.output.frames.lock().map(|mut frames| frames.drain(..).collect()).unwrap_or_default();

        let mut frame = Vec::new();

        for result in frames
        {
            frame = result.map_err(|status| format!("the encode failed ({status})"))?;
        }

        self.budget.spend(frame.len());

        Ok(frame)
    }

    fn set_bitrate(&mut self, bitrate: u32)
    {
        self.budget.set_rate(bitrate);
        //SAFETY: FRAMEWORK CONSTANT
        let key = unsafe { toolbox::kVTCompressionPropertyKey_AverageBitRate };

        self.set(key, CFNumber::new_i32(bitrate as i32).as_ref()).ok();
    }

    fn hardware(&self) -> bool
    {
        true
    }
}

impl Drop for VideoToolboxEncoder
{
    fn drop(&mut self)
    {
        //SAFETY: NO CALLBACKS AFTER THIS
        unsafe { self.session.invalidate() };
    }
}

//FUNCTIONS
unsafe extern "C-unwind" fn compressed //ONE ENCODED FRAME
(
    refcon: *mut c_void,
    _frame: *mut c_void,
    status: i32,
    _flags: VTEncodeInfoFlags,
    sample: *mut CMSampleBuffer,
)
{
    //SAFETY: refcon IS Output
    let output = unsafe { &*(refcon as *const Output) };

    let result = match NonNull::new(sample)
    {
        //SAFETY: VIDEOTOOLBOX'S SAMPLE
        Some(sample) if status == 0 => Ok(unsafe { annex_b(sample.as_ref()) }),
        None if status == 0 => Ok(Vec::new()),
        _ => Err(status),
    };

    if let Ok(mut frames) = output.frames.lock() { frames.push(result); }
}

//SAFETY: LIVE SAMPLE BUFFER
unsafe fn annex_b(sample: &CMSampleBuffer) -> Vec<u8> //AVCC INTO ANNEX B
{
    unsafe
    {
        let Some(block) = sample.data_buffer() else { return Vec::new() };

        let length = block.data_length();
        let mut data = vec![0u8; length];

        let Some(destination) = NonNull::new(data.as_mut_ptr() as *mut c_void) else { return Vec::new() };
        if block.copy_data_bytes(0, length, destination) != 0 { return Vec::new(); }

        let format = sample.format_description();
        let (sets, prefix) = format.as_deref().map(|format| parameter_sets(format)).unwrap_or_default();
        let prefix = if prefix == 0 { 4 } else { prefix };

        let mut frame = Vec::with_capacity(length + sets.len() + 16);
        let mut idr = false;
        let mut rest = data.as_slice();

        while rest.len() > prefix
        {
            let size = rest[..prefix].iter().fold(0usize, |size, byte| size << 8 | usize::from(*byte));
            let Some(unit) = rest.get(prefix..prefix + size) else { break };

            idr |= unit.first().is_some_and(|header| header & 0x1f == 5);

            frame.extend_from_slice(&[0, 0, 0, 1]);
            frame.extend_from_slice(unit);

            rest = &rest[prefix + size..];
        }

        if !idr { return frame; }

        let mut complete = sets;
        complete.extend_from_slice(&frame);

        complete
    }
}

//SAFETY: LIVE FORMAT DESCRIPTION
unsafe fn parameter_sets(format: &CMFormatDescription) -> (Vec<u8>, usize) //SPS, PPS AND PREFIX SIZE
{
    let mut sets = Vec::new();
    let mut prefix = 0;
    let mut index = 0;

    loop
    {
        let mut pointer: *const u8 = ptr::null();
        let mut size = 0usize;
        let mut count = 0usize;

        //SAFETY: LOCAL OUT POINTERS
        let status = unsafe { media::CMVideoFormatDescriptionGetH264ParameterSetAtIndex(format, index, &mut pointer, &mut size, &mut count, &mut prefix) };

        if status != 0 || pointer.is_null() { break; }

        sets.extend_from_slice(&[0, 0, 0, 1]);

        //SAFETY: size BYTES AT pointer
        sets.extend_from_slice(unsafe { slice::from_raw_parts(pointer, size) });

        index += 1;

        if index >= count { break; }
    }

    (sets, prefix.max(0) as usize)
}
