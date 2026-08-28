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

/// Number of round keys used in the WHY2 cipher.
///
/// Each round key introduces nonlinear and linear mixing. This value controls the depth
/// of encryption and decryption. Increasing it strengthens diffusion but adds computational cost.
///
/// Do not modify unless you're fully aware of the cryptographic implications.
pub const ROUND_KEYS: usize = 16;

/// Number of ARX mixing iterations per cell in the [`subcell`](crate::grid::Grid::subcell) transformation.
///
/// This controls how many rounds of Add-Rotate-XOR are applied to each cell. More rounds
/// increase diffusion and resistance to pattern leakage.
///
/// Changing this affects the cipher’s nonlinear behavior.
pub const SUBCELL_ROUNDS: u32 = 4;

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
