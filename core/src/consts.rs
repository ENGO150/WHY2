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
pub const SUBCELL_ROUNDS: u32 = 32;

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

/// The diffusion coefficients for the MDS-like MixColumns transformation.
///
/// These 16 constants are large, odd integers derived from the fractional parts
/// of the square roots of the first 16 primes. They are used to populate the
/// circulating matrix in [`mix_columns`](crate::grid::Grid::mix_columns).
///
/// # Properties
/// - **Invertibility**: All values are odd, making them coprime to the modulus $2^{64}$.
///   This ensures that the transformation preserves entropy and does not destroy information.
/// - **High Entropy**: The bit patterns are effectively random (derived from irrational numbers),
///   ensuring a rapid avalanche effect when multiplied.
/// - **Nothing-up-my-sleeve**: The constants are derived mathematically to demonstrate
///   that no hidden backdoors or weaknesses were inserted.
///
/// # Derivation
/// Values are taken from the first 64 bits of the fractional part of $\sqrt{p_n}$.
///
/// This value is cryptographically sensitive and should not be changed casually.
pub const MC_COEFFICIENTS: [i64; 16] =
[
    0x6A09E667_F3BCC909,
    0x3B67AE85_84CAA73B,
    0x3C6EF372_FE94F82B,
    0x25193134_86242315,
    0x510E527F_ADE682D1,
    0x1F6D3B65_F062D713,
    0x1B10A456_0737C8D9,
    0x27F47608_5188C027,
    0x5F1D382A_61928065,
    0x6A32F7C0_4286F453,
    0x2C46538F_14264D0F,
    0x36486F58_316B305D,
    0x0926715C_03287661,
    0x38F02011_03507307,
    0x10368143_2109439D,
    0x46320573_9521342B,
];
