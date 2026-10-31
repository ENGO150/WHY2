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

//! # REX Authentication
//!
//! This module implements the integrity and authenticity layer for WHY2, utilizing
//! an **Encrypt-then-MAC** scheme backed by HMAC-SHA256.
//!
//! ## Overview
//! While standard REX encryption guarantees confidentiality, it does not inherently prevent
//! ciphertext manipulation (bit-flipping) or forgery. This module wraps encrypted data
//! with a cryptographic authentication tag to ensure that:
//!
//! 1. Data has not been modified in transit (**Integrity**).
//! 2. Data originated from a party knowing the shared secret (**Authenticity**).
//!
//! ## Algorithm
//! The authentication process computes a Message Authentication Code (MAC) over the
//! serialized ciphertext and the initialization vector (Nonce).
//!
//! $$ \text{Tag} = \text{HMAC-SHA256}(K_{mac}, \text{Nonce} \mathbin\| \text{Ciphertext}) $$
//!
//! The final authenticated package is constructed by prepending the tag to the data:
//!
//! $$ \text{Packet} = \text{Tag} \mathbin\| \text{Nonce} \mathbin\| \text{Ciphertext} $$
//!
//! ## Binary Layout
//! When serialized for network transmission or storage, the byte stream follows this exact order:
//!
//! ```text
//! +----------------------------+-----------------------+---------------------------+
//! | HMAC Tag (32 bytes)        | Nonce (Grid Size)     | Ciphertext (N * Grids)    |
//! +----------------------------+-----------------------+---------------------------+
//! | Verified first (const-time)| Used for CTR setup    | Encrypted payload         |
//! +----------------------------+-----------------------+---------------------------+
//! ```
//!
//! ## Security Notes
//! - **Constant-Time Verification**: Tag comparison uses constant-time operations to prevent timing attacks.
//! - **Independent Keys**: It is strongly recommended to use different keys for encryption and authentication (or derive them via HKDF).
//! - **Order of Operations**: Verification MUST occur **before** decryption. If verification fails, the data is discarded immediately.
//! - **No Replay Protection**: A valid packet stays valid forever. The nonce binds the packet to a value but gives no freshness; track accepted counters/nonces, use timestamps or challenge-response at the protocol level.

use sha2::Sha256;
use hmac::
{
    Mac,
    Hmac,
    KeyInit,
};

use zeroize::Zeroize;

use crate::
{
    consts,
    grid::{ Grid, GridError },
    types::EncryptedData,
};

//STRUCTS
/// Sender-side authenticated data: the MAC tag together with the full [`EncryptedData`].
///
/// Created by [`authenticate`](Self::authenticate) and serialized with `Vec::<u8>::from`.
/// The serialization contains **no key**. The receiving side parses the bytes into an
/// [`AuthenticatedPacket`], which cannot be decrypted without supplying the key explicitly.
///
/// # Layout
/// ```text
/// [32-byte HMAC-SHA256 tag][nonce grid][ciphertext grids]
/// ```
///
/// # Notes
/// - The MAC is computed over the serialized nonce and ciphertext grids.
/// - The MAC provides integrity and authenticity, **not freshness**: a recorded packet
///   verifies again when replayed. Replay protection (accepted-counter state, timestamps or
///   challenge-response) must be provided by the protocol.
#[derive(Zeroize, Clone)]
pub struct AuthenticatedData
<
    const W: usize = { consts::DEFAULT_GRID_WIDTH },
    const H: usize = { consts::DEFAULT_GRID_HEIGHT },
>
{
    pub mac: [u8; 32],
    pub encrypted_data: EncryptedData<W, H>,
}

/// Receiver-side packet parsed from bytes: MAC tag, nonce and ciphertext, **without a key**.
///
/// The fields are private, so the only way to turn it into [`EncryptedData`] is
/// [`open`](Self::open), which verifies the tag first and takes the key as an explicit
/// argument. There is no dummy key to decrypt with by accident.
#[derive(Zeroize, Clone)]
pub struct AuthenticatedPacket
<
    const W: usize = { consts::DEFAULT_GRID_WIDTH },
    const H: usize = { consts::DEFAULT_GRID_HEIGHT },
>
{
    mac: [u8; 32],
    nonce: Grid<W, H>,

    #[zeroize(skip)]
    output: Vec<Grid<W, H>>,
}

//IMPLEMENTATIONS
impl<const W: usize, const H: usize> AuthenticatedData<W, H>
{
    /// Computes the HMAC-SHA256 tag over `nonce || ciphertext` and returns it together with
    /// the original [`EncryptedData`].
    ///
    /// $$ \text{tag} = \text{HMAC-SHA256}(K_{\text{mac}}, \text{nonce} \mathbin\| G_0 \mathbin\| \cdots \mathbin\| G_n) $$
    ///
    /// Each grid cell is encoded as a big-endian `i64`.
    ///
    /// # Security Notes
    /// - The MAC key must be derived independently from the encryption key (e.g. via HKDF).
    /// - The nonce is authenticated so it cannot be swapped. This does **not** prevent replay.
    /// - The MAC carries no version or context binding; use a unique MAC key per protocol.
    #[must_use]
    pub fn authenticate(encrypted_data: EncryptedData<W, H>, mac_key: &[u8; 32]) -> Self //CREATE HMAC
    {
        Self
        {
            mac: compute_mac(&encrypted_data.nonce, &encrypted_data.output, mac_key).finalize().into_bytes().into(),
            encrypted_data,
        }
    }

    /// Verifies the stored tag in constant time (`verify_slice`).
    ///
    /// If this returns `false`, the data MUST be discarded.
    #[must_use]
    pub fn verify(&self, mac_key: &[u8; 32]) -> bool
    {
        compute_mac(&self.encrypted_data.nonce, &self.encrypted_data.output, mac_key).verify_slice(&self.mac).is_ok()
    }
}

impl<const W: usize, const H: usize> AuthenticatedPacket<W, H>
{
    /// Verifies the stored tag in constant time (`verify_slice`).
    #[must_use]
    pub fn verify(&self, mac_key: &[u8; 32]) -> bool
    {
        compute_mac(&self.nonce, &self.output, mac_key).verify_slice(&self.mac).is_ok()
    }

    /// Verifies the packet and, only if the tag is valid, attaches `key` and returns
    /// [`EncryptedData`] ready for decryption.
    ///
    /// # Parameters
    /// - `mac_key`: The 32-byte HMAC key.
    /// - `key`: The master key [`Grid`], transmitted separately from the packet.
    ///
    /// # Returns
    /// - `Some(EncryptedData)` if the tag is valid.
    /// - `None` if verification failed; nothing is decrypted.
    #[must_use]
    pub fn open(self, mac_key: &[u8; 32], key: Grid<W, H>) -> Option<EncryptedData<W, H>>
    {
        if !self.verify(mac_key)
        {
            return None;
        }

        Some(EncryptedData
        {
            output: self.output,
            key,
            nonce: self.nonce,
        })
    }
}

impl<const W: usize, const H: usize> From<AuthenticatedData<W, H>> for Vec<u8> //SERIALIZATION
{
    fn from(data: AuthenticatedData<W, H>) -> Self
    {
        let grid_size = W * H * 8;
        let mut bytes = Vec::with_capacity(32 + (data.encrypted_data.output.len() + 1) * grid_size); //+1 FOR NONCE

        //ADD MAC
        bytes.extend_from_slice(&data.mac);

        //ADD NONCE
        for row in data.encrypted_data.nonce.iter()
        {
            for val in row
            {
                bytes.extend_from_slice(&val.to_be_bytes());
            }
        }

        //ADD CIPHERTEXT
        for grid in &data.encrypted_data.output
        {
            for row in grid.iter()
            {
                for val in row
                {
                    bytes.extend_from_slice(&val.to_be_bytes());
                }
            }
        }

        bytes
    }
}

impl<const W: usize, const H: usize> TryFrom<&[u8]> for AuthenticatedPacket<W, H>
{
    type Error = GridError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error>
    {
        let grid_size = W * H * 8;

        //THE SHORTEST WELL-FORMED PACKET IS A TAG PLUS THE NONCE GRID
        if bytes.len() < 32 + grid_size
        {
            return Err(GridError::InvalidByteLength { expected_mod: grid_size, actual_len: bytes.len() });
        }

        //SEPARATE MAC
        let (mac_slice, content) = bytes.split_at(32);

        //LOAD GRIDS (REJECTS ANY LENGTH THAT IS NOT A WHOLE NUMBER OF GRIDS)
        let mut grids = Grid::<W, H>::from_bytes(content)?;

        //REMOVE NONCE
        let nonce = grids.remove(0);

        let mut mac = [0u8; 32];
        mac.copy_from_slice(mac_slice);

        Ok(Self
        {
            mac,
            nonce,
            output: grids,
        })
    }
}

//PRIVATE FUNCTIONS
fn compute_mac<const W: usize, const H: usize>(nonce: &Grid<W, H>, output: &[Grid<W, H>], mac_key: &[u8; 32]) -> Hmac<Sha256>
{
    //INITIALIZE HMAC
    let mut mac = Hmac::<Sha256>::new_from_slice(mac_key).expect("HMAC initialization failed");

    //FEED NONCE
    for row in nonce.iter()
    {
        for val in row
        {
            mac.update(&val.to_be_bytes());
        }
    }

    //FEED GRIDS
    for grid in output
    {
        for row in grid.iter()
        {
            for val in row
            {
                mac.update(&val.to_be_bytes());
            }
        }
    }

    mac
}
