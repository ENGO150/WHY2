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
    atomic::{ AtomicBool, Ordering },
};

#[cfg(feature = "client_base")]
use std::
{
    sync::
    {
        Mutex,
        LazyLock,
        atomic::AtomicUsize,
    },
};

#[cfg(feature = "client_voice")]
use std::collections::HashSet;

#[cfg(feature = "client_base")]
use crate::consts::SharedKeys;

//SETTINGS
static SERVER_USERNAME: RwLock<String> = RwLock::new(String::new());

#[cfg(feature = "server")]
static VOICE_CHAT: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "client_base")]
static KEYS: LazyLock<RwLock<Option<SharedKeys>>> = LazyLock::new(|| //SHARED SYMMETRIC KEY
{
    RwLock::new(None)
});

#[cfg(feature = "client_base")]
static ASKING_PASSWORD: AtomicBool = AtomicBool::new(false); //CLIENT IS SENDING PASSWORD (DISABLE ECHO)

#[cfg(feature = "client_base")]
static EXTRA_SPACE: AtomicBool = AtomicBool::new(false); //CLIENT DISPLAYED SOME MENU (/help ETC.), ADD EXTRA SPACE ON NEXT MESSAGE

#[cfg(feature = "client_base")]
static SENDING_MESSAGES: AtomicBool = AtomicBool::new(false); //SENDING MESSAGES BOOL (CONDITION FOR ADDING MESSAGES TO HISTORY)

#[cfg(feature = "client_base")]
static SEQ: AtomicUsize = AtomicUsize::new(0); //PACKET SEQUENCE NUMBER (CLIENT -> SERVER)

#[cfg(feature = "client_base")]
static SERVER_SEQ: AtomicUsize = AtomicUsize::new(0); //PACKET SEQUENCE NUMBER (SERVER -> CLIENT)

#[cfg(feature = "client_base")]
static SERVER_ADDRESS: RwLock<String> = RwLock::new(String::new());

#[cfg(feature = "client_base")]
static OBFUSCATION_KEY: RwLock<[u8; 32]> = RwLock::new([0; 32]);

#[cfg(feature = "client_base")]
static CHANNEL: LazyLock<RwLock<String>> = LazyLock::new(|| RwLock::new(String::new())); //ACTIVE CHANNEL

#[cfg(feature = "client_voice")]
static MUTE: AtomicBool = AtomicBool::new(false); //MUTE LOCAL CLIENT
#[cfg(feature = "client_voice")]
static MUTED: LazyLock<Mutex<HashSet<usize>>> = LazyLock::new(|| Mutex::new(HashSet::new())); //MUTED CLIENTS

#[cfg(feature = "client_base")]
static SOCKS5: AtomicBool = AtomicBool::new(false); //USE SOCKS5 (TOR)

//FUNCTIONS
//SERVER USERNAME
pub fn get_server_username() -> String //GET SERVER USERNAME
{
    SERVER_USERNAME.read().unwrap().to_owned()
}

pub fn set_server_username(username: &str) //SET SERVER USERNAME
{
    *SERVER_USERNAME.write().unwrap() = username.to_owned();
}

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
#[cfg(feature = "client_base")]
pub fn set_keys(keys: SharedKeys) //SET KEY
{
    let mut shared_key = KEYS.write().unwrap();
    *shared_key = Some(keys);
}

#[cfg(feature = "client_base")]
pub fn get_keys() -> Option<SharedKeys> //RETURN KEY
{
    let shared_key = KEYS.read().unwrap();
    shared_key.clone()
}

//ASKING PASSWORD
#[cfg(feature = "client_base")]
pub fn set_asking_password(value: bool) //SET ASKING_PASSWORD
{
    ASKING_PASSWORD.store(value, Ordering::Relaxed);
}

#[cfg(feature = "client_base")]
pub fn get_asking_password() -> bool //GET ASKING_PASSWORD
{
    ASKING_PASSWORD.load(Ordering::Relaxed)
}

//ADD EXTRA SPACE
#[cfg(feature = "client_base")]
pub fn set_extra_space(value: bool) //SET EXTRA_SPACE
{
    EXTRA_SPACE.store(value, Ordering::Relaxed);
}

#[cfg(feature = "client_base")]
pub fn get_extra_space() -> bool //GET EXTRA_SPACE
{
    EXTRA_SPACE.load(Ordering::Relaxed)
}

//SENDING MESSAGES
#[cfg(feature = "client_base")]
pub fn get_sending_messages() -> bool //GET SENDING_MESSAGES
{
    SENDING_MESSAGES.load(Ordering::Relaxed)
}

#[cfg(feature = "client_base")]
pub fn set_sending_messages(value: bool) //SET SENDING_MESSAGES
{
    SENDING_MESSAGES.store(value, Ordering::Relaxed);
}

#[cfg(feature = "client_base")]
pub fn get_seq() -> usize //GET SEQUENCE NUMBER
{
    SEQ.load(Ordering::Relaxed)
}

#[cfg(feature = "client_base")]
pub fn set_seq(value: usize) //SET SEQUENCE NUMBER
{
    SEQ.store(value, Ordering::Relaxed)
}

#[cfg(feature = "client_base")]
pub fn get_server_seq() -> usize //GET SERVER SEQUENCE NUMBER
{
    SERVER_SEQ.load(Ordering::Relaxed)
}

#[cfg(feature = "client_base")]
pub fn set_server_seq(value: usize) //SET SERVER SEQUENCE NUMBER
{
    SERVER_SEQ.store(value, Ordering::Relaxed)
}

//SERVER ADDRESS
#[cfg(feature = "client_base")]
pub fn get_server_address() -> String //GET SERVER ADDRESS
{
    SERVER_ADDRESS.read().unwrap().to_owned()
}

#[cfg(feature = "client_base")]
pub fn set_server_address(address: &str) //SET SERVER ADDRESS
{
    *SERVER_ADDRESS.write().unwrap() = address.to_owned();
}

//OBFUSCATION KEY
#[cfg(feature = "client_base")]
pub fn get_obfuscation_key() -> [u8; 32] //GET OBFUSCATION KEY
{
    *OBFUSCATION_KEY.read().unwrap()
}

#[cfg(feature = "client_base")]
pub fn set_obfuscation_key(key: &[u8; 32]) //SET OBFUSCATION KEY
{
    *OBFUSCATION_KEY.write().unwrap() = *key;
}

//CHANNEL
#[cfg(feature = "client_base")]
pub fn get_channel() -> String //GET CHANNEL
{
    CHANNEL.read().unwrap().clone()
}

#[cfg(feature = "client_base")]
pub fn set_channel(channel: String) //SET CHANNEL
{
    *CHANNEL.write().unwrap() = channel;
}

//MUTING
#[cfg(feature = "client_voice")]
pub fn toggle_mute(id: Option<usize>) -> bool //ENABLE/DISABLE MUTE
{
    if let Some(id) = id //MUTE CLIENT
    {
        let mut muted = MUTED.lock().unwrap();

        if muted.remove(&id)
        {
            false
        } else
        {
            muted.insert(id);
            true
        }

    } else //MUTE LOCAL CLIENT
    {
        !MUTE.fetch_xor(true, Ordering::Relaxed)
    }
}

#[cfg(feature = "client_voice")]
pub fn is_muted(id: Option<usize>) -> bool //CHECK IF CLIENT IS MUTED
{
    if let Some(id) = id
    {
        MUTED.lock().unwrap().contains(&id)
    } else
    {
        MUTE.load(Ordering::Relaxed)
    }
}

//SOCKS5
#[cfg(feature = "client_base")]
pub fn enable_socks5() //SET SOCKS5 TO TRUE
{
    SOCKS5.store(true, Ordering::Relaxed);
}

#[cfg(feature = "client_base")]
pub fn socks5_enabled() -> bool //GET SOCKS5
{
    SOCKS5.load(Ordering::Relaxed)
}
