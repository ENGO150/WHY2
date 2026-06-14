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
    io::Cursor,
    convert::TryInto,
};

use crate::network::screen::
{
    consts,
    client::frame::{ CompressedFrame, Frame },
};

use image::
{
    ImageDecoder,
    ColorType,
    codecs::jpeg::{ JpegEncoder, JpegDecoder },
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

pub fn compress_jpeg(width: u32, height: u32, rgb_data: &[u8]) -> CompressedFrame
{
    let mut out = Vec::with_capacity(rgb_data.len() / 10);
    out.push(1);
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());

    let mut enc = JpegEncoder::new_with_quality(&mut out, 80);
    enc.encode(rgb_data, width, height, ColorType::Rgb8.into()).expect("JPEG encode failed");

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

        let cursor = Cursor::new(&compressed.compressed_data[9..]);
        let dec = JpegDecoder::new(cursor).expect("JPEG decode failed");
        let mut rgb = vec![0u8; (w * h * 3) as usize];
        ImageDecoder::read_image(dec, &mut rgb).expect("JPEG read failed");

        let mut data = Vec::with_capacity((w * h) as usize);
        for chunk in rgb.chunks_exact(3)
        {
            let r = chunk[0] as u32;
            let g = chunk[1] as u32;
            let b = chunk[2] as u32;
            data.push(0xFF000000 | (r << 16) | (g << 8) | b);
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
