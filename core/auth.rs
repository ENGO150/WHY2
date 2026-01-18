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
//! This module provides HMAC-based authentication for encrypted WHY2 data.
//! It ensures message integrity and authenticity by computing a keyed hash
//! over the ciphertext and prepending it to the encrypted payload.
//!
//! # Overview
//! The authentication scheme uses HMAC-SHA256 to protect against tampering
//! and forgery attacks. The MAC is computed over the serialized ciphertext
//! and prepended to create an authenticated package:
//!
//! $$ \text{AuthData} = \text{HMAC-SHA256}(K_{\text{mac}}, C) \mathbin\| C $$
//!
//! where $C$ is the serialized ciphertext and $K_{\text{mac}}$ is the MAC key.
//!
//! # Security Properties
//! - **Integrity**: Any modification to the ciphertext will cause MAC verification to fail.
//! - **Authenticity**: Only parties with the correct MAC key can produce valid tags.
//! - **Encrypt-then-MAC**: The MAC is computed over ciphertext, following best practices.

use hmac::{ Hmac, Mac };
use sha2::Sha256;

use crate::
{
    options::EncryptedData,
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
pub struct AuthenticatedData<const W: usize, const H: usize>
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
    ///    $ \text{expected\_tag} = \text{HMAC-SHA256}(K_{\text{mac}}, C) $
    ///
    /// 3. **Constant-Time Comparison**: Compare the calculated tag with the stored `self.mac`:
    ///    $ \text{valid} \iff \text{CT\_EQ}(\text{tag}, \text{expected\_tag}) $
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
    pub fn verify(&self, mac_key: &[u8; 32]) -> bool
    {
        //USE HANDLER FOR COMPUTING HMAC
        compute_mac(&self.encrypted_data, mac_key).verify_slice(&self.mac).is_ok()
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
