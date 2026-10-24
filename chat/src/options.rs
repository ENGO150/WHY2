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

//! # Process-Wide Session State
//!
//! Global state of the running session, kept in atomics and locks so that tasks, audio
//! callbacks and the UI can read it without threading it through every call.
//!
//! Client state is reset when a session is lost, so nothing here outlives a connection.

use std::sync::
{
    RwLock,
    atomic::{ AtomicBool, Ordering },
};

#[cfg(feature = "client_base")]
use std::sync::atomic::AtomicUsize;

#[cfg(feature = "client_voice")]
use std::
{
    sync::Mutex,
    collections::BTreeSet,
};

#[cfg(feature = "client_base")]
use crate::consts::SharedKeys;

//ENUMS
/// Which login prompt the server last asked the client for.
#[cfg(feature = "client_base")]
#[derive(Copy, Clone)]
pub enum LoginState //STATE OF LOGIN
{
    None,
    Username,
    Login,
    Register,
}

//SETTINGS
/// Name the server signs its own messages with.
static SERVER_USERNAME: RwLock<String> = RwLock::new(String::new());

/// Whether the server runs the UDP voice listener.
#[cfg(feature = "server")]
static VOICE_CHAT: AtomicBool = AtomicBool::new(false);

/// Current session keys.
#[cfg(feature = "client_base")]
static KEYS: RwLock<Option<SharedKeys>> = RwLock::new(None); //SHARED SYMMETRIC KEY

/// Whether the client is answering a password prompt.
#[cfg(feature = "client_base")]
static ASKING_PASSWORD: AtomicBool = AtomicBool::new(false); //CLIENT IS SENDING PASSWORD (DISABLE ECHO)

/// Current login prompt.
#[cfg(feature = "client_base")]
static LOGIN_STATE: RwLock<LoginState> = RwLock::new(LoginState::None);

/// Whether the client is past login and sending chat messages.
#[cfg(feature = "client_base")]
static SENDING_MESSAGES: AtomicBool = AtomicBool::new(false); //SENDING MESSAGES BOOL

/// Sequence number of the next packet to the server.
#[cfg(feature = "client_base")]
static SEQ: AtomicUsize = AtomicUsize::new(0); //PACKET SEQUENCE NUMBER (CLIENT -> SERVER)

/// Sequence number expected on the next packet from the server.
#[cfg(feature = "client_base")]
static SERVER_SEQ: AtomicUsize = AtomicUsize::new(0); //PACKET SEQUENCE NUMBER (SERVER -> CLIENT)

/// Address of the connected server.
#[cfg(feature = "client_base")]
static SERVER_ADDRESS: RwLock<String> = RwLock::new(String::new());

/// Key of the non-cryptographic XOR obfuscation layer.
#[cfg(feature = "client_base")]
static OBFUSCATION_KEY: RwLock<[u8; 32]> = RwLock::new([0; 32]);

/// Fingerprint of the connected server; scopes the image cache.
#[cfg(feature = "client_base")]
static SERVER_FINGERPRINT: RwLock<String> = RwLock::new(String::new()); //WHAT THE IMAGE CACHE IS SCOPED BY

/// Channel the client is in; empty for the lobby.
#[cfg(feature = "client_base")]
static CHANNEL: RwLock<String> = RwLock::new(String::new()); //ACTIVE CHANNEL

/// Whether the local microphone is muted.
#[cfg(feature = "client_voice")]
static MUTE: AtomicBool = AtomicBool::new(false); //MUTE LOCAL CLIENT
/// Session ids of peers muted locally.
#[cfg(feature = "client_voice")]
static MUTED: Mutex<BTreeSet<usize>> = Mutex::new(BTreeSet::new()); //MUTED CLIENTS

/// Whether connections go through a SOCKS5 proxy.
#[cfg(feature = "client_base")]
static SOCKS5: AtomicBool = AtomicBool::new(false); //USE SOCKS5 (TOR)

//FUNCTIONS
//SERVER USERNAME
/// Returns the name the server signs its own messages with.
pub fn get_server_username() -> String //GET SERVER USERNAME
{
    SERVER_USERNAME.read().unwrap().to_owned()
}

/// Sets the name the server signs its own messages with.
pub fn set_server_username(username: &str) //SET SERVER USERNAME
{
    *SERVER_USERNAME.write().unwrap() = username.to_owned();
}

//VOICE CHAT
/// Marks the voice listener as running.
#[cfg(feature = "server")]
pub fn enable_voice_chat() //SET VOICE CHAT TO TRUE
{
    VOICE_CHAT.store(true, Ordering::Relaxed);
}

/// Returns whether the voice listener is running.
#[cfg(feature = "server")]
pub fn voice_chat_enabled() -> bool //GET VOICE CHAT
{
    VOICE_CHAT.load(Ordering::Relaxed)
}

//SHARED KEYS
/// Stores the current session keys.
#[cfg(feature = "client_base")]
pub fn set_keys(keys: SharedKeys) //SET KEY
{
    let mut shared_key = KEYS.write().unwrap();
    *shared_key = Some(keys);
}

/// Returns a copy of the current session keys, if a session exists.
#[cfg(feature = "client_base")]
pub fn get_keys() -> Option<SharedKeys> //RETURN KEY
{
    let shared_key = KEYS.read().unwrap();
    shared_key.clone()
}

//ASKING PASSWORD
/// Sets whether the client is answering a password prompt.
#[cfg(feature = "client_base")]
pub fn set_asking_password(value: bool) //SET ASKING_PASSWORD
{
    ASKING_PASSWORD.store(value, Ordering::Relaxed);
}

/// Returns whether the client is answering a password prompt.
#[cfg(feature = "client_base")]
pub fn get_asking_password() -> bool //GET ASKING_PASSWORD
{
    ASKING_PASSWORD.load(Ordering::Relaxed)
}

//LOGIN STATE
/// Sets the current login prompt.
#[cfg(feature = "client_base")]
pub fn set_login_state(state: LoginState)
{
    *LOGIN_STATE.write().unwrap() = state;
}

/// Returns the current login prompt.
#[cfg(feature = "client_base")]
pub fn get_login_state() -> LoginState
{
    *LOGIN_STATE.read().unwrap()
}

//SENDING MESSAGES
/// Returns whether the client is past login.
#[cfg(feature = "client_base")]
pub fn get_sending_messages() -> bool //GET SENDING_MESSAGES
{
    SENDING_MESSAGES.load(Ordering::Relaxed)
}

/// Sets whether the client is past login.
#[cfg(feature = "client_base")]
pub fn set_sending_messages(value: bool) //SET SENDING_MESSAGES
{
    SENDING_MESSAGES.store(value, Ordering::Relaxed);
}

/// Returns the sequence number of the next packet to the server.
#[cfg(feature = "client_base")]
pub fn get_seq() -> usize //GET SEQUENCE NUMBER
{
    SEQ.load(Ordering::Relaxed)
}

/// Sets the sequence number of the next packet to the server.
#[cfg(feature = "client_base")]
pub fn set_seq(value: usize) //SET SEQUENCE NUMBER
{
    SEQ.store(value, Ordering::Relaxed)
}

/// Returns the sequence number expected from the server.
#[cfg(feature = "client_base")]
pub fn get_server_seq() -> usize //GET SERVER SEQUENCE NUMBER
{
    SERVER_SEQ.load(Ordering::Relaxed)
}

/// Sets the sequence number expected from the server.
#[cfg(feature = "client_base")]
pub fn set_server_seq(value: usize) //SET SERVER SEQUENCE NUMBER
{
    SERVER_SEQ.store(value, Ordering::Relaxed)
}

//SERVER ADDRESS
/// Returns the address of the connected server.
#[cfg(feature = "client_base")]
pub fn get_server_address() -> String //GET SERVER ADDRESS
{
    SERVER_ADDRESS.read().unwrap().to_owned()
}

/// Sets the address of the connected server.
#[cfg(feature = "client_base")]
pub fn set_server_address(address: &str) //SET SERVER ADDRESS
{
    *SERVER_ADDRESS.write().unwrap() = address.to_owned();
}

//OBFUSCATION KEY
/// Returns the obfuscation key.
#[cfg(feature = "client_base")]
pub fn get_obfuscation_key() -> [u8; 32] //GET OBFUSCATION KEY
{
    *OBFUSCATION_KEY.read().unwrap()
}

/// Sets the obfuscation key.
#[cfg(feature = "client_base")]
pub fn set_obfuscation_key(key: &[u8; 32]) //SET OBFUSCATION KEY
{
    *OBFUSCATION_KEY.write().unwrap() = *key;
}

//SERVER FINGERPRINT
/// Returns the fingerprint of the connected server.
#[cfg(feature = "client_base")]
pub fn get_fingerprint() -> String //GET SERVER FINGERPRINT
{
    SERVER_FINGERPRINT.read().unwrap().clone()
}

/// Sets the fingerprint of the connected server.
#[cfg(feature = "client_base")]
pub fn set_fingerprint(fingerprint: &str) //SET SERVER FINGERPRINT
{
    *SERVER_FINGERPRINT.write().unwrap() = fingerprint.to_owned();
}

//CHANNEL
/// Returns the channel the client is in; empty for the lobby.
#[cfg(feature = "client_base")]
pub fn get_channel() -> String //GET CHANNEL
{
    CHANNEL.read().unwrap().clone()
}

/// Sets the channel the client is in.
#[cfg(feature = "client_base")]
pub fn set_channel(channel: String) //SET CHANNEL
{
    *CHANNEL.write().unwrap() = channel;
}

//MUTING
/// Toggles a mute.
///
/// # Parameters
/// - `id`: A peer's session id, or `None` for the local microphone.
///
/// # Returns
/// `true` if the target is now muted.
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

/// Returns whether a target is muted.
///
/// # Parameters
/// - `id`: A peer's session id, or `None` for the local microphone.
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
/// Routes connections through SOCKS5.
#[cfg(feature = "client_base")]
pub fn enable_socks5() //SET SOCKS5 TO TRUE
{
    SOCKS5.store(true, Ordering::Relaxed);
}

/// Returns whether connections go through SOCKS5.
#[cfg(feature = "client_base")]
pub fn socks5_enabled() -> bool //GET SOCKS5
{
    SOCKS5.load(Ordering::Relaxed)
}
