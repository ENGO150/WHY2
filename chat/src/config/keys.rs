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

//! # Server Identity Pinning (TOFU)
//!
//! The client remembers the fingerprint of every server identity it has accepted, keyed by
//! host, in `server_keys.toml`. The first connection to a host asks the user; every later one
//! must present the same identity.

use std::fmt::Write;

use crate::
{
    consts,
    crypto,
};

//ENUMS
/// The outcome of checking a server identity against the pinned one.
pub enum TofuCode //POSSIBLE KEY VERIFICATION RESULTS
{
    /// The identity matches the pinned fingerprint.
    Valid, //KEY MATCHES LOCAL CONFIG
    /// No identity is pinned for this host: `(fingerprint, host)`.
    Unknown(String, String), //KEY NOT FOUND IN CONFIG
    /// The identity differs from the pinned fingerprint.
    Mismatch, //KEY DIFFERS
}

/// Returns the fingerprint of a server identity: the lowercase hex SHA-256 of its public key.
pub fn hash(pubkey: &[u8]) -> String //HASH SERVER IDENTITY KEY
{
    //HASH PUBKEY
    let pubkey_hash = crypto::sha256(pubkey);
    let mut pubkey_string = String::with_capacity(64);

    //SERIALIZE
    for byte in pubkey_hash
    {
        write!(pubkey_string, "{:02x}", byte).unwrap();
    }

    pubkey_string
}

/// Checks a server identity against the one pinned for `host`.
///
/// # Parameters
/// - `host`: The address the client dialed.
/// - `pubkey`: The server's identity public key.
pub fn check(host: &str, pubkey: &[u8]) -> TofuCode //CHECK PUBKEY VALIDITY (TOFU)
{
    let pubkey_string = hash(pubkey);

    //PEER PUBKEY STORED, CHECK VALIDITY
    if super::with_cached(&super::config_path(consts::SERVER_KEYS_CONFIG), |keys| keys.get(host).is_some())
    {
        //COMPARE
        return if super::config_read::<String>(consts::SERVER_KEYS_CONFIG, host) == pubkey_string
        {
            TofuCode::Valid
        } else
        {
            TofuCode::Mismatch
        }
    }

    TofuCode::Unknown(pubkey_string, host.to_string())
}

/// Returns the fingerprint pinned for `host`, if any.
pub fn pinned(host: &str) -> Option<String> //THE FINGERPRINT CURRENTLY PINNED FOR host, IF ANY
{
    if super::with_cached(&super::config_path(consts::SERVER_KEYS_CONFIG), |keys| keys.get(host).is_none()) { return None; }

    Some(super::config_read::<String>(consts::SERVER_KEYS_CONFIG, host))
}

/// Pins a fingerprint for `host`, replacing any previous one.
pub fn save(host: &str, pubkey_hash: &str) //SAVE KEY
{
    //WRITE
    super::config_write(consts::SERVER_KEYS_CONFIG, host, pubkey_hash);
}
