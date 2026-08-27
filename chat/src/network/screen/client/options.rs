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

use std::sync::
{
    RwLock,
    atomic::{ AtomicBool, AtomicUsize, Ordering },
};

//OPTIONS
static USE_SCREEN: AtomicBool = AtomicBool::new(false);
static ATTACH_SCREEN: AtomicBool = AtomicBool::new(false);

//WHICH MONITOR /screen SHARES, BY NAME. THE CHOICE NEVER LEAVES THIS MACHINE (THE SERVER ONLY EVER
//TOGGLES THE SHARE) AND IT IS NOT REMEMBERED PAST THE SHARE THAT ASKED FOR IT: EVERY PATH THAT ENDS
//A SHARE PUTS IT BACK TO `None`, WHICH IS THE DEFAULT (PRIMARY) MONITOR.
static MONITOR: RwLock<Option<String>> = RwLock::new(None);

//BUMPED WHENEVER THE PICK ACTUALLY CHANGES. A RUNNING CAPTURE WATCHES IT AND STARTS OVER ON THE NEW
//MONITOR, WHICH IS WHAT MAKES `/screen OTHER` A SWAP RATHER THAN THE END OF THE SHARE.
static MONITOR_GENERATION: AtomicUsize = AtomicUsize::new(0);

//USE SCREEN
pub fn get_use_screen() -> bool
{
    USE_SCREEN.load(Ordering::Relaxed)
}

pub fn set_use_screen(use_screen: bool)
{
    USE_SCREEN.store(use_screen, Ordering::Relaxed)
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

//MONITOR
pub fn get_monitor() -> Option<String>
{
    MONITOR.read().unwrap().clone()
}

pub fn set_monitor(monitor: Option<String>)
{
    let mut current = MONITOR.write().unwrap();

    if *current == monitor { return; } //THE SAME MONITOR IS NOT A SWAP - NOBODY HAS TO START OVER FOR IT

    *current = monitor;

    MONITOR_GENERATION.fetch_add(1, Ordering::Relaxed);
}

pub fn monitor_generation() -> usize
{
    MONITOR_GENERATION.load(Ordering::Relaxed)
}
