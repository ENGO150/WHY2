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

use std::sync::atomic::
{
    AtomicBool,
    AtomicUsize,
    Ordering,
};

//OPTIONS
static SEQ: AtomicUsize = AtomicUsize::new(0); //PACKET SEQUENCE NUMBER (CLIENT -> SERVER)

static SERVER_SEQ: AtomicUsize = AtomicUsize::new(0); //PACKET SEQUENCE NUMBER (SERVER -> CLIENT)

static USE_VOICE: AtomicBool = AtomicBool::new(false);

//SEQ
pub fn get_seq() -> usize //GET SEQUENCE NUMBER
{
    SEQ.load(Ordering::Relaxed)
}

pub fn set_seq(value: usize) //SET SEQUENCE NUMBER
{
    SEQ.store(value, Ordering::Relaxed)
}

//SERVER SEQ
pub fn get_server_seq() -> usize //GET SERVER SEQUENCE NUMBER
{
    SERVER_SEQ.load(Ordering::Relaxed)
}

pub fn set_server_seq(value: usize) //SET SERVER SEQUENCE NUMBER
{
    SERVER_SEQ.store(value, Ordering::Relaxed)
}

//USE VOICE
pub fn get_use_voice() -> bool //GET USE VOICE
{
    USE_VOICE.load(Ordering::Relaxed)
}

pub fn swap_use_voice() -> bool //SET USE VOICE
{
    !USE_VOICE.fetch_xor(true, Ordering::Relaxed)
}
