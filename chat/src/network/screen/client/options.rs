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

//! # Screen Share State
//!
//! Whether we are sharing or watching, what we share, and how loud a watched share is played.
//!
//! The monitor pick lasts exactly as long as the share: every path that ends a share resets it,
//! so a plain `/screen` always starts on the default monitor.

use std::sync::
{
    LazyLock,
    RwLock,
    atomic::
    {
        AtomicBool,
        AtomicUsize,
        Ordering,
    },
};

use crate::
{
    config,
    network::voice::client::options as voice_options,
};

//OPTIONS
/// Whether we are sharing.
static USE_SCREEN: AtomicBool = AtomicBool::new(false);
/// Whether we are watching a share.
static ATTACH_SCREEN: AtomicBool = AtomicBool::new(false);
/// Whether our share includes audio.
static SHARE_AUDIO: AtomicBool = AtomicBool::new(true);

//WHICH MONITOR /screen SHARES, BY NAME
/// Which monitor is shared, by name; `None` is the default.
static MONITOR: RwLock<Option<String>> = RwLock::new(None);

//BUMPED WHENEVER THE PICK CHANGES
/// Bumped whenever the pick changes; capture loops watch it to switch monitors mid-share.
static MONITOR_GENERATION: AtomicUsize = AtomicUsize::new(0);

//HOW LOUD AN ATTACHED SHARE IS PLAYED, IN PERCENT
/// Playback volume of a watched share, in percent.
static SCREEN_VOLUME: LazyLock<AtomicUsize> = LazyLock::new(|| AtomicUsize::new(voice_options::clamp_volume(config::read_config::<u32>("screen_volume")) as usize));

//USE SCREEN
/// Returns whether we are sharing.
pub fn get_use_screen() -> bool
{
    USE_SCREEN.load(Ordering::Relaxed)
}

/// Sets whether we are sharing; `false` ends the share.
pub fn set_use_screen(use_screen: bool)
{
    USE_SCREEN.store(use_screen, Ordering::Relaxed)
}

//ATTACH SCREEN
/// Returns whether we are watching a share.
pub fn get_attach_screen() -> bool
{
    ATTACH_SCREEN.load(Ordering::Relaxed)
}

/// Sets whether we are watching a share; `false` ends the attachment.
pub fn set_attach_screen(attach: bool)
{
    ATTACH_SCREEN.store(attach, Ordering::Relaxed)
}

//SHARE AUDIO
/// Returns whether our share includes audio.
pub fn get_share_audio() -> bool
{
    SHARE_AUDIO.load(Ordering::Relaxed)
}

/// Sets whether our share includes audio; takes effect mid-share.
pub fn set_share_audio(share_audio: bool)
{
    SHARE_AUDIO.store(share_audio, Ordering::Relaxed)
}

//MONITOR
/// Returns the picked monitor.
pub fn get_monitor() -> Option<String>
{
    MONITOR.read().unwrap().clone()
}

/// Picks a monitor; picking a different one mid-share switches the capture over.
pub fn set_monitor(monitor: Option<String>)
{
    let mut current = MONITOR.write().unwrap();

    if *current == monitor { return; } //THE SAME MONITOR IS NOT A SWAP

    *current = monitor;

    MONITOR_GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Returns the monitor generation.
pub fn monitor_generation() -> usize
{
    MONITOR_GENERATION.load(Ordering::Relaxed)
}

//SCREEN VOLUME
/// Returns the playback volume of a watched share, in percent.
pub fn get_screen_volume() -> u32 //ATTACHED SHARE PLAYBACK VOLUME (PERCENT)
{
    SCREEN_VOLUME.load(Ordering::Relaxed) as u32
}

/// Sets the playback volume of a watched share, in percent.
pub fn set_screen_volume(percent: u32)
{
    SCREEN_VOLUME.store(voice_options::clamp_volume(percent) as usize, Ordering::Relaxed);
}

/// Returns the playback volume of a watched share as a multiplier.
pub fn get_screen_gain() -> f32 //THE SAME, AS A MULTIPLIER
{
    get_screen_volume() as f32 / 100.
}
