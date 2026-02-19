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

//! # REX Crypto Utilities
//!
//! Provides key derivation, round key expansion, and CTR mode application
//! for the WHY2 encryption system.

use sha2::{ Sha256, Digest };

use rand_chacha::
{
    ChaCha20Rng,
    rand_core::{ SeedableRng, Rng },
};

use rand::
{
    TryRng,
    rngs::SysRng,
};

use rayon::prelude::
{
    ParallelIterator,
    IndexedParallelIterator,
    IntoParallelRefMutIterator,
};

use zeroize::Zeroizing;

use crate::
{
    consts,
    grid::{ Grid, GridError },
};

/// Computes a SHA-256 hash of the Grid contents to produce a deterministic seed.
///
/// This function serializes the grid into big-endian bytes and feeds them into a SHA-256
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
/// - Each `i64` cell is encoded using big-endian byte order.
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
/// This function creates a 32-byte seed using [`SysRng`], then initializes
/// a [`ChaCha20Rng`] with that seed to produce a deterministic
/// stream of pseudorandom values. The output is a flat `Vec<i64>` of length $2 \times W \times H$,
/// suitable for use with [`Grid::from_key`](crate::grid::Grid::from_key).
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
    SysRng.try_fill_bytes(&mut seed).expect("Creating seed failed"); //FILL

    generate_key_deterministic::<W, H>(&mut ChaCha20Rng::from_seed(seed)) //USE HANDLER
}

/// Derives a sequence of round keys from a master Grid using a deterministic CSPRNG stream.
///
/// This function generates [`consts::ROUND_KEYS`] round keys by expanding the master key
/// using a CSPRNG stream.
///
/// $$ S = \text{Hash}(\text{MasterKey}) $$
/// $$ K_0, \dots, K_N \leftarrow \text{CSPRNG}(S) $$
///
/// The master key hash is used as a seed for a [`ChaCha20Rng`], which produces a continuous
/// stream of `i64` values. These are then converted into `Grid` instances using
/// [`Grid::from_key`](crate::grid::Grid::from_key).
///
/// # Parameters
/// - `master_key`: The initial Grid used to seed the key generation.
///
/// # Returns
/// A vector of `Grid` round keys, derived deterministically from the master key.
///
/// # Notes
/// - The CSPRNG is initialized once using the `master_key` hash.
/// - All keys are generated from a single stream, acting as a KDF-Expand phase.
/// - This method ensures reproducible round key generation without external randomness.
pub fn generate_round_keys<const W: usize, const H: usize>(master_key: &Grid<W, H>) -> Result<Vec<Grid<W, H>>, GridError>
{
    let mut keys: Vec<Grid<W, H>> = Vec::with_capacity(consts::ROUND_KEYS);

    //CREATE CSPRNG FROM THE MASTER KEY
    let mut rng = ChaCha20Rng::from_seed(sha256_seed_grid(master_key)); //DERIVE SEED FROM MASTER KEY HASH

    //GENERATE KEYS
    for _ in 0..(consts::ROUND_KEYS)
    {
        //USE SEED OF LAST KEY TO GENERATE NEW KEY
        let key = generate_key_deterministic::<W, H>(&mut rng);

        //CONVERT KEY TO Grid & PUSH TO keys
        keys.push(Grid::from_key(&key)?);
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
    Grid::from_key(&generate_key::<W, H>())
}

/// Applies CTR (Counter) mode encryption/decryption in parallel.
///
/// This function transforms the input `grids` in-place by XORing them with a generated
/// keystream. Because CTR mode is symmetric, this function is utilized for both encryption
/// and decryption logic.
///
/// # Overview
/// For each grid block $G_i$ at index $i$, the transformation is defined as:
///
/// $$ G_i \leftarrow G_i \oplus E_K(\text{Nonce} + i) $$
///
/// Where $E_K$ denotes the WHY2 block cipher keyed with the provided `round_keys`.
/// The block counter is computed by adding the index $i$ to the base `nonce` using
/// wrapping arithmetic.
///
/// # Parallelism
/// This function utilizes [`rayon`] to process blocks concurrently. The keystream
/// for each block is generated independently through the following pipeline:
///
/// 1. **Counter Initialization**: $B = \text{Nonce} + i$
/// 2. **Initial Whitening**: $B \leftarrow B \oplus K_0$
/// 3. **Round Operations**: For each round $r$ and key $K_r$ (from 1 to $N$):
///    * **Key Addition**: $B \leftarrow B \oplus K_r$
///    * **Nonlinear Mixing**: $B \leftarrow \text{Subcell}(B, r)$
///    * **Row Permutation**: $B \leftarrow \text{ShiftRows}(B, K_r)$
///    * **Column Diffusion**: $B \leftarrow \text{MixColumns}(B, K_r)$
///
/// # Parameters
/// - `grids`: A mutable slice of [`Grid`]s representing the plaintext or ciphertext.
/// - `nonce`: The initial counter [`Grid`] (IV).
/// - `round_keys`: A sequence of round keys derived from the master key.
pub fn apply_ctr<const W: usize, const H: usize>
(
    grids: &mut [Grid<W, H>],
    nonce: &Grid<W, H>,
    round_keys: &[Grid<W, H>],
)
{
    //PRECALCULATE SHIFTS
    let round_shifts: Vec<[usize; H]> = round_keys.iter()
        .map(|k| k.precalculate_shifts())
        .collect();

    //PRECALCULATE ROTATIONS
    let round_rotations: Vec<[usize; W]> = round_keys[1..].iter().enumerate()
        .map(|(i, k)| k.precalculate_rotations(i))
        .collect();

    //APPLY ENCRYPTION TO EACH GRID (PARALLEL)
    grids.par_iter_mut().enumerate().for_each(|(i, grid)|
    {
        //CREATE KEYSTREAM BLOCK
        let mut keystream_block = nonce.clone();

        //BLOCK INDEX
        keystream_block.increment(&mut (i as u64));

        //INITIAL XOR
        keystream_block ^= &round_keys[0];

        //ROUND OPERATIONS
        for (i, round_key) in round_keys[1..].iter().enumerate()
        {
            keystream_block ^= round_key;                     //XOR
            keystream_block.subcell(i);                //SUBCELL (ARX)
            keystream_block.shift_rows(&round_shifts[i]);     //SHIFT ROWS
            keystream_block.mix_columns(&round_rotations[i]); //MIX COLUMNS (MDS)
        }

        //XOR KEYSTREAM AND DATA
        *grid ^= &keystream_block;
    });
}
