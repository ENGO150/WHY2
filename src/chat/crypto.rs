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

use std::{ fs, path::Path };

use p521::
{
    ecdh,
    PublicKey,
    SecretKey,
    elliptic_curve::rand_core::OsRng,
    pkcs8::
    {
        EncodePrivateKey,
        DecodePrivateKey,
        EncodePublicKey,
        DecodePublicKey,
    },
};

#[cfg(feature = "server")]
use argon2::
{
    Argon2,
    PasswordHasher,
    PasswordVerifier,
    password_hash::{ PasswordHash, SaltString },
};

use rand_chacha::ChaCha20Rng;
use rand::SeedableRng;

use sha2::{ Sha256, Digest };

use crate::
{
    core::rex::crypto,
    chat::{ misc, options },
};

pub fn generate_ephemeral_keys() -> (String, String) //CREATE ECC KEYS
{
    //GENERATE PRIVATE KEY
    let private = SecretKey::random(&mut OsRng);

    //ENCODE KEYS TO PEM
    let private_pem = private.to_pkcs8_pem(Default::default()).expect("Encoding key to PEM failed");
    let public_pem = private.public_key().to_public_key_pem(Default::default()).expect("Encoding pkey to PEM failed");

    //RETURN TUPLE OF PRIVATE AND PUBLIC KEYS
    (private_pem.to_string(), public_pem.to_string())
}

pub fn generate_server_keys() //CREATE STATIC SERVER ECC KEYS
{
    //CHECK IF KEY DIRECTORY EXISTS
    let server_keys_dir = misc::get_why2_dir() + options::SERVER_KEYS_DIR;
    if !Path::new(&server_keys_dir).is_dir()
    {
        fs::create_dir_all(&server_keys_dir).expect("Failed to create WHY2 server-keys directory"); //CREATE DIRECTORY

        //GENERATE KEYS
        let (sk, pk) = generate_ephemeral_keys();

        //SAVE KEYS
        fs::write(server_keys_dir.clone() + options::SERVER_SKEY, sk).expect("Saving server secret key failed");
        fs::write(server_keys_dir + options::SERVER_PKEY, pk).expect("Saving server public key failed");
    }
}

pub fn get_server_keys() -> (String, String) //GET SERVER ECC KEYS
{
    let server_keys_dir = misc::get_why2_dir() + options::SERVER_KEYS_DIR;

    let sk = fs::read_to_string(server_keys_dir.clone() + options::SERVER_SKEY).expect("Reading server secret key failed");
    let pk = fs::read_to_string(server_keys_dir + options::SERVER_PKEY).expect("Reading server public key failed");

    (sk, pk)
}

pub fn derive_shared_secret<const W: usize, const H: usize>(local_key: String, peer_pkey: String) -> Vec<i64> //DERIVE SHARED SYMKEY USING ECDH
{
    //PARSE KEYS
    let local_private = SecretKey::from_pkcs8_pem(&local_key).expect("Invalid key");
    let remote_public = PublicKey::from_public_key_pem(&peer_pkey).expect("Invalid key");

    //COMPUTE EDCH
    let shared = ecdh::diffie_hellman(local_private.to_nonzero_scalar(), remote_public.as_affine());

    //SEED ChaCha20Rng USING SHARED KEY
    let shared_encoded = base91::slice_encode(shared.raw_secret_bytes());
    let mut dprng = ChaCha20Rng::from_seed(sha256(std::str::from_utf8(&shared_encoded).expect("Encoding shared key failed")));

    //RETURN GENERATED KEY
    crypto::generate_key_deterministic::<W, H>(&mut dprng)
}

pub fn sha256(seed_str: &str) -> [u8; 32] //GET HASH SEED; USED FOR PADDING
{
    //SHA256
    let mut hasher = Sha256::new();
    hasher.update(seed_str.as_bytes());

    //FINALIZE
    hasher.finalize().into()
}

#[cfg(feature = "server")]
pub fn hash_password(password: &str) -> String //HASH PASSWORD USING ARGON2
{
    //GENERATE RANDOM SALT
    let salt = SaltString::generate(&mut OsRng);

    //HASH
    Argon2::default().hash_password(password.as_bytes(), &salt).unwrap().to_string()
}

#[cfg(feature = "server")]
pub fn compare_password_hash(hashed: &str, password: &str) -> bool //COMPARE ARGON2 HASH WITH UNHASHED PASSWORD
{
    //PARSE HASH STRING
    let parsed_hash = PasswordHash::new(hashed).unwrap();

    //COMPARE
    Argon2::default().verify_password(password.as_bytes(), &parsed_hash).is_ok()
}
