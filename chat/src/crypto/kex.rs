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

use p521::
{
    ecdh,
    PublicKey,
    SecretKey,
    elliptic_curve::Generate,
    pkcs8::
    {
        EncodePrivateKey,
        DecodePrivateKey,
        EncodePublicKey,
        DecodePublicKey,
        der::pem::
        {
            self,
            LineEnding,
        },
    },
};

use ml_kem::
{
    MlKem768,
    Ciphertext,
    DecapsulationKey768,
    EncapsulationKey768,
    KeyInit,
    TryKeyInit,
    kem::{ Encapsulate, Decapsulate },
};

use sha2::Sha256;
use hkdf::Hkdf;

use zeroize::Zeroizing;

use why2::consts;

use crate::
{
    misc,
    consts as consts_chat,
};

#[cfg(feature = "server")]
use std::
{
    fs,
    path::Path,
};

#[cfg(feature = "server")]
use ml_kem::
{
    KeyExport,
    kem::Kem,
};

//FUNCTIONS
//PRIVATE
fn decode_raw_pem(pem: &str) -> Option<Vec<u8>>
{
    pem::decode_vec(pem.as_bytes()).ok().map(|p| p.1.to_vec())
}

fn derive_encryption_keys(shared_secret: &[u8], info: &str) -> consts_chat::SharedKeys //GENERATE ENCRYPTION KEY AND MAC FROM SHARED SYM KEY
{
    let hkdf = Hkdf::<Sha256>::new(None, shared_secret);

    //DERIVE KEYS FOR ENCRYPTION & MAC
    let mut encryption_key = Zeroizing::new(vec![0u8; consts::DEFAULT_GRID_WIDTH * consts::DEFAULT_GRID_HEIGHT * 16]);
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

//PUBLIC
pub fn generate_ephemeral_keys() -> (Zeroizing<String>, String) //CREATE ECC KEYS
{
    //GENERATE PRIVATE KEY
    let private = SecretKey::generate();

    //ENCODE KEYS TO PEM
    let private_pem = private.to_pkcs8_pem(Default::default()).expect("Encoding key to PEM failed");
    let public_pem = private.public_key().to_public_key_pem(Default::default()).expect("Encoding pkey to PEM failed");

    //RETURN TUPLE OF PRIVATE AND PUBLIC KEYS
    (private_pem, public_pem.to_string())
}

#[cfg(feature = "server")]
pub fn generate_server_pq_keys() -> (Zeroizing<String>, String) //GENERATE POST-QUANTUM KEYS
{
    //GENERATE KEYS
    let (dk, ek) = MlKem768::generate_keypair();

    let dk_pem = pem::encode_string("PQ PRIVATE KEY", LineEnding::LF, dk.to_bytes().as_slice())
        .expect("Encoding EQ key to PEM failed");
    let ek_pem = pem::encode_string("PQ PUBLIC KEY", LineEnding::LF, ek.to_bytes().as_slice())
        .expect("Encoding EQ pkey to PEM failed");

    (Zeroizing::new(dk_pem), ek_pem)
}

#[cfg(feature = "server")]
pub fn generate_server_keys() //CREATE STATIC SERVER ECC KEYS
{
    //CHECK IF KEY DIRECTORY EXISTS
    let server_keys_dir = misc::get_why2_dir() + consts_chat::SERVER_KEYS_DIR;
    if !Path::new(&server_keys_dir).is_dir()
    {
        fs::create_dir_all(&server_keys_dir).expect("Failed to create WHY2 server-keys directory"); //CREATE DIRECTORY

        //GENERATE KEYS
        let (sk, pk) = generate_ephemeral_keys();   //ECC
        let (dk, ek) = generate_server_pq_keys(); //ML-KEM

        //SAVE ECC KEYS
        fs::write(server_keys_dir.clone() + consts_chat::SERVER_SKEY, sk.as_str()).expect("Saving server secret key failed");
        fs::write(server_keys_dir.clone() + consts_chat::SERVER_PKEY, pk).expect("Saving server public key failed");

        //SAVE PQ KEYS
        fs::write(server_keys_dir.clone() + consts_chat::SERVER_PQ_SKEY, dk.as_str()).expect("Saving server PQ secret key failed");
        fs::write(server_keys_dir + consts_chat::SERVER_PQ_PKEY, ek).expect("Saving server PQ public key failed");
    } else
    {
        //MIGRATE PQ KEYS FROM OLD EXPANDED FORMAT (ml-kem 0.2) TO NEW SEED FORMAT (ml-kem 0.3)
        let pq_sk_path = server_keys_dir.clone() + consts_chat::SERVER_PQ_SKEY;
        let needs_migration = match fs::read_to_string(&pq_sk_path)
        {
            Ok(pem_str) => decode_raw_pem(&pem_str).map_or(true, |bytes| bytes.len() != 64), //SEED IS 64 BYTES
            Err(_) => true, //MISSING FILE, REGENERATE
        };

        if needs_migration
        {
            log::warn!("PQ keys are in old format or missing, regenerating...");

            let (dk, ek) = generate_server_pq_keys();
            fs::write(&pq_sk_path, dk.as_str()).expect("Saving server PQ secret key failed");
            fs::write(server_keys_dir + consts_chat::SERVER_PQ_PKEY, ek).expect("Saving server PQ public key failed");
        }
    }
}

#[cfg(feature = "server")]
pub fn get_server_keys() -> (Zeroizing<String>, String) //GET SERVER ECC KEYS
{
    let server_keys_dir = misc::get_why2_dir() + consts_chat::SERVER_KEYS_DIR;

    let sk = fs::read_to_string(server_keys_dir.clone() + consts_chat::SERVER_SKEY).expect("Reading server secret key failed");
    let pk = fs::read_to_string(server_keys_dir + consts_chat::SERVER_PKEY).expect("Reading server public key failed");

    (Zeroizing::new(sk), pk)
}

#[cfg(feature = "server")]
pub fn get_server_pq_keys() -> (Zeroizing<String>, String) //GET SERVER ML-KEM KEYS
{
    let server_keys_dir = misc::get_why2_dir() + consts_chat::SERVER_KEYS_DIR;

    let dk = fs::read_to_string(server_keys_dir.clone() + consts_chat::SERVER_PQ_SKEY).expect("Reading server PQ secret key failed");
    let ek = fs::read_to_string(server_keys_dir + consts_chat::SERVER_PQ_PKEY).expect("Reading server PQ public key failed");

    (Zeroizing::new(dk), ek)
}

pub fn derive_shared_secret //DERIVE SHARED SYMKEY USING ECDH AND DERIVE ENCRYPTION & MAC KEY
(
    local_key: Zeroizing<String>,
    peer_pkey: String,
    pq_secret: Zeroizing<Vec<u8>>,
) -> Option<consts_chat::SharedKeys>
{
    //PARSE KEYS
    let local_private = SecretKey::from_pkcs8_pem(&local_key).expect("Invalid key");
    let remote_public = PublicKey::from_public_key_pem(&peer_pkey).ok()?;

    //COMPUTE EDCH
    let shared = ecdh::diffie_hellman(local_private.to_nonzero_scalar(), remote_public.as_affine());

    //COMBINE SECRETS
    let hkdf = Hkdf::<Sha256>::new(None, &[]);
    let mut combined = Zeroizing::new(vec![0u8; 64]);
    hkdf.expand_multi_info
    (
        &[&shared.raw_secret_bytes(), &pq_secret, b"WHY2-HYBRID"],
        &mut combined
    ).unwrap();

    //USE HKDF TO DERIVE SEPARATE ENCRYPTION AND MAC KEY
    Some(derive_encryption_keys(&combined, misc::get_version()))
}

pub fn encapsulate_pq(peer_pk_bytes: &str) -> (String, Zeroizing<Vec<u8>>)
{
    //DECODE PEM
    let pk_bytes = decode_raw_pem(peer_pk_bytes).expect("Decoding PEM failed");

    //DESERIALIZE KEY
    let ek = EncapsulationKey768::new_from_slice(&pk_bytes).expect("Invalid encapsulation key");

    //ENCAPSULATE
    let (ct, ss) = ek.encapsulate();

    //ENCODE CIPHERTEXT TO PEM
    let ct_pem = pem::encode_string("PQ CIPHERTEXT", LineEnding::LF, &ct).unwrap();

    (ct_pem, Zeroizing::new(ss.as_slice().to_vec()))
}

pub fn decapsulate_pq(local_sk_pem: &str, ciphertext_pem: &str) -> Option<Zeroizing<Vec<u8>>>
{
    //DECODE PEM
    let sk_bytes = Zeroizing::new(decode_raw_pem(local_sk_pem)?);
    let ct_bytes = decode_raw_pem(ciphertext_pem)?;

    //DESERIALIZE
    let dk = DecapsulationKey768::new_from_slice(&sk_bytes).ok()?;
    let ct = Ciphertext::<MlKem768>::try_from(ct_bytes.as_slice()).ok()?;

    Some(Zeroizing::new(dk.decapsulate(&ct).as_slice().to_vec()))
}
