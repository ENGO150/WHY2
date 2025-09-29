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

use std::
{
    io::Write,
    path::Path,
    fs::{ self, File },
    str,
};

use p521::
{
    ecdh,
    PublicKey,
    SecretKey,

    elliptic_curve::
    {
        rand_core::OsRng,
        sec1::ToEncodedPoint,
    },

    pkcs8::
    {
        DecodePrivateKey,
        EncodePrivateKey
    },
};

use rand_chacha::ChaCha20Rng;
use rand::SeedableRng;

use sha2::{ Sha256, Digest };

use crate::
{
    chat::{ options, misc },
    core::
    {
        crypto,
        rex::crypto as rex_crypto,
    },
};

//PRIVATE
fn get_private_key() -> SecretKey //LOAD PRIVATE KEY
{
    //READ PEM
    let private_pem = fs::read_to_string(misc::get_why2_dir() + options::KEY_LOCATION + options::KEY_FILENAME).expect("Reading keyfile failed");

    //PARSE & RETURN
    SecretKey::from_pkcs8_pem(&private_pem).expect("Parsing PEM failed")
}

//PUBLIC
pub fn init_keys() //CREATE ECC KEYS
{
    //CHECK FOR KEYS DIRECTORY
    let key_dir = misc::get_why2_dir() + options::KEY_LOCATION;
    if Path::new(&key_dir).is_dir() { return; }

    //CREATE KEYS DIRECTORY
    fs::create_dir(&key_dir).expect("Failed creating keys directory");

    //GENERATE PRIVATE KEY
    let private = SecretKey::random(&mut OsRng);

    //ENCODE PRIVATE KEY TO PEM
    let private_pem = private.to_pkcs8_pem(Default::default()).expect("Encoding to PEM failed");

    //SAVE TO FILE
    let mut file = File::create(key_dir + options::KEY_FILENAME).expect("Creating keyfile failed");
    file.write_all(&private_pem.as_bytes()).expect("Writing to keyfile failed");
}

pub fn get_public_key() -> String //SERIALIZE PUBKEY
{
    //EXTRACT PUBKEY FROM PRIVATE
    let public = get_private_key().public_key();

    //CONVERT TO STRING
    let public_bytes = public.to_encoded_point(false).as_bytes().to_vec();

    //ENCODE TO BASE91
    String::from_utf8(base91::slice_encode(&public_bytes)).expect("Encoding pubkey failed")
}

pub fn get_shared_key<const W: usize, const H: usize>(key: String) -> Vec<i64> //CALCULATES ELLIPTIC CURVE DIFFIE HELLMAN
{
    //DECODE key (REMOTE PUBLIC KEY)
    let remote_public_bytes = base91::slice_decode(key.as_bytes());

    //PARSE KEY
    let remote_public = PublicKey::from_sec1_bytes(&remote_public_bytes).expect("Invalid key");

    //LOAD LOCAL PRIVATE KEY
    let local_private = get_private_key();

    //COMPUTE EDCH
    let shared = ecdh::diffie_hellman(local_private.to_nonzero_scalar(), remote_public.as_affine());

    //SEED ChaCha20Rng USING SHARED KEY
    let shared_encoded = base91::slice_encode(shared.raw_secret_bytes());
    let mut dprng = ChaCha20Rng::from_seed(crypto::sha256_seed(str::from_utf8(&shared_encoded).expect("Encoding shared key failed")));

    //RETURN GENERATED KEY
    rex_crypto::generate_key_deterministic::<W, H>(&mut dprng)
}

pub fn sha256(seed_str: &str) -> String //HASH seed_str
{
    //SHA256
    let mut hasher = Sha256::new();
    hasher.update(seed_str.as_bytes());

    let result = hasher.finalize();

    //FORMAT
    format!("{:x}", result)
}
