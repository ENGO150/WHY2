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
/// Authenticated encryption data containing both MAC tag and encrypted data.
///
/// This structure represents the output of the authentication process,
/// combining a 32-byte HMAC-SHA256 tag with the original [`EncryptedData`].
///
/// # Layout
/// When serialized to bytes, the format is:
/// ```text
/// [32-byte HMAC-SHA256 tag][serialized encrypted grids]
/// ```
///
/// # Fields
/// - `mac`: The 32-byte HMAC-SHA256 authentication tag.
/// - `encrypted_data`: The original [`EncryptedData`] containing encrypted grids, nonce, and key.
///
/// # Notes
/// - The MAC is computed over the serialized grids (nonce + ciphertext).
/// - The nonce is included in the authenticated data to ensure freshness.
/// - This structure preserves the original grid structure for easy decryption.
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

//IMPLEMENTATIONS
impl<const W: usize, const H: usize> AuthenticatedData<W, H>
{
    /// Computes HMAC-SHA256 authentication tag for encrypted data.
    ///
    /// This function serializes the encrypted grids into big-endian bytes,
    /// computes an HMAC-SHA256 tag using the provided MAC key, and returns
    /// an [`AuthenticatedData`] structure containing both the tag and the original
    /// [`EncryptedData`].
    ///
    /// # Algorithm
    /// The authentication process follows these steps:
    ///
    /// 1. **Serialization**: Convert encrypted grids to bytes:
    ///    $ C = \text{nonce} \mathbin\| G_0 \mathbin\| G_1 \mathbin\| \cdots \mathbin\| G_n $
    ///    where each grid cell is encoded as big-endian `i64`.
    ///
    /// 2. **MAC Computation**: Calculate HMAC-SHA256:
    ///    $ \text{tag} = \text{HMAC-SHA256}(K_{\text{mac}}, C) $
    ///
    /// 3. **Package Construction**: Return structure with tag and original data:
    ///    $ \text{output} = (\text{tag}, \text{EncryptedData}) $
    ///
    /// # Parameters
    /// - `encrypted_data`: The [`EncryptedData`] structure containing:
    ///   - `output`: Vector of encrypted [`Grid`]s
    ///   - `nonce`: The CTR mode initialization vector
    ///   - `key`: The encryption key (not used in MAC computation)
    /// - `mac_key`: A 32-byte key for HMAC-SHA256 computation.
    ///
    /// # Returns
    /// An [`AuthenticatedData`] structure containing:
    /// - `mac`: The 32-byte HMAC-SHA256 tag
    /// - `encrypted_data`: The original [`EncryptedData`]
    ///
    /// # Security Notes
    /// - The MAC key must be derived independently from the encryption key.
    /// - The nonce is included in the authenticated data to prevent replay attacks.
    /// - Use HKDF or similar KDF to derive separate encryption and MAC keys.
    #[must_use]
    pub fn authenticate(encrypted_data: EncryptedData<W, H>, mac_key: &[u8; 32]) -> Self //CREATE HMAC
    {
        Self
        {
            mac: compute_mac(&encrypted_data, mac_key).finalize().into_bytes().into(), //USE HANDLER FOR COMPUTING HMAC
            encrypted_data,
        }
    }

    /// Verifies the integrity and authenticity of the encrypted data.
    ///
    /// This function re-computes the HMAC-SHA256 tag over the stored encrypted grids
    /// and compares it with the attached authentication tag (`self.mac`).
    ///
    /// # Algorithm
    /// The verification process mirrors the authentication steps to ensure consistency:
    ///
    /// 1. **Re-serialization**: The internal nonce and ciphertext grids are serialized
    ///    into bytes using the exact same order as during authentication (Big-Endian).
    ///
    /// 2. **MAC Re-computation**: Calculate the expected tag using the provided key:
    ///    $$ T' = \text{HMAC-SHA256}(K_{\text{mac}}, C) $$
    ///
    /// 3. **Constant-Time Comparison**: Compare the calculated tag with the stored `self.mac`:
    ///    $$ \text{valid} \iff T' \stackrel{?}{=} T $$
    ///
    /// where $T$ is the stored tag and $T'$ is the recomputed tag.
    ///
    /// # Parameters
    /// - `mac_key`: The 32-byte key used for HMAC-SHA256 computation.
    ///   Must be the same key used to generate the authentication tag.
    ///
    /// # Returns
    /// - `true`: If the computed tag matches `self.mac` (data is authentic).
    /// - `false`: If the tags do not match (integrity check failed).
    ///
    /// # Security Notes
    /// - This function uses **constant-time comparison** (`verify_slice`) to prevent
    ///   timing side-channel attacks.
    /// - If this function returns `false`, the `encrypted_data` MUST be discarded
    ///   and NOT treated as valid ciphertext.
    #[must_use]
    pub fn verify(&self, mac_key: &[u8; 32]) -> bool
    {
        //USE HANDLER FOR COMPUTING HMAC
        compute_mac(&self.encrypted_data, mac_key).verify_slice(&self.mac).is_ok()
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

impl<const W: usize, const H: usize> TryFrom<&[u8]> for AuthenticatedData<W, H>
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
            encrypted_data: EncryptedData
            {
                output: grids,
                nonce,
                key: Grid::new()?, //USE DUMMY KEY
            },
        })
    }
}

//PRIVATE FUNCTIONS
fn compute_mac<const W: usize, const H: usize>(encrypted_data: &EncryptedData<W, H>, mac_key: &[u8; 32]) -> Hmac<Sha256>
{
    //INITIALIZE HMAC
    let mut mac = Hmac::<Sha256>::new_from_slice(mac_key).expect("HMAC initialization failed");

    //FEED NONCE
    for row in encrypted_data.nonce.iter()
    {
        for val in row
        {
            mac.update(&val.to_be_bytes());
        }
    }

    //FEED GRIDS
    for grid in &encrypted_data.output
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
