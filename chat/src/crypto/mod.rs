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

use why2::stream::RexStream;

use crate::consts::SharedKeys;

//PRIVATE
fn get_correct_key<const W: usize, const H: usize>(key: &Zeroizing<Vec<i64>>) -> Zeroizing<Vec<i64>> //DERIVE VALID KEYDIM USING HKDF
{
    //CONVERT KEY TO BYTES
    let mut key_bytes = Zeroizing::new(Vec::with_capacity(key.len() * 8));
    for val in key.iter()
    {
        key_bytes.extend_from_slice(&val.to_be_bytes());
    }

    //INIT HKDF
    let hkdf = Hkdf::<Sha256>::new(None, &key_bytes);

    let required_len = W * H * 2;
    let needed_bytes = required_len * 8;
    let mut output_bytes = Zeroizing::new(vec![0u8; needed_bytes]);

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

fn derive_stream_nonce<const W: usize, const H: usize>(context: &[u8]) -> Zeroizing<Vec<i64>>
{
    let hkdf = Hkdf::<Sha256>::new(None, context);

    let mut okm = Zeroizing::new(vec![0u8; W * H * 8]);
    hkdf.expand(b"WHY2-STREAM-NONCE", &mut okm).expect("HKDF expand failed");

    Zeroizing::new(okm.chunks_exact(8).map(|c| i64::from_be_bytes(c.try_into().unwrap())).collect())
}

//CRATE PUBLIC
pub(crate) fn bytes_to_i64(bytes: &[u8]) -> Vec<i64>
{
    bytes.chunks(8).map(|chunk|
    {
        let mut buf = [0u8; 8];
        buf[..chunk.len()].copy_from_slice(chunk);
        i64::from_be_bytes(buf)
    }).collect()
}

pub(crate) fn i64_to_bytes(vals: &[i64]) -> Vec<u8>
{
    vals.iter().flat_map(|v| v.to_be_bytes()).collect()
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

pub fn encrypt_packet<const W: usize, const H: usize>(packet_bytes: &[u8], keys: &SharedKeys) -> Vec<u8>
{
    //CONVERT packet_bytes to BINARY
    let input_i64 = Zeroizing::new(bytes_to_i64(&packet_bytes));

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

pub fn decrypt_packet<const W: usize, const H: usize>(decoded_packet: Vec<u8>, keys: &SharedKeys) -> Option<Zeroizing<Vec<u8>>>
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

    Some(Zeroizing::new(i64_to_bytes(&decrypted_packet.output)))
}

pub fn init_rex_stream<const W: usize, const H: usize>(keys: &SharedKeys, token: &[u8; 32]) -> Option<RexStream<W, H>>
{
    let key = if keys.0.len() == W * H * 2 { keys.0.clone() } else { get_correct_key::<W, H>(&keys.0) };
    let key_grid = Grid::from_key(&key).ok()?;

    let derived = derive_stream_nonce::<W, H>(token);

    //RECONSRUCT NONCE GRID
    let nonce_grid = Grid::from_flat(&derived).ok()?;

    //INIT & RETURN
    RexStream::new(&key_grid, nonce_grid).ok()
}
