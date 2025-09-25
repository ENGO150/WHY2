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
};

use openssl::
{
    nid::Nid,
    bn::BigNumContext,
    derive::Deriver,
    pkey::PKey,
    ec::
    {
        EcGroup,
        EcKey,
        EcPoint,
        PointConversionForm,
    },
};

use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

use sha2::{ Sha256, Digest };

use crate::
{
    core::crypto,
    core::rex::crypto as rex_crypto,
    core::misc,
    chat::options,
};

pub fn init_keys() //CREATE ECC KEYS
{
    //CHECK FOR KEYS DIRECTORY
    let key_dir = misc::get_why2_dir() + options::KEY_LOCATION;
    if Path::new(&key_dir).is_dir() { return; }

    //CREATE KEYS DIRECTORY
    fs::create_dir(&key_dir).expect("Failed creating keys directory");

    let group = EcGroup::from_curve_name(Nid::SECP521R1).expect("Invalid curve"); //CREATE secp512r1
    let ec_key = EcKey::generate(&group).expect("Key generation failed"); //GENERATE KEYPAIR

    //CONVERT PRIVATE KEY TO PEM
    let pem = ec_key.private_key_to_pem().expect("Getting PEM failed");

    //SAVE TO FILE
    let mut file = File::create(key_dir + options::KEY_FILENAME).expect("Creating keyfile failed");
    file.write_all(&pem).expect("Writing to keyfile failed");
}

pub fn get_public_key() -> String //SERIALIZE PUBKEY
{
    //READ KEY
    let key_pem = fs::read(misc::get_why2_dir() + options::KEY_LOCATION + options::KEY_FILENAME).expect("Reading keyfile failed");

    //PARSE PEM
    let ec_key = EcKey::private_key_from_pem(&key_pem).expect("Parsing PEM failed");

    //EXTRACT PUBKEY
    let pubkey = ec_key.public_key();

    //CONVERT TO STRING
    let pubkey_bytes = pubkey.to_bytes
    (
        &ec_key.group(),
        PointConversionForm::UNCOMPRESSED,
        &mut BigNumContext::new().expect("Failed to init BigNumContext")
    ).expect("Pubkey conversion failed");

    //ENCODE TO BASE91
    String::from_utf8(base91::slice_encode(&pubkey_bytes)).expect("Encoding pubkey failed")
}

pub fn get_shared_key(key: String) -> Vec<i64> //CALCULATES ECDH
{
    //DECODE key (REMOTE PUBLIC KEY)
    let pub_bytes = base91::slice_decode(key.as_bytes());

    //CURVE AND CONTEXT
    let group = EcGroup::from_curve_name(Nid::SECP521R1).expect("Invalid curve");
    let mut ctx = BigNumContext::new().expect("Failed to init BigNumContext");

    //CONVERT pub_bytes TO EcPoint
    let pub_point = EcPoint::from_bytes(&group, &pub_bytes, &mut ctx).expect("Converting pubkey failed");

    //CREATE EcKey FROM pub_point
    let remote_key = EcKey::from_public_key(&group, &pub_point).expect("Converting pubkey failed");

    //READ KEY
    let key_pem = fs::read(misc::get_why2_dir() + options::KEY_LOCATION + options::KEY_FILENAME).expect("Reading keyfile failed");

    //PARSE PEM
    let ec_key = EcKey::private_key_from_pem(&key_pem).expect("Parsing PEM failed");

    //PKeyS
    let local_pkey = PKey::from_ec_key(ec_key).expect("Invalid local key");
    let remote_pkey = PKey::from_ec_key(remote_key).expect("Invalid local key");

    //CREATE DERIVER FOR ECDH (USE LOCAL PRIVATE KEY)
    let mut deriver = Deriver::new(&local_pkey).expect("Invalid local key");
    deriver.set_peer(&remote_pkey).expect("Invalid remote key");

    //DERIVE SHARED SECRET & ENCODE TO BASE91
    let derived = String::from_utf8(base91::slice_encode(&deriver.derive_to_vec().expect("Converting deriver failed"))).expect("Encoding shared key failed");

    //SEED ChaCha20Rng USING derived
    let mut dprng = ChaCha20Rng::from_seed(crypto::sha256_seed(&derived));

    //RETURN GENERATED KEY
    rex_crypto::generate_key_deterministic(&mut dprng)
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
