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

use zeroize::Zeroizing;

use std::sync::atomic::{ AtomicBool, Ordering };

#[cfg(feature = "client")]
use std::sync::
{
    Arc,
    Mutex,
    RwLock,
    LazyLock,
    OnceLock,
    atomic::AtomicUsize,
};

//CONSTS
pub const METADATA_URL: &str        = "https://crates.io/api/v1/crates/why2";                                 //URL FOR PROJECT METADATA

pub const USER_CONFIG_DIR: &str     = "{HOME}/.config";                                                       //USER CONFIG DIRECTORY
pub const CONFIG_DIR: &str          = "/WHY2";                                                                //DIRECTORY FOR CONFIG FILES
pub const SERVER_CONFIG: &str       = "/server.toml";                                                         //SERVER CONFIG FILE
pub const CLIENT_CONFIG: &str       = "/client.toml";                                                         //CLIENT CONFIG FILE
pub const SERVER_USERS_CONFIG: &str = "/server_users.toml";                                                   //SERVER USERS CONFIG FILE

pub const SERVER_KEYS_CONFIG: &str  = "/server_keys.toml";                                                    //SERVER PUBKEY CONFIG FILE
pub const SERVER_KEYS_DIR: &str     = "/server_keys";                                                         //SERVER KEYS DIRECTORY
pub const SERVER_SKEY: &str         = "/private";                                                             //SERVER PRIVATE KEY FILE
pub const SERVER_PKEY: &str         = "/public";                                                              //SERVER PUBLIC KEY FILE
pub const SERVER_PQ_SKEY: &str      = "/private_pq";                                                          //SERVER POST-QUANTUM PRIVATE KEY FILE
pub const SERVER_PQ_PKEY: &str      = "/public_pq";                                                           //SERVER POST-QUANTUM PUBLIC KEY FILE

pub const FETCH_TIMEOUT: u64        = 5000;                                                                   //TIMOUT FOR FETCHING DATA (MS)

pub const REKEY_INTERVAL: u64       = 600;                                                                    //INTERVAL FOR RE-REKEYING (SECS)

//DO NOT CHANGE CONST BELOW UNLESS YOU ARE ABSOLUTELY SURE WHAT ARE YOU DOING
pub const OBFUSCATION_KEY: &[u8; 32]      =         //KEY FOR OBFUSCATING NON-ENCRYPTED PACKETS (NOT A SECURITY FEATURE)
&[
    0x9A, 0xF7, 0x1C, 0xD4, 0x62, 0x3E, 0x8B, 0x5A,
    0x0F, 0x2D, 0xE1, 0x79, 0x4C, 0xB8, 0x63, 0x90,
    0x21, 0x55, 0xAE, 0xD6, 0x04, 0x7F, 0x33, 0x82,
    0xBC, 0x19, 0x40, 0xE7, 0x95, 0x2A, 0x6B, 0xF8,
];

//TYPES
pub type SharedKeys = (Zeroizing<Vec<i64>>, Zeroizing<Vec<u8>>);

//SETTINGS
#[cfg(feature = "server")]
static VOICE_CHAT: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "client")]
static KEYS: LazyLock<RwLock<Option<SharedKeys>>> = LazyLock::new(|| //SHARED SYMMETRIC KEY
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
static SEQ: AtomicUsize = AtomicUsize::new(0); //PACKET SEQUENCE NUMBER (CLIENT -> SERVER)

#[cfg(feature = "client")]
static SERVER_SEQ: AtomicUsize = AtomicUsize::new(0); //PACKET SEQUENCE NUMBER (SERVER -> CLIENT)

#[cfg(feature = "client")]
static SERVER_ADDRESS: OnceLock<String> = OnceLock::new();

#[cfg(feature = "client")]
static SOCKS5: AtomicBool = AtomicBool::new(false); //USE SOCKS5 (TOR)

//FUNCTIONS
//VOICE CHAT
#[cfg(feature = "server")]
pub fn enable_voice_chat() //SET VOICE CHAT TO TRUE
{
    VOICE_CHAT.store(true, Ordering::Relaxed);
}

#[cfg(feature = "server")]
pub fn voice_chat_enabled() -> bool //GET VOICE CHAT
{
    VOICE_CHAT.load(Ordering::Relaxed)
}

//SHARED KEYS
#[cfg(feature = "client")]
pub fn set_keys(keys: SharedKeys) //SET KEY
{
    let mut shared_key = KEYS.write().unwrap();
    *shared_key = Some(keys);
}

#[cfg(feature = "client")]
pub fn get_keys() -> Option<SharedKeys> //RETURN KEY
{
    let shared_key = KEYS.read().unwrap();
    shared_key.clone()
}

//ASKING PASSWORD
#[cfg(feature = "client")]
pub fn set_asking_password(value: bool) //SET ASKING_PASSWORD
{
    ASKING_PASSWORD.store(value, Ordering::Relaxed);
}

#[cfg(feature = "client")]
pub fn get_asking_password() -> bool //GET ASKING_PASSWORD
{
    ASKING_PASSWORD.load(Ordering::Relaxed)
}

//ADD EXTRA SPACE
#[cfg(feature = "client")]
pub fn set_extra_space(value: bool) //SET EXTRA_SPACE
{
    EXTRA_SPACE.store(value, Ordering::Relaxed);
}

#[cfg(feature = "client")]
pub fn get_extra_space() -> bool //GET EXTRA_SPACE
{
    EXTRA_SPACE.load(Ordering::Relaxed)
}

//SENDING MESSAGES
#[cfg(feature = "client")]
pub fn get_sending_messages() -> bool //GET SENDING_MESSAGES
{
    SENDING_MESSAGES.load(Ordering::Relaxed)
}

#[cfg(feature = "client")]
pub fn set_sending_messages(value: bool) //SET SENDING_MESSAGES
{
    SENDING_MESSAGES.store(value, Ordering::Relaxed);
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

#[cfg(feature = "client")]
pub fn get_server_seq() -> usize //GET SERVER SEQUENCE NUMBER
{
    SERVER_SEQ.load(Ordering::Relaxed)
}

#[cfg(feature = "client")]
pub fn set_server_seq(value: usize) //SET SERVER SEQUENCE NUMBER
{
    SERVER_SEQ.store(value, Ordering::Relaxed)
}

//SERVER ADDRESS
#[cfg(feature = "client")]
pub fn get_server_address() -> String //GET SERVER ADDRESS
{
    SERVER_ADDRESS.get().unwrap().to_owned()
}

#[cfg(feature = "client")]
pub fn set_server_address(address: &str) //SET SERVER ADDRESS
{
    SERVER_ADDRESS.set(address.to_owned()).unwrap();
}

//SOCKS5
#[cfg(feature = "client")]
pub fn enable_socks5() //SET SOCKS5 TO TRUE
{
    SOCKS5.store(true, Ordering::Relaxed);
}

#[cfg(feature = "client")]
pub fn socks5_enabled() -> bool //GET SOCKS5
{
    SOCKS5.load(Ordering::Relaxed)
}
