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
pub mod backends;

use std::sync::{ Arc, Mutex };

use backends::scrap as scrap_backend;

#[cfg(all(target_os = "linux", feature = "wayland"))]
use std::env;

#[cfg(all(target_os = "linux", feature = "wayland"))]
use backends::wayland as wayland_backend;

//STRUCTS
pub struct CaptureInfo
{
    pub width:  usize, //SCREEN WIDTH
    pub height: usize, //SCREEN HEIGHT
    pub stride: usize, //STRIDE
}

//TYPES
pub type SharedFrame = Arc<Mutex<(Vec<u8>, bool)>>;

//FUNCTIONS
pub fn compress(raw: &[u8]) -> Vec<u8>
{
    zstd::encode_all(raw, 1).unwrap_or_else(|_| Vec::new()) //IGNORE ERRORS
}

pub fn decompress(compressed: &[u8], out: &mut Vec<u8>)
{
    *out = zstd::decode_all(compressed).unwrap_or_else(|_| Vec::new()); //IGNORE ERRORS
}

pub fn start(monitor_idx: usize) -> (CaptureInfo, SharedFrame)
{
    //TRY WAYLAND ON LINUX FIRST
    #[cfg(all(target_os = "linux", feature = "wayland"))]
    if env::var("WAYLAND_DISPLAY").is_ok()
    {
        return wayland_backend::start(monitor_idx);
    }

    //FALLBACK TO SCRAP
    scrap_backend::start(monitor_idx)
}
