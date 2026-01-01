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

use sha2::{ Sha256, Digest };
use hkdf::Hkdf;
use hmac::{ Hmac, Mac };

use zeroize::Zeroizing;

use crate::
{
    Grid,
    encrypter,
    decrypter,
    options as core_options,
    chat::{ misc, options },
};

#[cfg(feature = "server")]
use argon2::
{
    Argon2,
    PasswordHasher,
    PasswordVerifier,
    password_hash::{ PasswordHash, SaltString },
};

//CONSTS
const GRID_W: usize = options::GRID_DIMENSIONS.0;
const GRID_H: usize = options::GRID_DIMENSIONS.1;

//PRIVATE
fn derive_encryption_keys(shared_secret: &[u8], info: &str) -> options::SharedKeys //GENERATE ENCRYPTION KEY AND MAC FROM SHARED SYM KEY
{
    let hkdf = Hkdf::<Sha256>::new(None, shared_secret);

    //DERIVE KEYS FOR ENCRYPTION & MAC
    let mut encryption_key = Zeroizing::new(vec![0u8; GRID_W * GRID_H * 16]);
    let mut mac = Zeroizing::new(vec![0u8; 32]);

    //EXPAND
    hkdf.expand(format!("{}-encryption", info).as_bytes(), &mut encryption_key).expect("HKDF expand failed");
    hkdf.expand(format!("{}-mac", info).as_bytes(), &mut mac).expect("HKDF expand failed");

    //CONVERT ENCRYPTION KEY BYTES TO i64s & RETURN TOGETHER WITH MAC
    (Zeroizing::new(encryption_key.chunks(8).map(|chunk|
    {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(chunk);
        i64::from_be_bytes(bytes)
    }).collect()), mac)
}

fn verify_hmac(packet: Vec<u8>, mac: &Vec<u8>) -> Option<Vec<u8>> //VERIFY HMAC
{
    //VERIFY MAC LENGTH
    if packet.len() < 32
    {
        return None;
    }

    //SEPARATE MAC FROM CIPHERTEXT
    let received_mac: [u8; 32] = packet[..32].try_into().unwrap();
    let ciphertext = &packet[32..];

    //COMPUTE EXPECTED HMAC
    let mut mac = Hmac::<Sha256>::new_from_slice(mac).expect("HMAC initialization failed");
    mac.update(ciphertext);

    //VERIFY MAC
    if mac.verify_slice(&received_mac).is_ok()
    {
        Some(ciphertext.to_vec())
    } else
    {
        None
    }
}

//PUBLIC
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

pub fn derive_shared_secret<const W: usize, const H: usize>(local_key: String, peer_pkey: String) -> Option<options::SharedKeys> //DERIVE SHARED SYMKEY USING ECDH AND DERIVE ENCRYPTION & MAC KEY
{
    //PARSE KEYS
    let local_private = SecretKey::from_pkcs8_pem(&local_key).expect("Invalid key");
    let remote_public = PublicKey::from_public_key_pem(&peer_pkey).ok()?;

    //COMPUTE EDCH
    let shared = ecdh::diffie_hellman(local_private.to_nonzero_scalar(), remote_public.as_affine());

    //USE HKDF TO DERIVE SEPARATE ENCRYPTION AND MAC KEY
    Some(derive_encryption_keys(shared.raw_secret_bytes(), misc::get_version()))
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
    let encrypted_data = encrypter::encrypt::<GRID_W, GRID_H>(input_i64, Some(keys.0.to_vec())).expect("Encrypting packet failed");

    //SERIALIZE ENCRYPTED PACKET
    let mut grids = encrypted_data.output;
    grids.insert(0, encrypted_data.nonce); //NONCE

    //CONVERT ENCRYPTED PACKET (FROM Vec<Grid>) TO Vec<u8>
    let encrypted_bytes: Vec<u8> = grids.iter()
        .flat_map(|grid| grid.iter()
            .flat_map(|row| row.iter()
                .flat_map(|&val| val.to_be_bytes()))).collect();

    //COMPUTE HMAC OVER CIPHERTEXT
    let mut mac = Hmac::<Sha256>::new_from_slice(&keys.1).expect("HMAC initialization failed");
    mac.update(&encrypted_bytes);

    let mac_tag = mac.finalize().into_bytes();

    //PREPEND MAC TO CIPHERTEXT ([32-byte HMAC][CIPHERTEXT])
    let mut transmission_bytes = Vec::with_capacity(32 + encrypted_bytes.len());
    transmission_bytes.extend_from_slice(&mac_tag);
    transmission_bytes.extend_from_slice(&encrypted_bytes);
    transmission_bytes
}

pub fn decrypt_packet(mut decoded_packet: Vec<u8>, keys: &options::SharedKeys) -> Option<Vec<u8>>
{
    //VERIFY HMAC
    if let Some(ciphertext) = verify_hmac(decoded_packet, &keys.1)
    {
        decoded_packet = ciphertext;
    } else
    {
        return None;
    }

    //DESERIALIZE ENCRYPTED PACKET
    let mut grids = Grid::<GRID_W, GRID_H>::from_bytes(&decoded_packet).ok()?;

    //EXTRACT NONCE
    let nonce = grids.remove(0);

    //DECRYPT
    let decrypted_packet = decrypter::decrypt(core_options::EncryptedData
    {
        output: grids,
        key: Grid::from_key(keys.0.clone().into()).unwrap(),
        nonce: nonce,
    }).ok()?;

    //OVERWRITE decoded_packet
    decoded_packet = Vec::with_capacity(decrypted_packet.output.len() * 8);
    for val in decrypted_packet.output.to_vec()
    {
        decoded_packet.extend_from_slice(&val.to_be_bytes());
    }

    Some(decoded_packet)
}
