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

//MODULES
#[cfg(feature = "server")]
pub mod password;
pub mod kex;

use why2::
{
    Grid,
    encrypter,
    decrypter,
    options as core_options,
    auth::AuthenticatedData,
};

use sha2::{ Sha256, Digest };

use crate::options;

//CONSTS
const GRID_W: usize = options::GRID_DIMENSIONS.0;
const GRID_H: usize = options::GRID_DIMENSIONS.1;

//FUNCTIONS
pub fn sha256(seed_str: &str) -> [u8; 32] //GET HASH SEED; USED FOR PADDING
{
    //SHA256
    let mut hasher = Sha256::new();
    hasher.update(seed_str.as_bytes());

    //FINALIZE
    hasher.finalize().into()
}

pub fn encrypt_packet(packet_bytes: Vec<u8>, keys: &options::SharedKeys) -> Vec<u8>
{
    //CONVERT packet_bytes to BINARY
    let mut input_i64 = Vec::with_capacity((packet_bytes.len() + 7) / 8);
    for chunk in packet_bytes.chunks(8)
    {
        let mut buf = [0u8; 8];
        buf[..chunk.len()].copy_from_slice(chunk);
        input_i64.push(i64::from_be_bytes(buf));
    }

    //ENCRYPT
    let encrypted_data = encrypter::encrypt::<GRID_W, GRID_H>(&input_i64, Some(&keys.0)).expect("Encrypting packet failed");

    //AUTHENTICATE
    AuthenticatedData::authenticate(encrypted_data, keys.1.as_slice().try_into().unwrap()).into()
}

pub fn decrypt_packet(mut decoded_packet: Vec<u8>, keys: &options::SharedKeys) -> Option<Vec<u8>>
{
    //DESERIALIZE
    let auth_packet: AuthenticatedData<GRID_W, GRID_H> = decoded_packet.as_slice().try_into().ok()?;

    //VERIFY HMAC
    if !auth_packet.verify(keys.1.as_slice().try_into().ok()?)
    {
        return None;
    }

    //DECRYPT
    let decrypted_packet = decrypter::decrypt(core_options::EncryptedData
    {
        output: auth_packet.encrypted_data.output,
        key: Grid::from_key(&keys.0).ok()?,
        nonce: auth_packet.encrypted_data.nonce,
    }).ok()?;

    //OVERWRITE decoded_packet
    decoded_packet = Vec::with_capacity(decrypted_packet.output.len() * 8);
    for val in decrypted_packet.output.to_vec()
    {
        decoded_packet.extend_from_slice(&val.to_be_bytes());
    }

    Some(decoded_packet)
}
