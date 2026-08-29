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

//! # REX Constants
//!
//! This module defines the core cryptographic constants and configuration parameters
//! for the WHY2 algorithm. It includes round counts, mixing coefficients, and
//! default grid dimensions used to initialize the cipher state.

//PRIVATE MODULES
mod mds;
pub(crate) use mds::
{
    MDS_4,
    MDS_8,
    MDS_16,
};

/// Length of the round key schedule used by the WHY2 cipher.
///
/// The schedule is consumed as *whitening key, rounds, whitening key*: the first key is XORed
/// into the counter block before any round runs, the last is XORed into the result after every
/// round has run, and each key between them drives one round of nonlinear and linear mixing.
/// The number of keyed rounds is therefore `ROUND_KEYS - 2`, currently 15.
///
/// The two whitening keys are not optional padding. Without the trailing one the permutation
/// would end on [`mix_columns`](crate::grid::Grid::mix_columns) — a public, unkeyed, invertible
/// map — and CTR mode hands an attacker the permutation's input in the clear, so they could
/// strip that last linear layer for free and attack a cipher one round shorter than this one.
///
/// Raising this value strengthens diffusion but adds computational cost, and changes the
/// keystream: ciphertexts do not survive a change to it. Do not modify unless you're fully
/// aware of the cryptographic implications.
pub const ROUND_KEYS: usize = 17;

/// Number of ARX mixing iterations per cell in the [`subcell`](crate::grid::Grid::subcell) transformation.
///
/// This controls how many rounds of Add-Rotate-XOR are applied to each cell. More rounds
/// increase diffusion and resistance to pattern leakage.
///
/// Changing this affects the cipher’s nonlinear behavior.
pub const SUBCELL_ROUNDS: u32 = 5;

/// Constant used to break symmetry in ARX mixing.
///
/// This is derived from $\lfloor 2^{32} / \varphi \rfloor$, where $\varphi = \frac{1 + \sqrt{5}}{2}$
/// is the golden ratio. It ensures that each round introduces asymmetry and avoids cyclic patterns in the
/// [`subcell`](crate::grid::Grid::subcell) transformation.
///
/// This value is cryptographically sensitive and should not be changed casually.
pub const DELTA_32: u32 = 0x9E3779B9;

/// Constant used to break symmetry in ARX mixing.
///
/// This is derived from $\lfloor 2^{64} / \varphi \rfloor$. See [`DELTA_32`].
///
/// This value is cryptographically sensitive and should not be changed casually.
pub const DELTA_64: u64 = 0x9E3779B97F4A7C15;

/// The default width ($W$) of the [`Grid`](crate::grid::Grid).
///
/// This constant defines the number of columns in the standard grid configuration.
/// Together with [`DEFAULT_GRID_HEIGHT`], it determines the total state size.
pub const DEFAULT_GRID_WIDTH: usize = 8;

/// The default height ($H$) of the [`Grid`](crate::grid::Grid).
///
/// This constant defines the number of rows in the standard grid configuration.
pub const DEFAULT_GRID_HEIGHT: usize = 8;

/// Number of blocks below which the keystream is generated on the calling thread.
///
/// Handing a one-block slice to `rayon` costs more in pool dispatch than the block costs to
/// encrypt, which is the common case for the streaming API: a chat packet is frequently a
/// single grid. Above this the work is worth spreading.
pub const PARALLEL_THRESHOLD: usize = 4;
