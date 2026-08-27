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

// I420 -> RGB (BT.601, LIMITED RANGE), PLUS THE LETTERBOX.
//
// THE DECODER HANDS US THREE SINGLE-CHANNEL PLANES. UPLOADING THEM AS THEY ARE COSTS 1.5 BYTES PER
// PIXEL; CONVERTING ON THE CPU FIRST, AS THE `pixels` PATH DID, COST 4 - AND A FULL-FRAME SCALAR
// PASS ON TOP. THE SAMPLER DOES THE CHROMA UPSCALE AND THE SCALE-TO-WINDOW FOR FREE.

struct Geometry
{
    scale: vec2<f32>,  // LETTERBOX: <=1 ON THE AXIS THAT HAS TO SHRINK
    luma: vec2<f32>,   // x = SPAN, y = OFFSET, BOTH IN NORMALISED TEXTURE COORDINATES
    chroma: vec2<f32>, // AS ABOVE, FOR THE HALF-WIDTH PLANES
};

@group(0) @binding(0) var<uniform> geometry: Geometry;
@group(0) @binding(1) var luma: texture_2d<f32>;
@group(0) @binding(2) var chroma_u: texture_2d<f32>;
@group(0) @binding(3) var chroma_v: texture_2d<f32>;
@group(0) @binding(4) var frame_sampler: sampler;

struct Vertex
{
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// FULLSCREEN TRIANGLE - NO VERTEX BUFFER, NO INDEX BUFFER
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> Vertex
{
    var out: Vertex;

    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));

    out.uv = uv;
    out.position = vec4<f32>((uv * 2.0 - 1.0) * vec2<f32>(1.0, -1.0) * geometry.scale, 0.0, 1.0);

    return out;
}

@fragment
fn fragment(in: Vertex) -> @location(0) vec4<f32>
{
    // THE LETTERBOX IS THE PART OF THE TRIANGLE THAT IS NOT THE PICTURE, AND IT HAS TO BE PAINTED
    // BLACK HERE RATHER THAN LEFT TO THE CLEAR. THE FULLSCREEN *TRIANGLE* SPANS uv 0..2, WHICH IS
    // WHY IT COVERS THE WHOLE UNIT SQUARE - BUT SCALED DOWN IT NO LONGER OVERHANGS THE VIEWPORT,
    // SO ITS TWO EXTRA CORNERS LAND INSIDE THE WINDOW AND SAMPLE uv > 1, WHERE A CLAMPED SAMPLER
    // REPEATS THE LAST ROW/COLUMN. THAT IS THE TRIANGULAR SMEAR IN THE BARS: NOT A CLEAR THAT WAS
    // MISSED, BUT THE PICTURE BEING DRAWN OVER IT.
    if (in.uv.x > 1.0 || in.uv.y > 1.0)
    {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    // THE PLANES ARE ALLOCATED AT THEIR *STRIDE* WIDTH, SO THE PADDING PAST EACH ROW IS TRIMMED
    // HERE RATHER THAN BY REPACKING EVERY PLANE ON THE CPU FIRST.
    //
    // THE SPAN REACHES THE *CENTRE* OF THE LAST REAL TEXEL, NOT ITS TRAILING EDGE. MAPPING u=1 TO
    // width/stride LANDS EXACTLY ON THE BOUNDARY, WHERE A LINEAR SAMPLER MIXES IN HALF A TEXEL OF
    // PADDING - WHICH IS A VISIBLE SMEAR DOWN THE RIGHT-HAND COLUMN, NOT A ROUNDING DETAIL.
    let luma_uv = vec2<f32>(geometry.luma.y + in.uv.x * geometry.luma.x, in.uv.y);
    let chroma_uv = vec2<f32>(geometry.chroma.y + in.uv.x * geometry.chroma.x, in.uv.y);

    let y = textureSample(luma, frame_sampler, luma_uv).r;
    let u = textureSample(chroma_u, frame_sampler, chroma_uv).r;
    let v = textureSample(chroma_v, frame_sampler, chroma_uv).r;

    // LIMITED RANGE: LUMA LIVES IN 16..235, CHROMA IN 16..240
    let c = (y - 16.0 / 255.0) * (255.0 / 219.0);
    let d = (u - 128.0 / 255.0) * (255.0 / 224.0);
    let e = (v - 128.0 / 255.0) * (255.0 / 224.0);

    let rgb = vec3<f32>
    (
        c + 1.402 * e,
        c - 0.344136 * d - 0.714136 * e,
        c + 1.772 * d,
    );

    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
