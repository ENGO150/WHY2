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

use std::sync::{ LazyLock, RwLock };

#[cfg(feature = "client")]
use std::sync::
{
    Arc,
    Mutex,
    atomic::
    {
        AtomicBool,
        AtomicUsize,
        Ordering,
    },
};

//CONSTS
pub const METADATA_URL: &str        = "https://crates.io/api/v1/crates/why2";                                 //URL FOR PROJECT METADATA

pub const USER_CONFIG_DIR: &str     = "{HOME}/.config";                                                       //USER CONFIG DIRECTORY
pub const CONFIG_DIR: &str          = "/WHY2";                                                                //DIRECTORY FOR CONFIG FILES
pub const SERVER_CONFIG: &str       = "/server.toml";                                                         //SERVER CONFIG FILE
pub const CLIENT_CONFIG: &str       = "/client.toml";                                                         //CLIENT CONFIG FILE
pub const SERVER_USERS_CONFIG: &str = "/server_users.toml";                                                   //SERVER USERS CONFIG FILE
pub const CONFIG_URL: &str          = "https://git.satan.red/ENGO150/WHY2/-/raw/development/chat/src/config"; //CONFIG FILE DOWNLOAD URL

pub const SERVER_KEYS_CONFIG: &str  = "/server_keys.toml";                                                    //SERVER PUBKEY CONFIG FILE
pub const SERVER_KEYS_DIR: &str     = "/server_keys";                                                         //SERVER KEYS DIRECTORY
pub const SERVER_SKEY: &str         = "/private";                                                             //SERVER PRIVATE KEY FILE
pub const SERVER_PKEY: &str         = "/public";                                                              //SERVER PUBLIC KEY FILE

pub const FETCH_TIMEOUT: u64        = 5000;                                                                   //TIMOUT FOR FETCHING DATA (MS)

//DO NOT CHANGE CONST BELOW UNLESS YOU ARE ABSOLUTELY SURE WHAT ARE YOU DOING
pub const GRID_DIMENSIONS: (usize, usize) = (8, 8);                                                           //DIMENSIONS OF REX GRID

//SETTINGS
static SHARED_KEY: LazyLock<RwLock<Option<Vec<i64>>>> = LazyLock::new(|| //SHARED SYMMETRIC KEY
{
    RwLock::new(None)
});

#[cfg(feature = "client")]
static ASKING_PASSWORD: AtomicBool = AtomicBool::new(false); //CLIENT IS SENDING PASSWORD (DISABLE ECHO)

#[cfg(feature = "client")]
static EXTRA_SPACE: AtomicBool = AtomicBool::new(false); //CLIENT DISPLAYED SOME MENU (/help ETC.), ADD EXTRA SPACE ON NEXT MESSAGE

#[cfg(feature = "client")]
static SENDING_MESSAGES: AtomicBool = AtomicBool::new(false); //SENDING MESSAGES BOOL (CONDITION FOR ADDING MESSAGES TO HISTORY)

#[cfg(feature = "client")]
pub static INPUT_READ: LazyLock<Arc<Mutex<Vec<char>>>> = LazyLock::new(|| //INPUT READ FROM CLIENT
{
    Arc::new(Mutex::new(Vec::new()))
});

#[cfg(feature = "client")]
static SEQ: AtomicUsize = AtomicUsize::new(0); //PACKET SEQUENCE NUMBER

//FUNCTIONS
//SHARED SYM KEY
pub fn set_shared_key(key: Vec<i64>) //SET KEY
{
    let mut shared_key = SHARED_KEY.write().unwrap();
    *shared_key = Some(key);
}

pub fn get_shared_key() -> Option<Vec<i64>> //RETURN KEY
{
    let shared_key = SHARED_KEY.read().unwrap();
    shared_key.clone()
}

//ASKING PASSWORD
#[cfg(feature = "client")]
pub fn set_asking_password(value: bool) //SET ASKING_PASSWORD
{
    ASKING_PASSWORD.store(value, Ordering::SeqCst);
}

#[cfg(feature = "client")]
pub fn get_asking_password() -> bool //GET ASKING_PASSWORD
{
    ASKING_PASSWORD.load(Ordering::SeqCst)
}

//ADD EXTRA SPACE
#[cfg(feature = "client")]
pub fn set_extra_space(value: bool) //SET EXTRA_SPACE
{
    EXTRA_SPACE.store(value, Ordering::SeqCst);
}

#[cfg(feature = "client")]
pub fn get_extra_space() -> bool //GET EXTRA_SPACE
{
    EXTRA_SPACE.load(Ordering::SeqCst)
}

//SENDING MESSAGES
#[cfg(feature = "client")]
pub fn get_sending_messages() -> bool //GET SENDING_MESSAGES
{
    SENDING_MESSAGES.load(Ordering::SeqCst)
}

#[cfg(feature = "client")]
pub fn set_sending_messages(value: bool) //SET SENDING_MESSAGES
{
    SENDING_MESSAGES.store(value, Ordering::SeqCst);
}

#[cfg(feature = "client")]
pub fn get_seq() -> usize //GET SEQUENCE NUMBER
{
    SEQ.load(Ordering::Relaxed)
}

#[cfg(feature = "client")]
pub fn set_seq(value: usize) //SET SEQUENCE NUMBER
{
    SEQ.store(value, Ordering::Relaxed)
}
