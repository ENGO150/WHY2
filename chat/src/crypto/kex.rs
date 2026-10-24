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

//! # Hybrid Key Exchange
//!
//! The handshake that turns a fresh TCP connection into a pair of [`SharedKeys`](crate::consts::SharedKeys),
//! plus the management of the long-lived keys stored on disk.
//!
//! # Protocol
//! The server holds a static P-521 identity that **only ever signs**. For every connection:
//!
//! 1. The client sends a fresh nonce.
//! 2. The server generates an ephemeral P-521 pair and an ephemeral ML-KEM-768 pair, and signs
//!    `KEX_CONTEXT || nonce || eph_ecc || pq` with its identity (`create_offer`).
//! 3. The client checks the signature against the pinned identity ([`verify_offer`]),
//!    encapsulates to the ML-KEM key ([`encapsulate_pq`]) and answers with its own ephemeral
//!    P-521 key and the ciphertext.
//! 4. Both sides combine the ECDH secret and the ML-KEM secret with HKDF
//!    ([`derive_shared_secret`]) and expand the result into the grid key and the HMAC key.
//!
//! With $n$ the client's nonce, $E\_s$ and $E\_c$ the server's and the client's ephemeral P-521
//! keys, $P$ the ephemeral ML-KEM-768 encapsulation key and $\Vert$ concatenation, the offer is
//! signed as
//!
//! $$ \sigma = \mathrm{Sign}\_{sk}\left(\texttt{WHY2-KEX-V1} \Vert n \Vert E\_s \Vert P\right) $$
//!
//! and the session keys are derived from the two shared secrets $z\_{ec} = \mathrm{ECDH}(e\_s, E\_c)$
//! and $z\_{pq} = \mathrm{Decaps}(d\_P, c)$ as
//!
//! $$ S = \mathrm{HKDF\text{-}Expand}\left(\mathrm{PRK}\_0,\ z\_{ec} \Vert z\_{pq} \Vert \texttt{WHY2-HYBRID},\ 64\right) \qquad \mathrm{PRK}\_0 = \mathrm{HKDF\text{-}Extract}(\varnothing, \varnothing) $$
//!
//! $$ K = \mathrm{HKDF}(\varnothing,\ S,\ \texttt{WHY2-encryption}) \qquad K\_m = \mathrm{HKDF}(\varnothing,\ S,\ \texttt{WHY2-mac}) $$
//!
//! where $K$ is $16WH$ bytes (the grid key) and $K\_m$ is 32 bytes (the HMAC key).
//!
//! An attacker has to break **both** P-521 and ML-KEM-768 to recover a session key, and since
//! every key that encrypts is ephemeral, a later compromise of the identity does not expose
//! past sessions.
//!
//! # At-Rest Keys
//! The history, picture store and image cache keys (`history_key`, `image_key`,
//! `cache_key`) are random 32-byte files in the configuration root, created on first use
//! with mode `0600`. They are deliberately not derived from the identity, so the identity can
//! be rotated without losing stored data, and deleting a key file discards what it protected.


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

#[cfg(feature = "chat")]
use rand::
{
    TryRng,
    rngs::SysRng,
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
    },
};

#[cfg(feature = "chat")]
use std::
{
    io::Write,
    fs::
    {
        self,
        OpenOptions,
    },
};

#[cfg(feature = "server")]
use std::
{
    path::Path,
    fs::DirBuilder,
};

#[cfg(all(feature = "chat", unix))]
use std::os::unix::fs::OpenOptionsExt;

#[cfg(all(feature = "server", unix))]
use std::os::unix::fs::
{
    DirBuilderExt,
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
/// The server's ephemeral half of one exchange.
///
/// It is consumed by the exchange: the ML-KEM half decapsulates the client's ciphertext
/// ([`decapsulate_pq`]) and the ECC half performs the agreement ([`Ephemeral::into_ecc`]).
#[cfg(feature = "server")] //THE SERVER'S HALF OF ONE EXCHANGE
pub struct Ephemeral
{
    ecc: SecretKey,
    pq: DecapsulationKey768,
}

/// A signed server offer: the identity, the ephemeral keys and the signature over them.
#[cfg(feature = "server")]
pub struct Offer
{
    /// The server's static identity key.
    pub static_ecc: PublicKey,
    /// The ephemeral ECDH key of this exchange.
    pub eph_ecc: PublicKey,
    /// The ephemeral ML-KEM-768 encapsulation key of this exchange.
    pub pq: EncapsulationKey768,
    /// Signature by `static_ecc` over the transcript.
    pub sig: Signature,
}

//FUNCTIONS
//PRIVATE
/// Builds the transcript the identity signs: `KEX_CONTEXT || nonce || eph_ecc || pq`.
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

/// Expands the combined secret into the session's grid key and HMAC key.
fn derive_encryption_keys(shared_secret: &[u8]) -> consts_chat::SharedKeys //GENERATE ENCRYPTION KEY AND MAC FROM SHARED SYM KEY
{
    let hkdf = Hkdf::<Sha256>::new(None, shared_secret);

    //DERIVE KEYS FOR ENCRYPTION & MAC
    let mut encryption_key = Zeroizing::new(vec![0u8; consts::DEFAULT_GRID_WIDTH * consts::DEFAULT_GRID_HEIGHT * 16]);
    let mut mac = Zeroizing::new(vec![0u8; 32]);

    //EXPAND
    hkdf.expand(b"WHY2-encryption", &mut encryption_key).expect("HKDF expand failed");
    hkdf.expand(b"WHY2-mac", &mut mac).expect("HKDF expand failed");

    //CONVERT KEY BYTES TO i64s & RETURN WITH THE MAC
    (Zeroizing::new(encryption_key.chunks(8).map(|chunk|
    {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(chunk);
        i64::from_be_bytes(bytes)
    }).collect()), mac)
}

/// Loads a 32-byte at-rest key from the configuration root, creating it if missing or truncated.
///
/// # Parameters
/// - `filename`: The key file, relative to the configuration root.
///
/// # Panics
/// If the system RNG fails or the file cannot be written.
#[cfg(feature = "chat")]
fn media_key(filename: &str) -> Zeroizing<[u8; 32]>
{
    let path = misc::get_why2_dir() + filename;

    //LOAD KEY
    if let Ok(bytes) = fs::read(&path)
        && let Ok(key) = <[u8; 32]>::try_from(bytes.as_slice())
    {
        return Zeroizing::new(key);
    }

    //NO KEY, OR A TRUNCATED ONE - START A NEW ONE
    let mut key = Zeroizing::new([0u8; 32]);
    SysRng.try_fill_bytes(key.as_mut()).expect("Failed to generate history key");

    write_secure_key(path, key.as_ref());

    key
}

//PUBLIC
/// Encodes a P-521 public key as an uncompressed SEC1 point.
pub fn public_bytes(key: &PublicKey) -> [u8; consts_chat::ECC_PUBKEY_SIZE]
{
    key.to_sec1_point(false).as_bytes().try_into().expect("Unexpected SEC1 point length")
}

/// Generates an ephemeral P-521 keypair.
///
/// # Returns
/// `(private, public)`.
pub fn generate_ephemeral_keys() -> (SecretKey, PublicKey) //CREATE EPHEMERAL ECC KEYS
{
    let private = SecretKey::generate();
    let public = private.public_key();

    (private, public)
}

/// Generates a P-521 identity encoded as PEM.
///
/// # Returns
/// `(private PKCS#8 PEM, public SPKI PEM)`.
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

//THE FILE IS 0600 WHEREVER IT LANDS
/// Writes a secret to disk, readable by the owner only (`0600` on Unix).
///
/// # Panics
/// If the file cannot be opened or written.
#[cfg(feature = "chat")]
fn write_secure_key(path: String, data: &[u8]) //WRITE A SECRET TO DISK, READABLE BY NOBODY ELSE
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
}

/// Creates the server identity if it does not exist, or tightens the permissions of an
/// existing one (`0700` directory, `0600` files).
///
/// # Panics
/// If the key directory cannot be created.
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
        let (sk, pk) = generate_pem_keys();

        //SAVE ECC KEYS
        write_secure_key(server_keys_dir.clone() + consts_chat::SERVER_SKEY, sk.as_bytes());
        write_secure_key(server_keys_dir + consts_chat::SERVER_PKEY, pk.as_bytes());
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
            enforce_file_perms(consts_chat::SERVER_HISTORY_KEY);
        }
    }
}

/// Reads the server identity off disk.
///
/// # Returns
/// `(private PEM, public PEM)`.
///
/// # Panics
/// If either key file cannot be read.
#[cfg(feature = "server")]
pub fn get_server_keys() -> (Zeroizing<String>, String) //GET SERVER ECC KEYS (ON-DISK PEM)
{
    let server_keys_dir = misc::get_why2_dir() + consts_chat::SERVER_KEYS_DIR;

    let sk = fs::read_to_string(server_keys_dir.clone() + consts_chat::SERVER_SKEY).expect("Reading server secret key failed");
    let pk = fs::read_to_string(server_keys_dir + consts_chat::SERVER_PKEY).expect("Reading server public key failed");

    (Zeroizing::new(sk), pk)
}

/// Returns the message history's at-rest key, creating it on first use.
#[cfg(feature = "server")]
pub fn history_key() -> Zeroizing<[u8; 32]> //THE MESSAGE HISTORY'S AT-REST KEY, CREATED ON FIRST USE
{
    media_key(consts_chat::SERVER_HISTORY_KEY)
}

/// Returns the persistent picture store's at-rest key, creating it on first use.
#[cfg(feature = "server")]
pub fn image_key() -> Zeroizing<[u8; 32]> //PERSISTENT IMAGE KEY FILE
{
    media_key(consts_chat::SERVER_IMAGE_KEY)
}

/// Returns the client image cache's at-rest key, creating it on first use.
#[cfg(feature = "client_base")]
pub fn cache_key() -> Zeroizing<[u8; 32]> //CACHED IMAGE AT-REST KEY FILE
{
    media_key(consts_chat::CLIENT_CACHE_KEY)
}

/// Generates and signs a fresh ephemeral offer for one handshake.
///
/// # Parameters
/// - `nonce`: The client's handshake nonce, bound into the signed transcript.
///
/// # Returns
/// The server's secret half, kept for the rest of the exchange, and the offer to send.
///
/// # Panics
/// If the identity cannot be read or parsed.
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

/// Verifies a server offer against an identity.
///
/// # Parameters
/// - `nonce`: The nonce this client sent.
/// - `static_ecc`: The identity the offer claims; the caller checks it against the pinned key.
/// - `eph_ecc`, `pq`: The ephemeral keys offered.
/// - `sig`: The signature over the transcript.
///
/// # Returns
/// `true` if `sig` is a valid signature by `static_ecc` over this exchange.
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

/// Combines the ECDH and ML-KEM secrets into the session keys.
///
/// # Parameters
/// - `local_key`: This side's ephemeral ECDH private key.
/// - `peer_pkey`: The peer's ephemeral ECDH public key.
/// - `pq_secret`: The ML-KEM shared secret.
///
/// # Returns
/// The session's grid key and HMAC key.
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

    //HKDF SEPARATE ENCRYPTION AND MAC KEYS - NOT THE VERSION, THE HANDSHAKE HAS TO SURVIVE A MISMATCH
    derive_encryption_keys(&combined)
}

/// Encapsulates a fresh shared secret to the server's ML-KEM key.
///
/// # Returns
/// `(ciphertext to send, shared secret)`.
pub fn encapsulate_pq(peer_ek: &EncapsulationKey768) -> (Ciphertext<MlKem768>, Zeroizing<Vec<u8>>)
{
    //ENCAPSULATE
    let (ct, ss) = peer_ek.encapsulate();

    (ct, Zeroizing::new(ss.as_slice().to_vec()))
}

/// Recovers the ML-KEM shared secret from the client's ciphertext.
#[cfg(feature = "server")]
pub fn decapsulate_pq(ephemeral: &Ephemeral, ciphertext: &Ciphertext<MlKem768>) -> Zeroizing<Vec<u8>>
{
    Zeroizing::new(ephemeral.pq.decapsulate(ciphertext).as_slice().to_vec())
}

#[cfg(feature = "server")]
impl Ephemeral
{
    //THE AGREEMENT CONSUMES THE ECC HALF
    /// Consumes the exchange, returning the ECDH private key.
    pub fn into_ecc(self) -> SecretKey { self.ecc }
}
