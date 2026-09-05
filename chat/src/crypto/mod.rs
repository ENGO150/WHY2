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

use hmac::
{
    Mac,
    Hmac,
    KeyInit,
};

use why2::
{
    consts,
    stream::RexStream,
};

use crate::consts::SharedKeys;

//STRUCTS
pub struct RexPacketStream //AUTHENTICATED STREAM CIPHER (ENCRYPT-THEN-MAC OVER A RexStream)
{
    stream: RexStream,             //CTR MODE STREAM
    mac_key: Zeroizing<[u8; 32]>,  //HMAC KEY (STREAM SPECIFIC)
    counter: u64,                  //PACKET COUNTER (BOUND INTO EVERY TAG)
}

//IMPLEMENTATIONS
impl RexPacketStream
{
    //PRIVATE
    fn mac(&self, ciphertext: &[u8]) -> Hmac<Sha256> //PRIME MAC OVER counter || len || ciphertext
    {
        let mut mac = <Hmac<Sha256>>::new_from_slice(self.mac_key.as_ref()).expect("Invalid MAC key");

        mac.update(&self.counter.to_be_bytes());
        mac.update(&(ciphertext.len() as u64).to_be_bytes());
        mac.update(ciphertext);

        mac
    }

    //PUBLIC
    pub fn seal(&mut self, plaintext: &[u8]) -> Vec<u8> //ENCRYPT AND AUTHENTICATE ([TAG][CIPHERTEXT])
    {
        //CONVERT PLAINTEXT TO i64
        let input_i64 = Zeroizing::new(bytes_to_i64(plaintext));

        //ENCRYPT
        let mut encrypted_i64 = Zeroizing::new(self.stream.update(&input_i64).expect("Stream encryption failed"));

        //FLUSH
        encrypted_i64.extend(self.stream.finalize().expect("Stream finalize failed"));

        //CONVERT ENCRYPTED BYTES BACK TO u8
        let mut ciphertext = i64_to_bytes(&encrypted_i64);

        //TRUNCATE PADDING
        ciphertext.truncate(plaintext.len());

        //AUTHENTICATE
        let tag = self.mac(&ciphertext).finalize().into_bytes();
        self.counter += 1;

        //[TAG][CIPHERTEXT]
        let mut output = Vec::with_capacity(tag.len() + ciphertext.len());
        output.extend_from_slice(&tag);
        output.append(&mut ciphertext);

        output
    }

    pub fn open(&mut self, data: &[u8]) -> Option<Zeroizing<Vec<u8>>> //VERIFY AND DECRYPT
    {
        //SPLIT OFF THE TAG
        if data.len() < 32 { return None; }
        let (tag, ciphertext) = data.split_at(32);

        //VERIFY BEFORE DECRYPTING (CONSTANT TIME); STREAM STAYS UNTOUCHED ON FAILURE
        self.mac(ciphertext).verify_slice(tag).ok()?;
        self.counter += 1;

        //CONVERT u8 TO i64
        let input_i64 = Zeroizing::new(bytes_to_i64(ciphertext));

        //DECRYPT
        let mut decrypted_i64 = Zeroizing::new(self.stream.update(&input_i64).ok()?);

        //FLUSH
        decrypted_i64.extend(self.stream.finalize().ok()?);

        //CONVERT BACK TO u8
        let mut output = Zeroizing::new(i64_to_bytes(&decrypted_i64));

        //TRUNCATE PADDING
        output.truncate(ciphertext.len());

        Some(output)
    }
}

//FUNCTIONS
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

fn derive_stream_nonce(context: &[u8]) -> Zeroizing<Vec<i64>>
{
    let hkdf = Hkdf::<Sha256>::new(None, context);

    let mut okm = Zeroizing::new(vec![0u8; consts::DEFAULT_GRID_WIDTH * consts::DEFAULT_GRID_HEIGHT * 8]);
    hkdf.expand(b"WHY2-STREAM-NONCE", &mut okm).expect("HKDF expand failed");

    Zeroizing::new(okm.chunks_exact(8).map(|c| i64::from_be_bytes(c.try_into().unwrap())).collect())
}

fn derive_stream_mac_key(hmac_key: &[u8], context: &[u8]) -> Zeroizing<[u8; 32]> //STREAM SPECIFIC MAC KEY
{
    let hkdf = Hkdf::<Sha256>::new(Some(context), hmac_key);

    let mut okm = Zeroizing::new([0u8; 32]);
    hkdf.expand(b"WHY2-STREAM-MAC", okm.as_mut()).expect("HKDF expand failed");

    okm
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
pub fn sha256(data: &[u8]) -> [u8; 32] //SHA-256 OF A BYTE STRING
{
    //SHA256
    let mut hasher = Sha256::new();
    hasher.update(data);

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

pub fn init_rex_stream(keys: &SharedKeys, token: &[u8; 32]) -> Option<RexPacketStream>
{
    let key_grid = Grid::from_key(&keys.0).ok()?;

    let derived = derive_stream_nonce(token);

    //RECONSRUCT NONCE GRID
    let nonce_grid = Grid::from_flat(&derived).ok()?;

    //INIT & RETURN
    Some(RexPacketStream
    {
        stream: RexStream::new(&key_grid, nonce_grid).ok()?,
        mac_key: derive_stream_mac_key(&keys.1, token),
        counter: 0,
    })
}

#[cfg(feature = "server")]
pub fn image_keys(hash: &[u8; 32]) -> (Zeroizing<Vec<i64>>, Vec<i64>) //AT-REST KEY & NONCE FOR ONE IMAGE
{
    let hkdf = Hkdf::<Sha256>::new(Some(hash), kex::image_key().as_ref());

    const KEY_LEN: usize = consts::DEFAULT_GRID_WIDTH * consts::DEFAULT_GRID_HEIGHT * 2;
    const NONCE_LEN: usize = consts::DEFAULT_GRID_WIDTH * consts::DEFAULT_GRID_HEIGHT;

    //GRID KEY, AT THE FULL KEYDIM SO Grid::from_key DOES NOT RE-DERIVE IT
    let mut key_bytes = Zeroizing::new(vec![0u8; KEY_LEN * 8]);
    hkdf.expand(b"WHY2-IMAGE-KEY", &mut key_bytes).expect("HKDF expand failed");

    //NONCE, EXPANDED SEPARATELY SO IT SHARES NO MATERIAL WITH THE KEY
    let mut nonce_bytes = Zeroizing::new(vec![0u8; NONCE_LEN * 8]);
    hkdf.expand(b"WHY2-IMAGE-NONCE", &mut nonce_bytes).expect("HKDF expand failed");

    let to_i64 = |bytes: &[u8]| bytes.chunks_exact(8)
        .map(|c| i64::from_be_bytes(c.try_into().unwrap()))
        .collect::<Vec<i64>>();

    (Zeroizing::new(to_i64(&key_bytes)), to_i64(&nonce_bytes))
}

#[cfg(feature = "server")]
pub fn history_keys() -> SharedKeys //AT-REST KEYS FOR THE MESSAGE HISTORY
{
    let hkdf = Hkdf::<Sha256>::new(None, kex::history_key().as_ref());

    //GRID KEY, AT THE FULL KEYDIM SO encrypt_packet DOES NOT RE-DERIVE IT
    const KEY_LEN: usize = consts::DEFAULT_GRID_WIDTH * consts::DEFAULT_GRID_HEIGHT * 2;

    let mut key_bytes = Zeroizing::new(vec![0u8; KEY_LEN * 8]);
    hkdf.expand(b"WHY2-HISTORY-KEY", &mut key_bytes).expect("HKDF expand failed");

    let key = key_bytes.chunks_exact(8).map(|c| i64::from_be_bytes(c.try_into().unwrap())).collect();

    //MAC KEY, EXPANDED SEPARATELY SO THE TAG NEVER SHARES MATERIAL WITH THE CIPHER
    let mut mac_key = Zeroizing::new(vec![0u8; 32]);
    hkdf.expand(b"WHY2-HISTORY-MAC", &mut mac_key).expect("HKDF expand failed");

    (Zeroizing::new(key), mac_key)
}
