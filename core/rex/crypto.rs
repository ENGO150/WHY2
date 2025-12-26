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

//! # REX Crypto
//!
//! This module contains cryptographic utilities, used by REX module

use sha2::{ Sha256, Digest };

use rand_chacha::ChaCha20Rng;
use rand::
{
    SeedableRng,
    TryRngCore,
    RngCore,
    rngs::OsRng,
};

use zeroize::Zeroizing;

use crate::rex::
{
    options,
    Grid,
    GridError,
};

/// Computes a SHA-256 hash of the Grid contents to produce a deterministic seed.
///
/// This function serializes the grid into native-endian bytes and feeds them into a SHA-256
/// hasher. The resulting 32-byte digest can be used as a seed for shuffling, masking, or
/// round-dependent randomness in the WHY2 cipher.
///
/// # Parameters
/// - `key`: A `Grid` reference whose contents will be hashed.
///
/// # Returns
/// A `[u8; 32]` array containing the SHA-256 digest of the grid.
///
/// # Notes
/// - The hash is computed in row-major order.
/// - Each `i64` cell is encoded using native-endian byte order.
/// - This method is deterministic and does not use any external randomness.
pub fn sha256_seed_grid<const W: usize, const H: usize>(key: &Grid<W, H>) -> [u8; 32]
{
    //SHA256
    let mut hasher = Sha256::new();

    //ADD TO HASH
    for row in key.iter()
    {
        for val in row
        {
            hasher.update(&val.to_be_bytes());
        }
    }

    //FINALIZE
    hasher.finalize().into()
}

/// Generates a deterministic key vector using a ChaCha20-based DRNG.
///
/// This function produces a `Vec<i64>` of length $2 \times W \times H$ by sampling from
/// the provided ChaCha20 random number generator. Each value is derived from a
/// `u64` output and cast to `i64`.
///
/// # Parameters
/// - `rng`: A mutable reference to a seeded [`ChaCha20Rng`] instance.
///
/// # Returns
/// A vector of signed 64-bit integers representing raw key material.
///
/// # Notes
/// - The output is deterministic for a given RNG seed.
pub fn generate_key_deterministic<const W: usize, const H: usize>(rng: &mut ChaCha20Rng) -> Zeroizing<Vec<i64>>
{
    Zeroizing::new((0..(2 * W * H)).map(|_| rng.next_u64() as i64).collect())
}

/// Generates a symmetric WHY2 key using secure system entropy.
///
/// This function creates a 32-byte seed using [`OsRng`], then initializes
/// a [`ChaCha20Rng`] with that seed to produce a deterministic
/// stream of pseudorandom values. The output is a flat `Vec<i64>` of length $2 \times W \times H$,
/// suitable for use with [`Grid::from_key`](crate::rex::Grid::from_key).
///
/// # Returns
/// A vector of signed 64-bit integers representing raw symmetric key material.
///
/// # Notes
/// - The key is generated using system entropy and is cryptographically secure.
/// - The output is deterministic for the derived seed, but the seed itself is random.
/// - This method is suitable for one-time key generation in encryption workflows.
pub fn generate_key<const W: usize, const H: usize>() -> Zeroizing<Vec<i64>>
{
    //CREATE SEED FOR ChaCha20Rng
    let mut seed = [0u8; 32];
    OsRng.try_fill_bytes(&mut seed).expect("Creating seed failed"); //FILL

    generate_key_deterministic::<W, H>(&mut ChaCha20Rng::from_seed(seed)) //USE HANDLER
}

/// Derives a sequence of round keys from a master Grid using deterministic hashing.
///
/// This function generates [`options::ROUND_KEYS`] round keys by chaining SHA-256 hashes
/// of the previous key. Each hash is used as a seed for a [`ChaCha20Rng`],
/// which produces a vector of `i64` values. These are then converted into `Grid`
/// instances using [`Grid::from_key`](Grid::from_key).
///
/// # Parameters
/// - `master_key`: The initial Grid used to seed the first round key.
///
/// # Returns
/// A vector of `Grid` round keys, each derived deterministically from the previous one.
///
/// # Notes
/// - The first key is seeded from `master_key`.
/// - Each subsequent key is seeded from the SHA-256 digest of the previous key.
/// - This method ensures reproducible round key generation without external randomness.
pub fn generate_round_keys<const W: usize, const H: usize>(master_key: &Grid<W, H>) -> Result<Vec<Grid<W, H>>, GridError>
{
    let mut keys: Vec<Grid<W, H>> = Vec::with_capacity(options::ROUND_KEYS);

    //GENERATE KEYS
    for _ in 0..(options::ROUND_KEYS)
    {
        //USE SEED OF LAST KEY TO GENERATE NEW KEY
        let key = generate_key_deterministic::<W, H>(&mut ChaCha20Rng::from_seed(sha256_seed_grid(keys.last().unwrap_or(master_key))));

        //CONVERT KEY TO Grid & PUSH TO keys
        keys.push(Grid::from_key(key)?);
    }

    Ok(keys)
}

/// Generates a random nonce for CTR mode.
///
/// This creates a single Grid filled with cryptographically secure random values
/// using system entropy. The nonce must be unique for each encryption session.
///
/// # Returns
/// A Grid suitable for use as a CTR nonce.
///
/// # Errors
/// Returns an error if the Grid dimensions are invalid (should never happen in practice).
///
/// # Notes
/// - The nonce does not need to be secret, but must be strictly unique per message.
/// - The nonce will be transmitted alongside the ciphertext.
pub fn generate_nonce<const W: usize, const H: usize>() -> Result<Grid<W, H>, GridError>
{
    Grid::from_key(generate_key::<W, H>())
}
