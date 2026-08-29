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
    elliptic_curve::
    {
        Generate,
        sec1::ToSec1Point,
    },
    ecdsa::
    {
        Signature,
        VerifyingKey,
        signature::Verifier,
    },
};

use ml_kem::
{
    MlKem768,
    Ciphertext,
    EncapsulationKey768,
    KeyExport,
    kem::Encapsulate,
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
use crate::network::schema;

#[cfg(feature = "server")]
use p521::
{
    ecdsa::
    {
        SigningKey,
        signature::Signer,
    },
    pkcs8::
    {
        EncodePrivateKey,
        DecodePrivateKey,
        EncodePublicKey,
        der::pem::
        {
            self,
            LineEnding,
        },
    },
};

#[cfg(feature = "server")]
use std::
{
    path::Path,
    io::Write,
    fs::
    {
        self,
        DirBuilder,
        OpenOptions,
    },
};


#[cfg(all(feature = "server", unix))]
use std::os::unix::fs::
{
    DirBuilderExt,
    OpenOptionsExt,
    PermissionsExt,
};

#[cfg(feature = "server")]
use ml_kem::
{
    DecapsulationKey768,
    kem::
    {
        Kem,
        Decapsulate,
    },
};

//STRUCTS
#[cfg(feature = "server")] //THE SERVER'S HALF OF ONE EXCHANGE
pub struct Ephemeral
{
    ecc: SecretKey,
    pq: DecapsulationKey768,
}

#[cfg(feature = "server")]
pub struct Offer
{
    pub static_ecc: PublicKey,
    pub eph_ecc: PublicKey,
    pub pq: EncapsulationKey768,
    pub sig: Signature,
}

//FUNCTIONS
//PRIVATE
fn transcript(nonce: &[u8; 32], eph_ecc: &PublicKey, pq: &EncapsulationKey768) -> Vec<u8> //WHAT THE STATIC KEY SIGNS
{
    let eph_bytes = public_bytes(eph_ecc);
    let pq_bytes = pq.to_bytes();

    let mut message = Vec::with_capacity
    (
        consts_chat::KEX_CONTEXT.len() + nonce.len() + eph_bytes.len() + pq_bytes.len()
    );

    message.extend_from_slice(consts_chat::KEX_CONTEXT);
    message.extend_from_slice(nonce);
    message.extend_from_slice(&eph_bytes);
    message.extend_from_slice(&pq_bytes);

    message
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
pub fn public_bytes(key: &PublicKey) -> [u8; consts_chat::ECC_PUBKEY_SIZE]
{
    key.to_sec1_point(false).as_bytes().try_into().expect("Unexpected SEC1 point length")
}

pub fn generate_ephemeral_keys() -> (SecretKey, PublicKey) //CREATE EPHEMERAL ECC KEYS
{
    let private = SecretKey::generate();
    let public = private.public_key();

    (private, public)
}

#[cfg(feature = "server")]
fn generate_pem_keys() -> (Zeroizing<String>, String) //CREATE ECC KEYS IN THE ON-DISK PEM FORMAT
{
    //GENERATE PRIVATE KEY
    let private = SecretKey::generate();

    //ENCODE KEYS TO PEM
    let private_pem = private.to_pkcs8_pem(Default::default()).expect("Encoding key to PEM failed");
    let public_pem = private.public_key().to_public_key_pem(Default::default()).expect("Encoding pkey to PEM failed");

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
        let mut builder = DirBuilder::new();
        builder.recursive(true);

        //SET DIRECTORY PERMS ON UNIX-LIKE OS
        #[cfg(unix)]
        {
            builder.mode(0o700);
        }

        builder.create(&server_keys_dir).expect("Failed to create WHY2 server-keys directory");

        //GENERATE KEYS
        let (sk, pk) = generate_pem_keys();       //ECC
        let (dk, ek) = generate_server_pq_keys(); //ML-KEM

        //WRITE CLOSURE
        let write_secure_key = |path: String, data: &[u8]|
        {
            let mut options = OpenOptions::new();
            options.write(true).create(true).truncate(true);

            //SET FILE PERMS ON UNIX-LIKE OS
            #[cfg(unix)]
            {
                options.mode(0o600);
            }

            let mut file = options.open(&path)
                .unwrap_or_else(|_| panic!("Failed to open {} for writing", path));

            file.write_all(data)
                .unwrap_or_else(|_| panic!("Failed to write key to {}", path));
        };

        //SAVE ECC KEYS
        write_secure_key(server_keys_dir.clone() + consts_chat::SERVER_SKEY, sk.as_bytes());
        write_secure_key(server_keys_dir.clone() + consts_chat::SERVER_PKEY, pk.as_bytes());

        //SAVE PQ KEYS
        write_secure_key(server_keys_dir.clone() + consts_chat::SERVER_PQ_SKEY, dk.as_bytes());
        write_secure_key(server_keys_dir + consts_chat::SERVER_PQ_PKEY, ek.as_bytes());
    } else
    {
        //ENFORCE STRICT FILE PERMISSIONS
        #[cfg(unix)]
        {
            //ENFORCE 700 PERMS ON DIR
            if let Ok(metadata) = fs::metadata(&server_keys_dir)
            {
                let mut perms = metadata.permissions();
                perms.set_mode(0o700);
                fs::set_permissions(&server_keys_dir, perms).ok();
            }

            //FILE PERMS ENFORCE CLOSURE
            let enforce_file_perms = |file_name: &str|
            {
                let path = server_keys_dir.clone() + file_name;
                if let Ok(metadata) = fs::metadata(&path)
                {
                    let mut perms = metadata.permissions();
                    perms.set_mode(0o600);
                    fs::set_permissions(&path, perms).ok();
                }
            };

            //ENFORCE 600 PERMS ON FILES
            enforce_file_perms(consts_chat::SERVER_SKEY);
            enforce_file_perms(consts_chat::SERVER_PKEY);
            enforce_file_perms(consts_chat::SERVER_PQ_SKEY);
            enforce_file_perms(consts_chat::SERVER_PQ_PKEY);
        }
    }
}

#[cfg(feature = "server")]
pub fn get_server_keys() -> (Zeroizing<String>, String) //GET SERVER ECC KEYS (ON-DISK PEM)
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
    let ek = fs::read_to_string(server_keys_dir + consts_chat::SERVER_PQ_PKEY).expect("Reading server PQ pubkey failed");

    (Zeroizing::new(dk), ek)
}

#[cfg(feature = "server")]
pub fn create_offer(nonce: &[u8; 32]) -> (Ephemeral, Box<schema::Offer>) //SIGN A FRESH EPHEMERAL PAIR WITH THE STATIC IDENTITY
{
    //LOAD THE STATIC IDENTITY - IT ONLY EVER SIGNS
    let (static_sk_pem, _) = get_server_keys();
    let static_sk = SecretKey::from_pkcs8_pem(&static_sk_pem).expect("Invalid server secret key");
    let signing = SigningKey::from(&static_sk);

    //GENERATE BOTH EPHEMERALS
    let (eph_sk, eph_ecc) = generate_ephemeral_keys();
    let (pq_dk, pq) = MlKem768::generate_keypair();

    //SIGN THE TRANSCRIPT
    let sig: Signature = signing.sign(&transcript(nonce, &eph_ecc, &pq));

    (
        Ephemeral { ecc: eph_sk, pq: pq_dk },
        Box::new(schema::Offer { static_ecc: static_sk.public_key(), eph_ecc, pq, sig }),
    )
}

pub fn verify_offer //VERIFY AN OFFER AGAINST THE PINNED IDENTITY
(
    nonce: &[u8; 32],
    static_ecc: &PublicKey,
    eph_ecc: &PublicKey,
    pq: &EncapsulationKey768,
    sig: &Signature,
) -> bool
{
    let Ok(verifying) = VerifyingKey::from_sec1_bytes(&public_bytes(static_ecc)) else { return false };

    verifying.verify(&transcript(nonce, eph_ecc, pq), sig).is_ok()
}

pub fn derive_shared_secret //DERIVE SHARED SYMKEY USING ECDH AND DERIVE ENCRYPTION & MAC KEY
(
    local_key: SecretKey,
    peer_pkey: &PublicKey,
    pq_secret: Zeroizing<Vec<u8>>,
) -> consts_chat::SharedKeys
{
    //COMPUTE EDCH
    let shared = ecdh::diffie_hellman(local_key.to_nonzero_scalar(), peer_pkey.as_affine());

    //COMBINE SECRETS
    let hkdf = Hkdf::<Sha256>::new(None, &[]);
    let mut combined = Zeroizing::new(vec![0u8; 64]);
    hkdf.expand_multi_info
    (
        &[&shared.raw_secret_bytes(), &pq_secret, b"WHY2-HYBRID"],
        &mut combined
    ).unwrap();

    //USE HKDF TO DERIVE SEPARATE ENCRYPTION AND MAC KEY
    derive_encryption_keys(&combined, misc::get_version())
}

pub fn encapsulate_pq(peer_ek: &EncapsulationKey768) -> (Ciphertext<MlKem768>, Zeroizing<Vec<u8>>)
{
    //ENCAPSULATE
    let (ct, ss) = peer_ek.encapsulate();

    (ct, Zeroizing::new(ss.as_slice().to_vec()))
}

#[cfg(feature = "server")]
pub fn decapsulate_pq(ephemeral: &Ephemeral, ciphertext: &Ciphertext<MlKem768>) -> Zeroizing<Vec<u8>>
{
    Zeroizing::new(ephemeral.pq.decapsulate(ciphertext).as_slice().to_vec())
}

#[cfg(feature = "server")]
impl Ephemeral
{
    //THE ECC HALF IS CONSUMED BY THE AGREEMENT, WHICH IS ALSO WHAT ENDS ITS LIFE
    pub fn into_ecc(self) -> SecretKey { self.ecc }
}
