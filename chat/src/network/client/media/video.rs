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

//! # Embedded Video Demuxing and Decoding
//!
//! Container parsing is independent of decoding: MOV/MP4 and Matroska/WebM packets
//! feed H.264, HEVC, VP9 or Motion JPEG decoders. Only timestamps and encoded bytes
//! are retained for seeking; pictures are produced incrementally for previews and playback.

use std::
{
    io::Cursor,
    sync::Arc,
    time::Duration,
};

use tokio::sync::mpsc::Sender;

use image::
{
    DynamicImage,
    RgbImage,
    ImageFormat,
    ImageReader,
    Limits,
};

use mp4::
{
    MediaType,
    Mp4Reader,
    TrackType as Mp4TrackType,
};

use openh264::
{
    formats::YUVSource,
    decoder::
    {
        Decoder as AvcDecoder,
        DecodeOptions,
        DecodedYUV,
        Flush,
    },
};

use rust_h265::
{
    Decoder as HevcDecoder,
    Frame as HevcFrame,
    PixelData,
    DecodeLimits,
};

use vp9_mt::
{
    Vp9Decoder,
    Picture,
    Error as Vp9Error,
    BitReader,
};

use symphonia::
{
    default,
    core::
    {
        formats::
        {
            FormatReader,
            FormatOptions,
            probe::Hint,
        },
        io::MediaSourceStream,
        meta::MetadataOptions,
        units::TimeBase,
        codecs::video::{ VideoCodecParameters, well_known },
    },
};

use crate::
{
    consts,
    network::
    {
        voice::client::message,
        client::
        {
            media,
            image::ImageFrame,
            ClientEvent,
        },
    },
};

use super::Session;

//ENUMS
/// The common containers and a specialized QuickTime/MP4 sample reader.
enum Input
{
    Container { format: Box<dyn FormatReader>, track: u32, time_base: TimeBase },
    QuickTime { reader: Mp4Reader<Cursor<Arc<[u8]>>>, track: u32, next: u32, count: u32, scale: u64 },
}

/// Embedded decoders, without platform codec or player processes.
enum Codec
{
    Avc { decoder: AvcDecoder, length_size: usize },
    Hevc { decoder: HevcDecoder, length_size: usize },
    Vp9 { decoder: Vp9Decoder, references: [(u32, u32); 8] },
    Jpeg,
}

/// Borrowed planar pixels, including high-bit-depth HEVC and VP9 frames.
enum Plane<'a>
{
    U8(&'a [u8]),
    U16(&'a [u16]),
}

//STRUCTS
struct Packet
{
    data: Box<[u8]>,
    time: u64,
}

struct Frame
{
    image: ImageFrame,
    time: Option<u64>,
}

pub(super) struct Video
{
    input: Input,
    codec: Codec,
    times: Vec<u64>,
}

//IMPLEMENTATIONS
impl Input
{
    fn open(data: Arc<[u8]>) -> Result<Option<(Self, VideoCodecParameters)>, &'static str>
    {
        let source = MediaSourceStream::new(Box::new(Cursor::new(data.clone())), Default::default());
        let mut unsupported_video = false;

        if let Ok(format) = default::get_probe().probe(&Hint::new(), source, FormatOptions::default(), MetadataOptions::default())
        {
            if let Some(track) = format.tracks().iter().find(|track|
                track.codec_params.as_ref().and_then(|params| params.video()).is_some())
            {
                if track.num_frames.is_some_and(|count| count > consts::MAX_MEDIA_SAMPLES as u64)
                {
                    return Err("media.unsupported");
                }

                let params = track.codec_params.as_ref().and_then(|params| params.video()).unwrap().clone();
                let id = track.id;
                let time_base = track.time_base.ok_or("media.unsupported")?;

                if matches!(params.codec, well_known::CODEC_ID_H264 | well_known::CODEC_ID_HEVC
                    | well_known::CODEC_ID_VP9 | well_known::CODEC_ID_MJPEG)
                {
                    return Ok(Some((Self::Container { format, track: id, time_base }, params)));
                }

                unsupported_video = true;
            }
        }

        //QUICKTIME JPEG AND MP4 VP9 ENTRIES NEED THE SPECIALIZED SAMPLE READER
        let size = data.len() as u64;
        let Ok(mut reader) = Mp4Reader::read_header(Cursor::new(data), size) else
        {
            return if unsupported_video { Err("media.unsupported") } else { Ok(None) };
        };
        let Some(track) = reader.tracks().values().find(|track| track.track_type().ok() == Some(Mp4TrackType::Video))
            else { return Ok(None) };
        let id = track.track_id();
        let count = track.sample_count();
        let scale = track.timescale() as u64;
        let sizes = &track.trak.mdia.minf.stbl.stsz;

        if count == 0 || count > consts::MAX_MEDIA_SAMPLES || scale == 0
            || sizes.sample_size as u64 > size || sizes.sample_sizes.iter().any(|sample| *sample as u64 > size)
        {
            return Err("media.unsupported");
        }

        let vp9 = track.media_type().ok() == Some(MediaType::VP9);
        let mut params = VideoCodecParameters::default();

        if vp9
        {
            params.codec = well_known::CODEC_ID_VP9;
            params.width = Some(track.width());
            params.height = Some(track.height());
        } else
        {
            let first = reader.read_sample(id, 1).map_err(|_| "media.failed")?.ok_or("media.failed")?;
            if !first.bytes.starts_with(&[0xff, 0xd8]) { return Err("media.unsupported"); }
            params.codec = well_known::CODEC_ID_MJPEG;
        }

        Ok(Some((Self::QuickTime { reader, track: id, next: 1, count, scale }, params)))
    }

    fn next(&mut self) -> Result<Option<Packet>, &'static str>
    {
        match self
        {
            Self::Container { format, track, time_base } =>
            {
                while let Some(packet) = format.next_packet().map_err(|_| "media.failed")?
                {
                    if packet.track_id != *track { continue; }
                    let time = time_base.calc_time(packet.pts).ok_or("media.failed")?.as_millis().max(0) as u64;

                    return Ok(Some(Packet { data: packet.data, time }));
                }

                Ok(None)
            },

            Self::QuickTime { reader, track, next, count, scale } =>
            {
                if *next > *count { return Ok(None); }
                let sample = reader.read_sample(*track, *next).map_err(|_| "media.failed")?.ok_or("media.failed")?;
                *next += 1;
                let time = (sample.start_time as i128 + sample.rendering_offset as i128).max(0) as u64;

                Ok(Some(Packet { data: sample.bytes.to_vec().into_boxed_slice(), time: time.saturating_mul(1000) / *scale }))
            },
        }
    }
}

impl Codec
{
    fn open(params: &VideoCodecParameters) -> Result<Self, &'static str>
    {
        if params.width.zip(params.height).is_some_and(|(width, height)|
            width == 0 || height == 0 || width as usize * height as usize > consts::MAX_MEDIA_PIXELS)
        {
            return Err("media.unsupported");
        }

        match params.codec
        {
            well_known::CODEC_ID_H264 =>
            {
                let mut decoder = AvcDecoder::new().map_err(|_| "media.failed")?;
                let configuration = params.extra_data.iter().find(|data|
                    data.id == well_known::extra_data::VIDEO_EXTRA_DATA_ID_AVC_DECODER_CONFIG);
                let (length_size, annex) = match configuration
                {
                    Some(data) => avc_configuration(&data.data)?,
                    None => (0, Vec::new()),
                };
                if !annex.is_empty() { decoder.decode(&annex).map_err(|_| "media.failed")?; }

                Ok(Self::Avc { decoder, length_size })
            },

            well_known::CODEC_ID_HEVC =>
            {
                let mut decoder = HevcDecoder::with_limits(DecodeLimits
                {
                    max_picture_pixels: consts::MAX_MEDIA_PIXELS as u64,
                    max_picture_memory_bytes: consts::MAX_IMAGE_ALLOC,
                }).with_display_order();
                let configuration = params.extra_data.iter().find(|data|
                    data.id == well_known::extra_data::VIDEO_EXTRA_DATA_ID_HEVC_DECODER_CONFIG);
                let (length_size, annex) = match configuration
                {
                    Some(data) => hevc_configuration(&data.data)?,
                    None => (0, Vec::new()),
                };

                for nal in rust_h265::parse_annex_b(&annex)
                {
                    decoder.decode_nal(&nal).map_err(|_| "media.failed")?;
                }

                Ok(Self::Hevc { decoder, length_size })
            },

            well_known::CODEC_ID_VP9 => Ok(Self::Vp9 { decoder: Vp9Decoder::with_threads(1), references: [(0, 0); 8] }),
            well_known::CODEC_ID_MJPEG => Ok(Self::Jpeg),
            _ => Err("media.unsupported"),
        }
    }

    fn decode(&mut self, packet: &Packet) -> Result<Vec<Frame>, &'static str>
    {
        let mut frames = Vec::new();

        match self
        {
            Self::Avc { decoder, length_size } =>
            {
                let annex = annex_b(&packet.data, *length_size)?;
                if let Some(frame) = decoder.decode_with_options(&annex,
                    DecodeOptions::new().flush_after_decode(Flush::NoFlush)).map_err(|_| "media.failed")?
                {
                    frames.push(Frame { image: avc_image(frame)?, time: None });
                }
            },

            Self::Hevc { decoder, length_size } =>
            {
                let annex = annex_b(&packet.data, *length_size)?;
                for nal in rust_h265::parse_annex_b(&annex)
                {
                    decoder.decode_nal(&nal).map_err(|_| "media.failed")?;
                    while let Some(frame) = decoder.next_output()
                    {
                        if frames.len() >= 32 { return Err("media.unsupported"); }
                        frames.push(Frame { image: hevc_image(&frame)?, time: None });
                    }
                }
            },

            Self::Vp9 { decoder, references } =>
            {
                guard_vp9(&packet.data, references)?;
                decoder.push(&packet.data, Some(packet.time.min(i64::MAX as u64) as i64)).map_err(|_| "media.failed")?;

                while decoder.pending() > 0
                {
                    match decoder.next_picture()
                    {
                        Ok(picture) => frames.push(Frame { image: vp9_image(&picture)?, time: picture.pts.map(|time| time.max(0) as u64) }),
                        Err(Vp9Error::Again) => {},
                        Err(_) => return Err("media.failed"),
                    }
                }
            },

            Self::Jpeg =>
            {
                let mut reader = ImageReader::with_format(Cursor::new(&packet.data), ImageFormat::Jpeg);
                let mut limits = Limits::default();
                limits.max_image_width = Some(4096);
                limits.max_image_height = Some(4096);
                limits.max_alloc = Some(consts::MAX_IMAGE_ALLOC);
                reader.limits(limits);
                let image = reader.decode().map_err(|_| "media.failed")?;
                frames.push(Frame { image: preview(image)?, time: Some(packet.time) });
            },
        }

        Ok(frames)
    }

    fn flush(&mut self) -> Result<Vec<Frame>, &'static str>
    {
        let mut frames = Vec::new();

        match self
        {
            Self::Avc { decoder, .. } =>
            {
                for frame in decoder.flush_remaining().map_err(|_| "media.failed")?
                {
                    frames.push(Frame { image: avc_image(frame)?, time: None });
                }
            },
            Self::Hevc { decoder, .. } =>
            {
                while let Some(frame) = decoder.flush()
                {
                    if frames.len() >= 32 { return Err("media.unsupported"); }
                    frames.push(Frame { image: hevc_image(&frame)?, time: None });
                }
            },
            _ => {},
        }

        Ok(frames)
    }
}

impl Video
{
    pub(super) fn open(data: Arc<[u8]>, timed: bool) -> Result<Option<Self>, &'static str>
    {
        let Some((mut input, params)) = Input::open(data.clone())? else { return Ok(None) };
        let codec = Codec::open(&params)?;
        let mut times = Vec::new();

        //ONLY TIMESTAMPS ARE INDEXED; DECODED FRAMES ARE NEVER PRELOADED
        if timed
        {
            while let Some(packet) = input.next()?
            {
                if times.len() >= consts::MAX_MEDIA_SAMPLES as usize { return Err("media.unsupported"); }
                times.push(packet.time);
            }

            times.sort_unstable();
            input = Input::open(data)?.ok_or("media.failed")?.0;
        }

        Ok(Some(Self { input, codec, times }))
    }

    pub(super) fn thumbnail(mut self) -> Option<ImageFrame>
    {
        for _ in 0..consts::MEDIA_THUMBNAIL_SAMPLES
        {
            let Some(packet) = self.input.next().ok()? else { break };
            if let Some(frame) = self.codec.decode(&packet).ok()?.into_iter().next() { return Some(frame.image); }
        }

        self.codec.flush().ok()?.into_iter().next().map(|frame| frame.image)
    }

    pub(super) fn run(mut self, session: &Session, tx: &Sender<ClientEvent>) -> Result<(), &'static str>
    {
        let mut shown = 0;
        let first = self.times.first().copied().unwrap_or_default();

        while let Some(packet) = self.input.next()?
        {
            if !message::current(session.generation) { return Ok(()); }

            for frame in self.codec.decode(&packet)?
            {
                let time = frame.time.or_else(|| self.times.get(shown).copied()).unwrap_or(packet.time);
                media::present(frame.image, time.saturating_sub(first), session, tx)?;
                shown += 1;
            }
        }

        for frame in self.codec.flush()?
        {
            let time = self.times.get(shown).copied().unwrap_or(first);
            media::present(frame.image, time.saturating_sub(first), session, tx)?;
            shown += 1;
        }

        Ok(())
    }
}

impl Plane<'_>
{
    fn sample(&self, index: usize) -> Option<f32>
    {
        match self
        {
            Self::U8(data) => data.get(index).map(|value| *value as f32),
            Self::U16(data) => data.get(index).map(|value| *value as f32),
        }
    }
}


//FUNCTIONS
/// Recognizes ISO media/QuickTime atom headers and Matroska/WebM EBML headers.
pub(super) fn recognizes(data: &[u8]) -> bool
{
    data.starts_with(&[0x1a, 0x45, 0xdf, 0xa3])
        || matches!(data.get(4..8), Some(b"ftyp" | b"moov" | b"mdat" | b"wide" | b"free" | b"skip"))
}

//KEEP LARGE OPTIONAL HEVC CONFIGURATION SEI FROM HIDING ALL TRACKS
/// Symphonia limits hvcC to 1 KiB. x265 can put a much larger encoder-info SEI there.
/// Keep the parameter sets and replace the unused space with a free atom. No packet
/// offsets, file lengths, cached originals or hash identities change.
pub(super) fn prepare(mut data: Arc<[u8]>) -> Result<Arc<[u8]>, &'static str>
{
    if data.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) || !recognizes(&data) { return Ok(data); }
    let mut configurations = Vec::new();
    codec_atoms(&data, 0, data.len(), 0, &mut configurations)?;

    for (start, content, end) in configurations
    {
        if end - content <= 1024 { continue; }
        let config = &data[content..end];
        if config.len() < 23 || config[0] != 1 { return Err("media.failed"); }
        let mut reduced = config[..23].to_vec();
        reduced[22] = 0;
        let mut remaining = &config[23..];

        for _ in 0..config[22]
        {
            let original = remaining;
            let array = remaining.get(..3).ok_or("media.failed")?;
            let count = u16::from_be_bytes([array[1], array[2]]);
            let kind = array[0] & 63;
            remaining = &remaining[3..];
            for _ in 0..count { nal(&mut remaining, 2)?; }

            if matches!(kind, 32..=34)
            {
                reduced.extend_from_slice(&original[..original.len() - remaining.len()]);
                reduced[22] += 1;
            }
        }

        let size = 8 + reduced.len();
        let padding = (end - start).checked_sub(size).ok_or("media.failed")?;
        if reduced.len() > 1024 || padding < 8 { return Err("media.unsupported"); }
        let target = &mut Arc::make_mut(&mut data)[start..end];
        target.fill(0);
        target[..4].copy_from_slice(&(size as u32).to_be_bytes());
        target[4..8].copy_from_slice(b"hvcC");
        target[8..size].copy_from_slice(&reduced);
        target[size..size + 4].copy_from_slice(&(padding as u32).to_be_bytes());
        target[size + 4..size + 8].copy_from_slice(b"free");
    }

    Ok(data)
}

/// Visits only the bounded ISO atom paths that can contain video configuration records.
fn codec_atoms(data: &[u8], mut start: usize, end: usize, depth: usize,
    configurations: &mut Vec<(usize, usize, usize)>) -> Result<(), &'static str>
{
    if depth > 8 { return Err("media.failed"); }

    while start < end
    {
        //QUICKTIME SAMPLE ENTRIES MAY END WITH A FOUR-BYTE ZERO TERMINATOR
        if end - start == 4 && data.get(start..end) == Some(&[0, 0, 0, 0]) { break; }

        let header = data.get(start..start + 8).filter(|_| start + 8 <= end).ok_or("media.failed")?;
        let size = u32::from_be_bytes(header[..4].try_into().unwrap());
        let (size, header_size) = match size
        {
            0 => (end - start, 8),
            1 =>
            {
                let wide = data.get(start + 8..start + 16).ok_or("media.failed")?;
                (usize::try_from(u64::from_be_bytes(wide.try_into().unwrap())).map_err(|_| "media.failed")?, 16)
            },
            size => (size as usize, 8),
        };
        let next = start.checked_add(size).filter(|next| *next <= end).ok_or("media.failed")?;
        if size < header_size { return Err("media.failed"); }
        let content = start + header_size;

        let children = match &header[4..8]
        {
            b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl" => Some(content),
            b"stsd" => Some(content + 8),
            b"avc1" | b"avc3" | b"hev1" | b"hvc1" | b"dvh1" | b"dvhe" | b"vp09" => Some(content + 78),
            b"hvcC" => { configurations.push((start, content, next)); None },
            _ => None,
        };

        if let Some(children) = children
        {
            if children > next { return Err("media.failed"); }
            codec_atoms(data, children, next, depth + 1, configurations)?;
        }

        start = next;
    }

    Ok(())
}

//READ LENGTH-PREFIXED CODEC CONFIGURATION AND ACCESS UNITS
fn nal<'a>(data: &mut &'a [u8], length_size: usize) -> Result<&'a [u8], &'static str>
{
    let prefix = data.get(..length_size).ok_or("media.failed")?;
    let size = prefix.iter().fold(0usize, |size, byte| (size << 8) | *byte as usize);
    *data = &data[length_size..];
    let packet = data.get(..size).ok_or("media.failed")?;
    *data = &data[size..];

    Ok(packet)
}

fn annex_b(data: &[u8], length_size: usize) -> Result<Vec<u8>, &'static str>
{
    if length_size == 0 { return Ok(data.to_vec()); }
    let mut data = data;
    let mut output = Vec::with_capacity(data.len());

    while !data.is_empty()
    {
        output.extend_from_slice(&[0, 0, 0, 1]);
        output.extend_from_slice(nal(&mut data, length_size)?);
    }

    Ok(output)
}

fn avc_configuration(data: &[u8]) -> Result<(usize, Vec<u8>), &'static str>
{
    if data.first() != Some(&1) || data.len() < 7 { return Err("media.unsupported"); }
    let length_size = (data[4] & 3) as usize + 1;
    let mut remaining = &data[6..];
    let mut annex = Vec::new();

    for _ in 0..data[5] & 31
    {
        annex.extend_from_slice(&[0, 0, 0, 1]);
        annex.extend_from_slice(nal(&mut remaining, 2)?);
    }

    let count = *remaining.first().ok_or("media.failed")?;
    remaining = &remaining[1..];
    for _ in 0..count
    {
        annex.extend_from_slice(&[0, 0, 0, 1]);
        annex.extend_from_slice(nal(&mut remaining, 2)?);
    }

    Ok((length_size, annex))
}

fn hevc_configuration(data: &[u8]) -> Result<(usize, Vec<u8>), &'static str>
{
    if data.first() != Some(&1) || data.len() < 23 { return Err("media.unsupported"); }
    let length_size = (data[21] & 3) as usize + 1;
    let mut remaining = &data[23..];
    let mut annex = Vec::new();

    for _ in 0..data[22]
    {
        let header = remaining.get(..3).ok_or("media.failed")?;
        let count = u16::from_be_bytes([header[1], header[2]]);
        remaining = &remaining[3..];

        for _ in 0..count
        {
            annex.extend_from_slice(&[0, 0, 0, 1]);
            annex.extend_from_slice(nal(&mut remaining, 2)?);
        }
    }

    Ok((length_size, annex))
}

//VALIDATE VP9 SUPERFRAME GEOMETRY BEFORE THE DECODER ALLOCATES ITS REFERENCES
fn guard_vp9(data: &[u8], references: &mut [(u32, u32); 8]) -> Result<(), &'static str>
{
    let mut parts = Vec::new();
    let marker = *data.last().ok_or("media.failed")?;
    let count = (marker & 7) as usize + 1;
    let bytes = ((marker >> 3) & 3) as usize + 1;
    let index = 2 + count * bytes;
    let offset = data.len().checked_sub(index);

    if marker & 0xe0 == 0xc0 && offset.is_some_and(|offset| data[offset] == marker)
    {
        let offset = offset.unwrap();
        let mut sizes = &data[offset + 1..data.len() - 1];
        let mut content = &data[..offset];

        for _ in 0..count
        {
            let mut size = 0usize;
            for (shift, byte) in sizes[..bytes].iter().enumerate() { size |= (*byte as usize) << (shift * 8); }
            parts.push(content.get(..size).ok_or("media.failed")?);
            content = &content[size..];
            sizes = &sizes[bytes..];
        }
        if !content.is_empty() { return Err("media.failed"); }
    } else { parts.push(data); }

    for part in parts
    {
        let header = vp9_mt::parse_uncompressed_header(&mut BitReader::new(part), references).map_err(|_| "media.failed")?;
        if header.show_existing_frame { continue; }
        if header.width == 0 || header.height == 0
            || header.width as u64 * header.height as u64 > consts::MAX_MEDIA_PIXELS as u64
        {
            return Err("media.unsupported");
        }
        for (index, reference) in references.iter_mut().enumerate()
        {
            if header.key_frame || header.refresh_frame_flags & (1 << index) != 0 { *reference = (header.width, header.height); }
        }
    }

    Ok(())
}

//CONVERT DECODED PLANES TO THE EXISTING STILL/VIDEO PREVIEW TYPE
fn preview(image: DynamicImage) -> Result<ImageFrame, &'static str>
{
    if image.width() == 0 || image.height() == 0
        || image.width() as u64 * image.height() as u64 > consts::MAX_MEDIA_PIXELS as u64
    {
        return Err("media.unsupported");
    }
    let image = if image.width() > consts::MEDIA_PREVIEW_SIZE.0 || image.height() > consts::MEDIA_PREVIEW_SIZE.1
    {
        image.thumbnail(consts::MEDIA_PREVIEW_SIZE.0, consts::MEDIA_PREVIEW_SIZE.1)
    } else { image };

    Ok(ImageFrame { image, delay: Duration::ZERO })
}

fn avc_image(frame: DecodedYUV<'_>) -> Result<ImageFrame, &'static str>
{
    let (width, height) = frame.dimensions();
    if width == 0 || height == 0 || width * height > consts::MAX_MEDIA_PIXELS { return Err("media.unsupported"); }
    let mut rgb = vec![0; width * height * 3];
    frame.write_rgb8(&mut rgb);
    let image = RgbImage::from_raw(width as u32, height as u32, rgb).ok_or("media.failed")?;

    preview(DynamicImage::ImageRgb8(image))
}

fn plane(data: &PixelData) -> Plane<'_>
{
    match data
    {
        PixelData::U8(data) => Plane::U8(data),
        PixelData::U16(data) => Plane::U16(data),
    }
}

fn hevc_image(frame: &HevcFrame) -> Result<ImageFrame, &'static str>
{
    let subsampling = match frame.chroma_format_idc
    {
        1 => (1, 1),
        3 => (0, 0),
        _ => return Err("media.unsupported"),
    };
    let width = frame.width as usize;
    let chroma_width = width.div_ceil(1 << subsampling.0);

    yuv_image(frame.width, frame.height, [plane(&frame.y), plane(&frame.u), plane(&frame.v)],
        [width, chroma_width, chroma_width], subsampling, frame.bit_depth as u32, false,
        if frame.height >= 720 { 2 } else { 1 })
}

fn vp9_image(frame: &Picture) -> Result<ImageFrame, &'static str>
{
    let (y, ys) = frame.plane(0);
    let (u, us) = frame.plane(1);
    let (v, vs) = frame.plane(2);

    yuv_image(frame.width(), frame.height(), [Plane::U16(y), Plane::U16(u), Plane::U16(v)],
        [ys, us, vs], frame.subsampling(), frame.bit_depth(), frame.full_range(), frame.color_space())
}

fn yuv_image
(
    width: u32,
    height: u32,
    planes: [Plane<'_>; 3],
    strides: [usize; 3],
    subsampling: (u32, u32),
    depth: u32,
    full_range: bool,
    space: u32,
) -> Result<ImageFrame, &'static str>
{
    if width == 0 || height == 0 || width as u64 * height as u64 > consts::MAX_MEDIA_PIXELS as u64
        || !(8..=12).contains(&depth) || subsampling.0 > 1 || subsampling.1 > 1
    {
        return Err("media.unsupported");
    }

    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    let shift = (1u32 << (depth - 8)) as f32;
    let max = ((1u32 << depth) - 1) as f32;
    let (red, green, blue) = match space
    {
        2 => (1.5748, (-0.1873, -0.4681), 1.8556),
        5 => (1.4746, (-0.1646, -0.5714), 1.8814),
        _ => (1.402, (-0.3441, -0.7141), 1.772),
    };

    for row in 0..height as usize
    {
        for col in 0..width as usize
        {
            let y = planes[0].sample(row * strides[0] + col).ok_or("media.failed")?;
            let chroma_row = row >> subsampling.1;
            let chroma_col = col >> subsampling.0;
            let u = planes[1].sample(chroma_row * strides[1] + chroma_col).ok_or("media.failed")?;
            let v = planes[2].sample(chroma_row * strides[2] + chroma_col).ok_or("media.failed")?;
            let color = if space == 7 { [v / max, y / max, u / max] } else
            {
                let y = if full_range { y / max } else { (y - 16. * shift) / (219. * shift) };
                let range = if full_range { max } else { 224. * shift };
                let u = (u - 128. * shift) / range;
                let v = (v - 128. * shift) / range;
                [y + red * v, y + green.0 * u + green.1 * v, y + blue * u]
            };
            rgb.extend(color.map(|sample| (sample * 255.).round().clamp(0., 255.) as u8));
        }
    }

    let image = RgbImage::from_raw(width, height, rgb).ok_or("media.failed")?;
    preview(DynamicImage::ImageRgb8(image))
}
