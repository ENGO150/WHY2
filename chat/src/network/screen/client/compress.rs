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
    slice,
    convert::TryInto,
};

use crate::network::
{
    CompressedFrame,
    screen::
    {
        consts,
        client::frame::Frame,
    },
};

use turbojpeg::
{
    Compressor,
    Decompressor,
    Subsamp,
    Image,
    PixelFormat,
};

use zstd::bulk;

//ENUMS
pub enum DecompressedFrame
{
    ZstdDiff(Frame),
    JpegFull(Frame),
}

//FUNCTIONS
pub fn compress_zstd(frame: &Frame) -> CompressedFrame
{
    let mut out = Vec::with_capacity(frame.data.len() + 1);
    out.push(0);

    let raw_bytes = frame.as_bytes();
    let compressed_data = bulk::compress(raw_bytes, consts::ZSTD_LEVEL).expect("zstd compression failed");
    out.extend_from_slice(&compressed_data);

    CompressedFrame
    {
        width: frame.width,
        height: frame.height,
        compressed_data: out,
        pixel_count: frame.data.len(),
    }
}

pub fn compress_jpeg(width: u32, height: u32, rgba_data: &[u8]) -> CompressedFrame
{
    let mut out = Vec::with_capacity(rgba_data.len() / 10);
    out.push(1);
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());

    let mut comp = Compressor::new().expect("Failed to init turbojpeg compressor");
    comp.set_quality(80).ok();
    comp.set_subsamp(Subsamp::Sub2x2).ok();

    let image = Image
    {
        pixels: rgba_data,
        width: width as usize,
        pitch: width as usize * 4,
        height: height as usize,
        format: PixelFormat::RGBA,
    };
    let compressed_jpeg = comp.compress_to_owned(image).expect("JPEG encode failed");
    out.extend_from_slice(&compressed_jpeg);

    CompressedFrame
    {
        width,
        height,
        compressed_data: out,
        pixel_count: (width * height) as usize,
    }
}

pub fn decompress(compressed: &CompressedFrame) -> DecompressedFrame
{
    if compressed.compressed_data[0] == 1
    {
        let w = u32::from_le_bytes(compressed.compressed_data[1..5].try_into().unwrap());
        let h = u32::from_le_bytes(compressed.compressed_data[5..9].try_into().unwrap());

        let mut decomp = Decompressor::new().expect("Failed to init turbojpeg decompressor");
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        let image = Image
        {
            pixels: rgba.as_mut_slice(),
            width: w as usize,
            pitch: w as usize * 4,
            height: h as usize,
            format: PixelFormat::RGBA,
        };
        decomp.decompress(&compressed.compressed_data[9..], image).expect("JPEG decode failed");

        let mut data = Vec::with_capacity((w * h) as usize);
        let rgba_u32 = unsafe { slice::from_raw_parts(rgba.as_ptr() as *const u32, rgba.len() / 4) };
        for &pixel in rgba_u32.iter()
        {
            data.push(0xFF000000 | (pixel.to_be() >> 8));
        }

        DecompressedFrame::JpegFull(Frame { width: w, height: h, data })
    } else
    {
        let max_decompressed_size = compressed.pixel_count * 4;
        let bytes = bulk::decompress(&compressed.compressed_data[1..], max_decompressed_size)
            .expect("zstd decompression failed");

        DecompressedFrame::ZstdDiff(Frame::from_bytes(compressed.width, compressed.height, &bytes))
    }
}
