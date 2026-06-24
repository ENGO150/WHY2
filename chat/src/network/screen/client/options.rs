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

use std::sync::atomic::{ AtomicBool, Ordering };

//OPTIONS
static USE_SCREEN: AtomicBool = AtomicBool::new(false);
static ATTACH_SCREEN: AtomicBool = AtomicBool::new(false);

//USE SCREEN
pub fn get_use_screen() -> bool
{
    USE_SCREEN.load(Ordering::Relaxed)
}

pub fn swap_use_screen() -> bool
{
    !USE_SCREEN.fetch_xor(true, Ordering::Relaxed)
}

//ATTACH SCREEN
pub fn get_attach_screen() -> bool
{
    ATTACH_SCREEN.load(Ordering::Relaxed)
}

pub fn set_attach_screen(attach: bool)
{
    ATTACH_SCREEN.store(attach, Ordering::Relaxed)
}
