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
    encrypter,
    decrypter,
    grid::Grid,
    types::EncryptedData,
    auth::AuthenticatedData,
};

use zeroize::Zeroizing;

use hkdf::Hkdf;
use sha2::{ Sha256, Digest };

use crate::options;

//PRIVATE
pub fn get_correct_key<const W: usize, const H: usize>(key: &Zeroizing<Vec<i64>>) -> Zeroizing<Vec<i64>> //DERIVE VALID KEYDIM USING HKDF
{
    //CONVERT KEY TO BYTES
    let mut key_bytes = Vec::with_capacity(key.len() * 8);
    for val in key.iter()
    {
        key_bytes.extend_from_slice(&val.to_be_bytes());
    }

    //INIT HKDF
    let hkdf = Hkdf::<Sha256>::new(None, &key_bytes);

    let required_len = W * H * 2;
    let needed_bytes = required_len * 8;
    let mut output_bytes = vec![0u8; needed_bytes];

    //EXPAND
    hkdf.expand(format!("WHY2-DERIVED-KEY-{W}x{H}").as_bytes(), &mut output_bytes).expect("Key derivation failed");

    //CONVERT BACK TO i64
    let mut derived_key = Vec::with_capacity(required_len);
    for chunk in output_bytes.chunks_exact(8)
    {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(chunk);
        derived_key.push(i64::from_be_bytes(buf));
    }

    Zeroizing::new(derived_key)
}

//PUBLIC
pub fn sha256(seed_str: &str) -> [u8; 32] //GET HASH SEED; USED FOR PADDING
{
    //SHA256
    let mut hasher = Sha256::new();
    hasher.update(seed_str.as_bytes());

    //FINALIZE
    hasher.finalize().into()
}

pub fn encrypt_packet<const W: usize, const H: usize>(packet_bytes: Vec<u8>, keys: &options::SharedKeys) -> Vec<u8>
{
    //CONVERT packet_bytes to BINARY
    let mut input_i64 = Vec::with_capacity((packet_bytes.len() + 7) / 8);
    for chunk in packet_bytes.chunks(8)
    {
        let mut buf = [0u8; 8];
        buf[..chunk.len()].copy_from_slice(chunk);
        input_i64.push(i64::from_be_bytes(buf));
    }

    //GET VALID KEY
    let key = if keys.0.len() == W * H * 2
    {
        &keys.0
    } else
    {
        &get_correct_key::<W, H>(&keys.0)
    };

    //ENCRYPT
    let encrypted_data = encrypter::encrypt::<W, H>(&input_i64, Some(key))
        .expect("Encrypting packet failed");

    //AUTHENTICATE
    AuthenticatedData::authenticate(encrypted_data, keys.1.as_slice().try_into().unwrap()).into()
}

pub fn decrypt_packet<const W: usize, const H: usize>(mut decoded_packet: Vec<u8>, keys: &options::SharedKeys) -> Option<Vec<u8>>
{
    //DESERIALIZE
    let auth_packet: AuthenticatedData<W, H> = decoded_packet.as_slice().try_into().ok()?;

    //VERIFY HMAC
    if !auth_packet.verify(keys.1.as_slice().try_into().ok()?)
    {
        return None;
    }

    //GET VALID KEY
    let key = if keys.0.len() == W * H * 2
    {
        &keys.0
    } else
    {
        &get_correct_key::<W, H>(&keys.0)
    };

    //DECRYPT
    let decrypted_packet = decrypter::decrypt(EncryptedData
    {
        output: auth_packet.encrypted_data.output,
        key: Grid::from_key(&key).ok()?,
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
