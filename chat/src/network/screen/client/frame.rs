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

//STRUCTS
pub struct Frame //SINGLE FRAME
{
    pub width: u32,
    pub height: u32,
    pub data: Vec<u32>, //PIXEL DATA (0RGB)
}

pub struct CompressedFrame //COMPRESSED FRAME
{
    pub width: u32,
    pub height: u32,
    pub compressed_data: Vec<u8>,
    pub pixel_count: usize,
}

impl Frame
{
    pub fn as_bytes(&self) -> &[u8]
    {
        unsafe
        {
            std::slice::from_raw_parts(self.data.as_ptr() as *const u8, self.data.len() * 4)
        }
    }

    //RECONSTRUCT A FRAME
    pub fn from_bytes(width: u32, height: u32, bytes: &[u8]) -> Self
    {
        let data: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
            .collect();

        Self
        {
            width,
            height,
            data,
        }
    }
}
