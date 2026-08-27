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

use std::fmt::Write;

use crate::
{
    consts,
    crypto,
};

//ENUMS
pub enum TofuCode //POSSIBLE KEY VERIFICATION RESULTS
{
    Valid, //KEY MATCHES LOCAL CONFIG
    Unknown(String, String), //KEY NOT FOUND IN CONFIG
    Mismatch, //KEY DIFFERS
}

pub fn hash(pubkey: &str) -> String //HASH SERVER KEYS
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

pub fn check(host: &str, pubkey: &str) -> TofuCode //CHECK PUBKEY VALIDITY (TOFU)
{
    let pubkey_string = hash(pubkey);

    //PEER PUBKEY STORED, CHECK VALIDITY
    if super::get_data(&super::config_path(consts::SERVER_KEYS_CONFIG)).get(host).is_some()
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

pub fn pinned(host: &str) -> Option<String> //THE FINGERPRINT CURRENTLY PINNED FOR host, IF ANY
{
    if super::get_data(&super::config_path(consts::SERVER_KEYS_CONFIG)).get(host).is_none() { return None; }

    Some(super::config_read::<String>(consts::SERVER_KEYS_CONFIG, host))
}

pub fn save(host: &str, pubkey_hash: &str) //SAVE KEY
{
    //WRITE
    super::config_write(consts::SERVER_KEYS_CONFIG, host, pubkey_hash);
}
