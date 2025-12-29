/*
This is part of WHY2
Copyright (C) 2022-2025 Václav Šmejkal

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

//CONSTS (I HIGHLY RECOMMEND NOT CHANGING THOSE)
pub const SAMPLE_RATE: u32  = 48000;                                    //put some text here
pub const FRAME_MS: u32     = 20;                                       //LENGTH OF ONE FRAME
pub const FRAME_SIZE: usize = (SAMPLE_RATE * FRAME_MS / 1000) as usize; //960 SAMPLES PER FRAME
