// This is part of WHY2
// Copyright (C) 2022-2026 Václav Šmejkal
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

// RGBA8 -> I420 (BT.601, LIMITED RANGE).
//
// THE COEFFICIENTS AND THE ROUNDING BELOW ARE openh264's OWN, COPIED FROM `formats::rgb2yuv` SO
// THAT A STREAM LOOKS THE SAME WHICHEVER CONVERTER PRODUCED IT.
//
// LAYOUT: BOTH PLANES ARE PACKED FOUR SAMPLES TO A WORD. ONE INVOCATION WRITES EITHER ONE LUMA
// WORD (FOUR HORIZONTALLY ADJACENT PIXELS) OR ONE CHROMA WORD OF *BOTH* U AND V - DOING U AND V
// TOGETHER HALVES THE SOURCE READS, SINCE THEY SHARE THE SAME 2x2 AVERAGE.

struct Dimensions
{
    width: u32,
    height: u32,
    y_words: u32,
    c_words: u32,
};

@group(0) @binding(0) var<uniform> dimensions: Dimensions;
@group(0) @binding(1) var<storage, read> source: array<u32>;
@group(0) @binding(2) var<storage, read_write> planes: array<u32>;

fn red(pixel: u32) -> i32   { return i32(pixel & 0xffu); }
fn green(pixel: u32) -> i32 { return i32((pixel >> 8u) & 0xffu); }
fn blue(pixel: u32) -> i32  { return i32((pixel >> 16u) & 0xffu); }

fn luma(pixel: u32) -> u32
{
    let value = ((66 * red(pixel) + 129 * green(pixel) + 25 * blue(pixel)) >> 8u) + 16;

    return u32(clamp(value, 0, 255));
}

@compute @workgroup_size(64)
fn convert(@builtin(global_invocation_id) id: vec3<u32>)
{
    let index = id.x;

    // LUMA: FOUR PIXELS OF ONE ROW, WHICH IS WHY THE WIDTH MUST BE A MULTIPLE OF FOUR
    if (index < dimensions.y_words)
    {
        let base = index * 4u;

        var word: u32 = 0u;

        for (var offset: u32 = 0u; offset < 4u; offset = offset + 1u)
        {
            word = word | (luma(source[base + offset]) << (8u * offset));
        }

        planes[index] = word;

        return;
    }

    let chroma_index = index - dimensions.y_words;

    if (chroma_index >= dimensions.c_words)
    {
        return;
    }

    // CHROMA: FOUR 2x2 BLOCKS OF ONE CHROMA ROW, SO THE CHROMA WIDTH MUST ALSO BE A MULTIPLE OF
    // FOUR - HENCE THE WIDTH % 8 == 0 GUARD ON THE RUST SIDE
    let chroma_width = dimensions.width / 2u;

    let first = chroma_index * 4u;
    let chroma_y = first / chroma_width;

    var u_word: u32 = 0u;
    var v_word: u32 = 0u;

    for (var offset: u32 = 0u; offset < 4u; offset = offset + 1u)
    {
        let chroma_x = (first + offset) % chroma_width;

        let top = (chroma_y * 2u) * dimensions.width + chroma_x * 2u;
        let bottom = top + dimensions.width;

        let a = source[top];
        let b = source[top + 1u];
        let c = source[bottom];
        let d = source[bottom + 1u];

        // 2x2 BOX AVERAGE WITH THE SAME +2 ROUNDING THE CPU PATH USES
        let r = (red(a) + red(b) + red(c) + red(d) + 2) / 4;
        let g = (green(a) + green(b) + green(c) + green(d) + 2) / 4;
        let bl = (blue(a) + blue(b) + blue(c) + blue(d) + 2) / 4;

        let u = clamp(((-38 * r - 74 * g + 112 * bl) >> 8u) + 128, 0, 255);
        let v = clamp(((112 * r - 94 * g - 18 * bl) >> 8u) + 128, 0, 255);

        u_word = u_word | (u32(u) << (8u * offset));
        v_word = v_word | (u32(v) << (8u * offset));
    }

    planes[dimensions.y_words + chroma_index] = u_word;
    planes[dimensions.y_words + dimensions.c_words + chroma_index] = v_word;
}
