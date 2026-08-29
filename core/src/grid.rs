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

//! # REX Grid
//!
//! This module defines the [`Grid`] structure, the fundamental building block of the WHY2 algorithm.
//!
//! ## Overview
//! The [`Grid`] represents the internal state of the cipher as a fixed-size 2D matrix of 64-bit integers.
//! It serves as the primary data structure for both:
//! - **Key Scheduling**: Storing and transforming key material.
//! - **Encryption State**: Holding the plaintext/ciphertext during round transformations.
//!
//! ## Core Operations
//! This module implements the fundamental transformations of the WHY2 cipher.
//! The architecture follows a Substitution-Permutation Network (SPN) pattern:
//! - **Nonlinear Mixing**: ARX-based [`subcell`](Grid::subcell) operations acting as a variable S-box.
//! - **Row Permutation**: Cyclical row shifting via [`shift_rows`](Grid::shift_rows) for horizontal diffusion.
//! - **Column Diffusion**: True MDS matrix multiplication via [`mix_columns`](Grid::mix_columns)
//!   over $\mathbb{F}_{2^{64}}$ for vertical diffusion and provably optimal branch number.
//!
//! ## Safety & Errors
//! Grid initialization is strictly validated to ensure cryptographic stability. Invalid dimensions
//! or malformed input data result in detailed [`GridError`] reports.

use std::
{
    array,
    result,
    error::Error,
    iter::Flatten,
    array::IntoIter,
    slice::
    {
        self,
        Iter,
        IterMut,
    },
    ops::
    {
        Index,
        IndexMut,
        BitXorAssign,
    },
    fmt::
    {
        Display,
        Formatter,
        Result,
        LowerHex,
    },
};

use zeroize::Zeroize;

use wide::i64x4;
use rayon::prelude::{ ParallelSlice, ParallelIterator };

use crate::{ consts, gf };

#[cfg(feature = "constant-time")]
use subtle::
{
    Choice,
    ConstantTimeEq,
};

//TYPES
/// A 2D matrix of 64-bit signed integers used as the core data structure in WHY2 encryption.
///
/// The [`Grid`] represents either input data or a key, formatted into rows and columns of `i64` cells.
/// All transformations—round mixing, key scheduling, and nonlinear diffusion—operate directly on this structure.
///
/// Grids are flexible and can be transformed in-place.
/// This abstraction allows WHY2 to generalize encryption over variable-sized blocks of dimension $W \times H$.
///
/// # Grid Size Consistency
///
/// WHY2 requires that the same grid dimensions ($W \times H$) be used consistently
/// throughout encryption and decryption. Mixing grid sizes within a single session or
/// across rounds is unsupported and may lead to incorrect results or undefined behavior.
#[derive(Clone, Debug, Zeroize)]
#[zeroize(drop)]
pub struct Grid //GRID FOR REX DATA
<
    const W: usize = { consts::DEFAULT_GRID_WIDTH },
    const H: usize = { consts::DEFAULT_GRID_HEIGHT },
>([[i64; W]; H]);

/// Represents structured errors that can occur during Grid operations.
///
/// This enum replaces generic string errors to provide zero-allocation error handling
/// and programmatic access to failure details. It is primarily used during
/// Grid initialization and serialization.
#[derive(Debug, Clone, PartialEq)]
pub enum GridError
{
    /// Indicates that the requested Grid dimensions are invalid for cryptographic operations.
    ///
    /// This error occurs when creating a new Grid if the dimensions do not allow
    /// for sufficient diffusion (e.g., width is 1, or total area is too small).
    ///
    /// # Fields
    /// - `width`: The width (columns) of the attempted Grid.
    /// - `height`: The height (rows) of the attempted Grid.
    InvalidDimensions
    {
        width: usize,
        height: usize,
    },

    /// Indicates that the input byte sequence length does not align with [`Grid`] requirements.
    ///
    /// This error occurs during deserialization (e.g., `from_bytes`) when the provided
    /// data length is not a multiple of the [`Grid`]'s total byte size ($W \times H \times 8$ bytes).
    ///
    /// # Fields
    /// - `expected_mod`: The required modulus (block size in bytes).
    /// - `actual_len`: The actual length of the provided byte vector.
    InvalidByteLength
    {
        expected_mod: usize,
        actual_len: usize,
    },

    /// Indicates that the provided raw key has an incorrect length.
    ///
    /// The WHY2 key scheduling algorithm requires the input key vector to be exactly
    /// twice the size of the [`Grid`] area ($2 \times W \times H$). This allows for the initial
    /// folding and mixing of key parts (low and high components).
    ///
    /// # Fields
    /// - `expected_len`: The required key length (number of `i64` elements).
    /// - `actual_len`: The length of the provided key vector.
    InvalidKeyLength
    {
        expected_len: usize,
        actual_len: usize,
    },

    /// Indicates that decryption produced an invalid Unicode scalar value.
    ///
    /// This typically happens when the provided key is incorrect, resulting in
    /// random garbage data that does not represent valid text.
    ///
    /// # Fields
    /// - `value`: The invalid Unicode scalar value.
    InvalidUnicode
    {
        value: u32,
    },
}

//MACROS
macro_rules! subcell //SUBCELL CORE LOGIC
{
    ($v0:ident, $v1:ident, $sum:ident, $delta:expr, $round_tweak:expr, $mask:expr) =>
    {
        //XOR TWEAK -> MAKE ROUNDS DIFFERENT
        $v0 = ($v0 ^ $round_tweak) & $mask;

        //ARX-LIKE ROUNDS (INSPIRED BY XTEA/TEA)
        for _ in 0..consts::SUBCELL_ROUNDS
        {
            $sum += $delta;

            //MIX
            $v0 = ($v0 + ((($v1 << 4) ^ ($v1 >> 5)) + $v1 ^ $sum)) & $mask; //MIX V1 INTO V0
            $v1 = ($v1 + ((($v0 << 4) ^ ($v0 >> 5)) + $v0 ^ $sum)) & $mask; //MIX V0 INTO V1
        }

        //XOR TWEAK
        $v1 = ($v1 ^ $round_tweak) & $mask;
    }
}

//HADAMARD DIFFUSION
/// Checks at compile time that an MDS matrix satisfies $M_{i,j} = M_{0,\, i \oplus j}$.
///
/// [`mix_columns`](Grid::mix_columns) reads only row 0 of the matrix and indexes it by
/// $i \oplus j$, so a matrix that lost this shape would be silently multiplied by a different
/// matrix than the one written down. The assertions below fail the build instead.
const fn is_hadamard<const N: usize>(m: &[[u64; N]; N]) -> bool
{
    let mut i = 0;
    while i < N
    {
        let mut j = 0;
        while j < N
        {
            if m[i][j] != m[0][i ^ j] { return false; }
            j += 1;
        }

        i += 1;
    }

    true
}

/// Checks at compile time that an MDS matrix is its own inverse.
///
/// For a Hadamard matrix $(M^2)_{i,j} = \sum_k m_{i \oplus k} \, m_{k \oplus j}$. Off the
/// diagonal every term pairs with the one at $k \oplus i \oplus j$ and the two cancel; on it
/// the sum is $\sum_k m_k^2 = \left( \bigoplus_k m_k \right)^2$, because squaring is
/// $\mathbb{F}_2$-linear in characteristic two. So $M^2 = \left( \bigoplus_k m_k \right)^2 I$
/// and the matrix is involutory exactly when its row XORs to one — which takes no field
/// arithmetic to check, and is why undoing [`mix_columns`](Grid::mix_columns) needs no second
/// matrix and no second code path: applying it twice is the identity.
const fn is_involutory<const N: usize>(m: &[[u64; N]; N]) -> bool
{
    let mut x = 0u64;
    let mut i = 0;

    while i < N
    {
        x ^= m[0][i];
        i += 1;
    }

    x == 1
}

const _: () = assert!(is_hadamard(&consts::MDS_4), "MDS_4 is not Hadamard; mix_columns reads it as if it were");
const _: () = assert!(is_hadamard(&consts::MDS_8), "MDS_8 is not Hadamard; mix_columns reads it as if it were");
const _: () = assert!(is_hadamard(&consts::MDS_16), "MDS_16 is not Hadamard; mix_columns reads it as if it were");

const _: () = assert!(is_involutory(&consts::MDS_4), "MDS_4 is not involutory; mix_columns would no longer undo itself");
const _: () = assert!(is_involutory(&consts::MDS_8), "MDS_8 is not involutory; mix_columns would no longer undo itself");
const _: () = assert!(is_involutory(&consts::MDS_16), "MDS_16 is not involutory; mix_columns would no longer undo itself");

/// ARX transform of one cell, used for the tail that the vector loops cannot cover.
///
/// Arithmetic is done in `u64` with wrapping adds so the 32-bit halves behave exactly as the
/// masked vector lanes do. The previous tail used `u32` with a plain `+`, which would panic on
/// overflow in a debug build.
#[inline(always)]
fn subcell_cell(cell: i64, round: usize) -> i64
{
    const MASK: u64 = 0xFFFF_FFFF;

    let x = cell as u64;
    let mut v0 = x & MASK;
    let mut v1 = (x >> 32) & MASK;
    let mut sum: u64 = 0;

    //XOR TWEAK -> MAKE ROUNDS DIFFERENT
    v0 = (v0 ^ round as u64) & MASK;

    //ARX-LIKE ROUNDS (INSPIRED BY XTEA/TEA)
    for _ in 0..consts::SUBCELL_ROUNDS
    {
        sum = sum.wrapping_add(consts::DELTA_32 as u64);

        v0 = v0.wrapping_add((((v1 << 4) ^ (v1 >> 5)).wrapping_add(v1)) ^ sum) & MASK; //MIX V1 INTO V0
        v1 = v1.wrapping_add((((v0 << 4) ^ (v0 >> 5)).wrapping_add(v0)) ^ sum) & MASK; //MIX V0 INTO V1
    }

    //XOR TWEAK
    v1 = (v1 ^ round as u64) & MASK;

    ((v1 << 32) | v0) as i64
}

/// XOR of two lanes, written by value rather than through indices: a compiler will fold this
/// into one vector instruction, where the equivalent loop over a `&mut` array often does not
/// leave the scalar registers.
#[inline(always)]
fn xor_lane(a: [u64; gf::LANE], b: [u64; gf::LANE]) -> [u64; gf::LANE]
{
    array::from_fn(|i| a[i] ^ b[i])
}

/// [`gf::xtime`] across a lane. See [`xor_lane`] for why it takes and returns by value.
#[inline(always)]
fn xtime_lane(v: [u64; gf::LANE]) -> [u64; gf::LANE]
{
    array::from_fn(|i| gf::xtime(v[i]))
}

macro_rules! mix_terms //ONE INPUT ROW'S CONTRIBUTION TO EVERY OUTPUT ROW
{
    ($m:expr, $exp:expr, $x:expr, $out:expr, $k:literal, [$($i:literal),*]) =>
    {{
        //RUNNING VALUE: x^e TIMES INPUT ROW $k, ONE LANE WIDE
        let mut s = $x[$k];

        for e in 0..$exp
        {
            if e > 0 { s = xtime_lane(s); }

            //ONE STRAIGHT-LINE TEST PER OUTPUT ROW, EACH ON A CONSTANT COEFFICIENT
            $( if $m[$i ^ $k] >> e & 1 == 1 { $out[$i] = xor_lane($out[$i], s); } )*
        }
    }}
}

macro_rules! mix_hadamard //ONE MONOMORPHISED DIFFUSION LAYER
{
    ($name:ident, $n:literal, $row:expr, $is:tt, [$($k:literal),*]) =>
    {
        /// Multiplies a lane of columns by the order-`$n` MDS matrix.
        ///
        /// The matrix is Hadamard, so $M_{i,j} = m_{i \oplus j}$ and its first row is all of
        /// it. Every $m_d$ is a sum of powers of $x$ with small exponents, which is what makes
        /// the product this cheap: walk the input rows, keep multiplying the running value by
        /// $x$ with `xtime`, and XOR it into each output row whose coefficient uses that power.
        /// No general field multiply is performed anywhere in here.
        ///
        /// **Both index loops are unrolled by the macro**, which is the whole point of writing
        /// it this way: with $i$ and $k$ literal, every `M[i ^ k]` is a constant the compiler
        /// folds the coefficient test against, leaving a fixed straight-line sequence of shifts
        /// and XORs over [`gf::LANE`]-wide values. Left as ordinary loops the same code is
        /// **twice as slow**: the coefficients stay in memory and each term costs a load, a bit
        /// test and a branch.
        #[inline(always)]
        fn $name(x: &[[u64; gf::LANE]; $n]) -> [[u64; gf::LANE]; $n]
        {
            const M: [u64; $n] = $row;

            //HIGHEST POWER OF x ANY COEFFICIENT USES
            const EXP: u32 =
            {
                let mut bits = 0u64;
                let mut i = 0;

                while i < $n
                {
                    bits |= M[i];
                    i += 1;
                }

                64 - bits.leading_zeros()
            };

            let mut out = [[0u64; gf::LANE]; $n];

            $( mix_terms!(M, EXP, x, out, $k, $is); )*

            out
        }
    }
}

mix_hadamard!
(
    mix_4, 4, consts::MDS_4[0],
    [0, 1, 2, 3],
    [0, 1, 2, 3]
);

mix_hadamard!
(
    mix_8, 8, consts::MDS_8[0],
    [0, 1, 2, 3, 4, 5, 6, 7],
    [0, 1, 2, 3, 4, 5, 6, 7]
);

mix_hadamard!
(
    mix_16, 16, consts::MDS_16[0],
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
);


//IMPLEMENTATIONS
/// Implementation of core Grid operations for fixed-size grids.
///
/// This block defines methods for `Grid<W, H>`, where `W` and `H` are compile-time constants
/// representing the grid's width and height. All transformations — such as ARX mixing, key application,
/// and round-based encryption - operate on grids of this fixed shape.
///
/// # Type Parameters
/// - `W`: Number of columns (width), must be a compile-time constant.
/// - `H`: Number of rows (height), must be a compile-time constant.
///
/// # Notes
/// - Grid dimensions must remain consistent across encryption and decryption.
impl<const W: usize, const H: usize> Grid<W, H>
{
    /// Creates a new Grid initialized with zeroes.
    ///
    /// This constructor sets up an empty Grid where all cells are set to `0`.
    ///
    /// # Returns
    /// - Ok(`Grid`) instance with all values set to zero.
    /// - Err(`GridError`) if the area is invalid.
    ///
    /// # Notes
    /// - This method does not perform any encryption or transformation.
    /// - Valid area is defined by $W > 0$ and $H \in \lbrace4, 8, 16\rbrace$
    #[inline]
    #[must_use]
    pub fn new() -> result::Result<Self, GridError>
    {
        if W > 0 && (H == 4 || H == 8 || H == 16)
        {
            Ok(Self([[0i64; W]; H]))
        } else
        {
            Err(GridError::InvalidDimensions { width: W, height: H })
        }
    }

    /// Initializes a key Grid from a vector of signed 64-bit integers.
    ///
    /// Each cell is built from two key parts using nonlinear mixing:
    /// addition, XOR, and key-dependent rotation. This improves diffusion
    /// and ensures that both the values and the mixing angles depend on
    /// the key material.
    ///
    /// # Algorithm
    /// For each cell index $i$, two intermediate values are derived from the input vector $V$:
    ///
    /// $$ A = V_i + V_{i + \text{Area}} $$
    ///
    /// $$ B = V_i \oplus V_{i + \text{Area}} $$
    ///
    /// Rotation amounts are derived via cross-dependence — each value is rotated
    /// by an angle derived from the other:
    ///
    /// $$ A' = A \lll (B \bmod 64) $$
    ///
    /// $$ B' = B \ggg (A \bmod 64) $$
    ///
    /// where $\lll$ and $\ggg$ denote left and right rotation respectively.
    ///
    /// The final grid value is computed as:
    ///
    /// $$ \text{Grid}_{x,y} = A' \oplus B' \oplus i $$
    ///
    /// where $i$ acts as domain separation, ensuring distinct positions
    /// produce distinct output even for identical key values.
    ///
    /// # Parameters
    /// - `vec`: A slice of signed 64-bit integers representing the raw key.
    ///   Must contain at least $2 \times W \times H$ elements.
    ///
    /// # Returns
    /// - Ok(`Grid`) with mixed key values if input is valid.
    /// - Err([`GridError::InvalidKeyLength`]) if `vec.len() < 2 × W × H`.
    /// - Err([`GridError::InvalidDimensions`]) if grid dimensions are invalid.
    #[must_use]
    pub fn from_key(vec: &[i64]) -> result::Result<Self, GridError>
    {
        //GRID OPTIONS
        let grid_area = W * H;

        //CHECK INVALID KEY LENGTH
        if vec.len() < grid_area * 2
        {
            return Err(GridError::InvalidKeyLength
            {
                expected_len: grid_area * 2,
                actual_len: vec.len(),
            });
        }

        //SHAPE
        let mut key_grid = Self::new()?;
        for i in 0..grid_area
        {
            //APPLY NONLINEAR MIX TO KEY
            let mut a = vec[i].wrapping_add(vec[i + grid_area]);
            let mut b = vec[i] ^ vec[i + grid_area];

            //CALCULATE ROTATIONS
            let rot_a = (b as u32) & 63;
            let rot_b = (a as u32) & 63;

            //ROTATE
            a = a.rotate_left(rot_a);
            b = b.rotate_right(rot_b);

            //APPLY
            key_grid[i / W][i % W] = a ^ b ^ (i as i64);
        }

        Ok(key_grid)
    }

    /// Initializes [`Grid`] from vector of unsigned 8-bit integers.
    ///
    /// This function constructs [`Grid`] by chunking the input vector into `i64` cells. It expects
    /// exactly $W \times H \times 8$ bytes and returns an error if the input length does not match.
    ///
    /// # Parameters
    /// - `bytes`: A byte slice (`&[8u]`) containing the raw data.
    ///
    /// # Returns
    /// - Ok(Vec<`Grid`>) if the byte length matches the expected grid size
    /// - Err(`GridError`) if the input length is not divisible by matrix size.
    ///
    /// # Notes
    /// - No transformation is applied
    /// - Use this for raw Grid construction, not for secure key loading
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> result::Result<Vec<Self>, GridError>
    {
        let matrix_size = W * H * 8; //EACH i64 IS 8 BYTES

        //CHECK FOR VALID GRID
        if bytes.len() % matrix_size != 0
        {
            return Err(GridError::InvalidByteLength { expected_mod: matrix_size, actual_len: bytes.len() });
        }

        bytes.par_chunks(matrix_size).map(|chunk|
        {
            let mut grid = Grid::new()?;
            for j in 0..H
            {
                for i in 0..W
                {
                    let start = (j * W + i) * 8;
                    let slice = &chunk[start..start + 8];
                    grid[j][i] = i64::from_be_bytes(slice.try_into().unwrap());
                }
            }

            Ok(grid)
        }).collect()
    }

    /// Initializes [`Grid`] from a flat slice of 64-bit integers.
    ///
    /// This function constructs a [`Grid`] by sequentially taking up to $W \times H$ elements
    /// from the provided slice and placing them into the 2D grid structure, row by row.
    ///
    /// # Parameters
    /// - `vec`: A slice of signed 64-bit integers (`&[i64]`). Elements beyond the grid's capacity are ignored.
    ///   If the slice is shorter than the grid capacity, the remaining cells retain their initial (zeroed) state.
    ///
    /// # Returns
    /// - Ok(`Grid`) populated with the values from the slice.
    /// - Err([`GridError::InvalidDimensions`]) if grid dimensions are invalid.
    #[must_use]
    pub fn from_flat(vec: &[i64]) -> result::Result<Self, GridError>
    {
        let mut grid = Self::new()?;
        for (i, &val) in vec.iter().take(W * H).enumerate()
        {
            grid[i / W][i % W] = val;
        }
        Ok(grid)
    }

    /// Converts the [`Grid`] into a flat vector of 64-bit integers.
    ///
    /// This method flattens the 2D grid structure by sequentially iterating over its rows
    /// and collecting all cells into a single, continuous [`Vec<i64>`].
    ///
    /// # Returns
    /// - A `Vec<i64>` containing exactly $W \times H$ elements extracted from the grid.
    #[must_use]
    pub fn to_flat(&self) -> Vec<i64>
    {
        self.iter().flat_map(|row| row.iter().copied()).collect()
    }

    /// Returns an iterator over rows in the Grid
    #[inline(always)]
    pub fn iter(&self) -> Iter<'_, [i64; W]>
    {
        self.0.iter()
    }

    /// Returns a mutable iterator over rows in the Grid
    #[inline(always)]
    pub fn iter_mut(&mut self) -> IterMut<'_, [i64; W]>
    {
        self.0.iter_mut()
    }

    /// Returns width (number of columns) in the Grid
    #[inline(always)]
    pub fn width(&self) -> usize
    {
        W
    }

    /// Returns height (number of rows) in the Grid
    #[inline(always)]
    pub fn height(&self) -> usize
    {
        H
    }

    //ENCRYPTION
    /// Computes the cell-wise XOR of two Grids.
    ///
    /// This function takes two [`Grid`]s of equal dimensions and modifies the [`Grid`] in-place:
    /// $$ G_{x,y} = G_{x,y} \oplus K_{x,y} $$
    /// It is used in WHY2 for mixing round keys, applying masks, or combining intermediate states.
    ///
    /// # Parameters
    /// - `key_grid`: Input Grid for XOR
    ///
    /// # Implementation
    /// Uses SIMD acceleration to process 4 cells simultaneously when possible.
    #[inline(always)]
    pub fn xor_grids(&mut self, key_grid: &Grid<W, H>)
    {
        //CONVERT TO FLAT SLICES
        let self_data: &mut [i64] = unsafe
        {
            slice::from_raw_parts_mut(self.0.as_mut_ptr() as *mut i64, W * H)
        };

        let key_data: &[i64] = unsafe
        {
            slice::from_raw_parts(key_grid.0.as_ptr() as *const i64, W * H)
        };

        //SIMD LOOP (4xi64 AT ONCE [256 BITS])
        let mut chunks = self_data.chunks_exact_mut(4);
        let mut key_chunks = key_data.chunks_exact(4);

        for (self_chunk, key_chunk) in chunks.by_ref().zip(key_chunks.by_ref())
        {
            let mut self_arr = [0i64; 4];
            self_arr.copy_from_slice(self_chunk);
            let self_vec = i64x4::from(self_arr);

            let mut key_arr = [0i64; 4];
            key_arr.copy_from_slice(key_chunk);
            let key_vec = i64x4::from(key_arr);

            let result_vec = self_vec ^ key_vec;
            let result_arr: [i64; 4] = result_vec.into();
            self_chunk.copy_from_slice(&result_arr);
        }

        //SCALAR FALLBACK
        for (s, k) in chunks.into_remainder().iter_mut().zip(key_chunks.remainder())
        {
            *s ^= k;
        }
    }

    /// Applies nonlinear ARX-style mixing to each cell in the grid.
    ///
    /// This transformation introduces symmetric diffusion by modifying each `i64` cell
    /// using a combination of addition, rotation, and XOR operations. The process is
    /// round-dependent and designed to obscure bit patterns across the [`Grid`].
    ///
    /// # Parameters
    /// - `round`: A round index used to tweak the transformation logic.
    ///
    /// # Behavior
    /// Each 64-bit cell is split into two 32-bit halves $v_0, v_1$.
    /// For [`SUBCELL_ROUNDS`](crate::consts::SUBCELL_ROUNDS) iterations, the Feistel-like network applies:
    ///
    /// $$ v_0 \leftarrow v_0 + (((v_1 \ll 4) \oplus (v_1 \gg 5)) + v_1) \oplus \text{sum} $$
    ///
    /// $$ v_1 \leftarrow v_1 + (((v_0 \ll 4) \oplus (v_0 \gg 5)) + v_0) \oplus \text{sum} $$
    ///
    /// where $\text{sum}$ is incremented by a constant $\delta_{32} = $ [`DELTA_32`](crate::consts::DELTA_32) in each round:
    ///
    /// $$ \text{sum} \leftarrow \text{sum} + \delta_{32} $$
    ///
    /// # Implementation
    /// Four cells are transformed at a time in a 256-bit vector. The implementation is chosen at
    /// **run time**: on x86-64 with AVX2 an intrinsics path is used, and everything else falls
    /// back to a portable path. This matters because `wide`'s
    /// `i64x4` selects its backend from the *compile-time* target features, so a stock
    /// `cargo build --release` — which targets baseline `x86-64` — silently produced scalar
    /// code. Dispatching at run time is what makes a portable binary actually vectorised.
    ///
    /// # Notes
    /// - This method mutates the [`Grid`] in-place.
    /// - It is inspired by TEA/XTEA but adapted for WHY2's [`Grid`] architecture.
    /// - The transformation is deterministic for a given round and [`Grid`] state.
    #[inline(always)]
    pub fn subcell(&mut self, round: usize)
    {
        //DISPATCH ONCE PER CALL; THE VECTOR BODY NEEDS THE FEATURE ENABLED AT COMPILE TIME
        #[cfg(target_arch = "x86_64")]
        if gf::has_avx2()
        {
            unsafe { self.subcell_avx2(round) };
            return;
        }

        self.subcell_portable(round);
    }

    /// AVX2 implementation of [`subcell`](Self::subcell).
    ///
    /// Identical arithmetic to [`subcell_portable`](Self::subcell_portable), written against the
    /// intrinsics directly. `wide`'s `i64x4` picks its backend from the *compile-time* target
    /// features, so on a stock `cargo build --release` (baseline `x86-64`) it degrades to scalar
    /// code and the vectorisation the docs promise never happens. Selecting the implementation
    /// at run time is what makes it real on a portable binary.
    ///
    /// # Safety
    /// Requires `avx2`; reached only through [`gf::has_avx2`].
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    unsafe fn subcell_avx2(&mut self, round: usize)
    {
        use std::arch::x86_64::*;

        let data: &mut [i64] = unsafe
        {
            slice::from_raw_parts_mut(self.0.as_mut_ptr() as *mut i64, W * H)
        };

        let mask = _mm256_set1_epi64x(0xFFFF_FFFF);
        let delta = _mm256_set1_epi64x(consts::DELTA_32 as i64);
        let tweak = _mm256_set1_epi64x(round as i64);

        let mut chunks_iter = data.chunks_exact_mut(4);

        for chunk in &mut chunks_iter
        {
            unsafe
            {
                let x = _mm256_loadu_si256(chunk.as_ptr() as *const __m256i);

                //SPLIT CELL TO HIGH32 AND LOW32
                let mut v0 = _mm256_and_si256(x, mask);
                let mut v1 = _mm256_and_si256(_mm256_srli_epi64(x, 32), mask);
                let mut sum = _mm256_setzero_si256();

                //XOR TWEAK
                v0 = _mm256_and_si256(_mm256_xor_si256(v0, tweak), mask);

                //ARX-LIKE ROUNDS (INSPIRED BY XTEA/TEA)
                for _ in 0..consts::SUBCELL_ROUNDS
                {
                    sum = _mm256_add_epi64(sum, delta);

                    //MIX V1 INTO V0
                    let a = _mm256_xor_si256(_mm256_slli_epi64(v1, 4), _mm256_srli_epi64(v1, 5));
                    let a = _mm256_xor_si256(_mm256_add_epi64(a, v1), sum);
                    v0 = _mm256_and_si256(_mm256_add_epi64(v0, a), mask);

                    //MIX V0 INTO V1
                    let b = _mm256_xor_si256(_mm256_slli_epi64(v0, 4), _mm256_srli_epi64(v0, 5));
                    let b = _mm256_xor_si256(_mm256_add_epi64(b, v0), sum);
                    v1 = _mm256_and_si256(_mm256_add_epi64(v1, b), mask);
                }

                //XOR TWEAK
                v1 = _mm256_and_si256(_mm256_xor_si256(v1, tweak), mask);

                //RECONSTRUCT AND STORE
                let res = _mm256_or_si256(_mm256_slli_epi64(v1, 32), v0);
                _mm256_storeu_si256(chunk.as_mut_ptr() as *mut __m256i, res);
            }
        }

        //TAIL (UNREACHABLE FOR THE SUPPORTED HEIGHTS, WHICH ALL MAKE W * H A MULTIPLE OF 4)
        for cell in chunks_iter.into_remainder() { *cell = subcell_cell(*cell, round); }
    }

    /// Portable implementation of [`subcell`](Self::subcell), used when no vector backend applies.
    #[inline(always)]
    fn subcell_portable(&mut self, round: usize)
    {
        //CONVERT DATA TO i64 SLICE
        let data: &mut [i64] = unsafe
        {
            slice::from_raw_parts_mut(self.0.as_mut_ptr() as *mut i64, W * H)
        };

        //256-BIT AVX / 2x128-BiT NEON
        let mut chunks_iter = data.chunks_exact_mut(4);

        //SIMD LOOP
        let mask_simd = i64x4::splat(0xFFFF_FFFF); //LOW MASK FOR SIMD
        for chunk in &mut chunks_iter
        {
            //LOAD 4 i64 VALUES
            let x = i64x4::new([chunk[0], chunk[1], chunk[2], chunk[3]]);

            //SPLIT CELL TO HIGH32 AND LOW32
            let mut v0 = x & mask_simd; //LOW
            let mut v1 = (x >> 32) & mask_simd; //HIGH

            let mut sum = i64x4::ZERO;

            //MIX
            subcell!
            (
                v0,
                v1,
                sum,
                i64x4::splat(consts::DELTA_32 as i64),
                i64x4::splat(round as i64),
                mask_simd
            );

            //RECONSTRUCT AND STORE
            let res_vec: i64x4 = (v1 << 32) | v0;
            let res_arr: [i64; 4] = res_vec.into();
            chunk.copy_from_slice(&res_arr);
        }

        //SCALAR FALLBACK (WHEN (W * H) % 4 != 0)
        for cell in chunks_iter.into_remainder() { *cell = subcell_cell(*cell, round); }
    }

    /// Precomputes row shift amounts from the current Grid state.
    ///
    /// This function derives a deterministic shift value for each row by XOR-folding
    /// all elements in that row and applying a modulo operation. The resulting array
    /// can be reused across multiple rounds or operations without redundant computation.
    ///
    /// # Algorithm
    /// For each row $i$, the shift amount $S_i$ is computed as:
    ///
    /// $$ H_i = \bigoplus_{j=0}^{W-1} G_{i,j} $$
    /// $$ S_i = \left\lfloor \frac{H_i \cdot W}{2^{64}} \right\rfloor $$
    ///
    /// where $G_{i,j}$ represents the cell at row $i$, column $j$.
    ///
    /// # Returns
    /// An array of length $H$ containing shift amounts in the range $[0, W)$ for each row.
    ///
    /// # Security Notes
    /// - The fixed-point scaling above is the *only* mapping this function has, in every
    ///   feature configuration.
    /// - The multiply-high is inherently constant time: it is a single instruction on
    ///   every target the crate supports, with no data-dependent path.
    /// - The XOR-fold ensures each row's shift is influenced by all cells in that row.
    /// - Output shifts are deterministic for a given Grid state.
    ///
    /// # Performance
    /// This function should be called once per round key, not per grid, to avoid
    /// redundant computation. The precomputed shifts can be reused for all grids
    /// in a single encryption/decryption round.
    #[inline(always)]
    pub fn precalculate_shifts(&self) -> [usize; H]
    {
        let mut shifts = [0usize; H];

        //SHIFT EACH ROW
        for (i, row) in self.iter().enumerate()
        {
            //XOR-FOLD THE ROW, THEN SCALE THE FOLD INTO [0, W) BY TAKING THE HIGH HALF OF
            //THE WIDENING PRODUCT
            let hash_chunk = row.iter().fold(0i64, |acc, &x| acc ^ x);

            shifts[i] = ((hash_chunk as u64 as u128 * W as u128) >> 64) as usize;
        }

        shifts
    }

    /// Applies precomputed row-wise shifting to the Grid.
    ///
    /// This transformation rotates each row of the Grid left by a precalculated amount,
    /// providing horizontal diffusion.
    ///
    /// # Algorithm
    /// For each row $i$, apply a left rotation by shift amount $S_i$:
    ///
    /// $$ R'_i = \text{RotateLeft}(R_i, S_i) $$
    ///
    /// where $R_i$ is the original row and $R'_i$ is the transformed row.
    ///
    /// # Parameters
    /// - `shifts`: A precomputed array of shift amounts for each row, typically obtained
    ///   from [`precalculate_shifts`](Self::precalculate_shifts) called on a round key Grid.
    ///
    /// # Security Notes
    /// - The constant-time implementation prevents side-channel attacks via memory access patterns.
    /// - Shift amounts must come from a cryptographically secure source (e.g., round keys).
    /// - This operation is reversible if shift amounts are known.
    ///
    /// # Notes
    /// - This method mutates the Grid in-place.
    /// - The shifts array must have exactly $H$ elements.
    /// - All shift values must be in the range $[0, W)$.
    #[inline(always)]
    pub fn shift_rows(&mut self, shifts: &[usize; H])
    {
        //SHIFT EACH ROW
        for (i, row) in self.iter_mut().enumerate()
        {
            #[cfg(not(feature = "constant-time"))]
            if shifts[i] == 0 { continue; }

            //ROTATE THE ROW
            #[cfg(feature = "constant-time")]
            {
                let shift = shifts[i];
                let mut tmp = *row;
                let mut stride = 1usize;
                let mut bit = 0usize;

                //BARREL-SHIFTER
                while stride < W
                {
                    //ALL-ONES WHEN THIS STAGE ROTATES, ALL-ZEROES WHEN IT DOES NOT.
                    //`Choice::unwrap_u8` CARRIES subtle'S OPTIMISATION BARRIER, SO THE SELECT
                    //BELOW CANNOT BE TURNED BACK INTO A BRANCH. ONE BARRIER PER STAGE IS
                    //ENOUGH; THE PREVIOUS CODE PAID FOR ONE PER CELL AND BUILT A SECOND
                    //SCRATCH ROW TO SELECT FROM.
                    let mask = 0i64.wrapping_sub(Choice::from(((shift >> bit) & 1) as u8).unwrap_u8() as i64);

                    //THE ROTATION SOURCE IS TWO CONTIGUOUS RUNS, NOT A GATHER: FOR
                    //j < W - stride IT IS tmp[j + stride], AND FOR THE TAIL IT WRAPS TO
                    //tmp[j + stride - W]. WRITTEN AS ONE `% W` THE COMPILER CANNOT VECTORISE
                    //THE SELECT; SPLIT LIKE THIS IT CAN. BOTH BOUNDS COME FROM THE PUBLIC
                    //LOOP COUNTERS, SO NOTHING SECRET REACHES AN INDEX.
                    let mut rotated = [0i64; W];
                    let split = W - stride;

                    for j in 0..split
                    {
                        rotated[j] = tmp[j] ^ ((tmp[j] ^ tmp[j + stride]) & mask);
                    }

                    for j in split..W
                    {
                        rotated[j] = tmp[j] ^ ((tmp[j] ^ tmp[j - split]) & mask);
                    }

                    tmp = rotated;

                    bit += 1;
                    stride <<= 1;
                }

                *row = tmp;
            }

            #[cfg(not(feature = "constant-time"))]
            {
                row.rotate_left(shifts[i]);
            }
        }
    }

    /// Applies column-wise mixing using a fixed involutory MDS matrix over
    /// $\mathbb{F}_{2^{64}}$.
    ///
    /// This transformation provides vertical diffusion by treating each column as a vector
    /// of elements in $\mathbb{F}_{2^{64}}$ and multiplying it by a fixed MDS matrix.
    /// The matrix is **independent of the round key**, which is what makes the layer formally
    /// analyzable.
    ///
    /// # Algorithm
    /// For each column $c$, the output vector is computed as:
    ///
    /// $$ \text{out}\[r\] = \sum_{k=0}^{H-1} M\[r\]\[k\] \cdot \text{col}\[k\] $$
    ///
    /// Multiplication is modulo the irreducible polynomial
    /// $p(x) = x^{64} + x^4 + x^3 + x + 1$ in $\mathbb{F}_{2^{64}}$; addition is XOR.
    ///
    /// # Security Properties
    /// - **True MDS**: Branch number is provably $H + 1$ — the theoretical maximum.
    ///   Any nonzero input with $k$ nonzero elements produces an output with at least $H+1-k$
    ///   nonzero elements. Verified exhaustively: every square submatrix of every matrix in
    ///   [`consts`] is non-singular.
    /// - **Formally analyzable**: Fixed matrix enables standard differential/linear cryptanalysis bounds.
    /// - **Involutory**: $M^2 = I$, so this function is its own inverse — applying it twice is
    ///   the identity, and undoing the layer costs exactly what applying it does. Enforced at
    ///   compile time. CTR mode never needs it: the keystream is generated the same way in both
    ///   directions, so the cipher only ever runs the permutation forwards.
    /// - **Nothing-up-my-sleeve**: each matrix is the lexicographically smallest one meeting
    ///   those conditions at minimal weight, not a value picked out of a large set of equals.
    ///
    /// # Implementation
    /// Two things make this far cheaper than the $H^2$ field multiplications the definition
    /// asks for, neither of which changes the result:
    ///
    /// 1. **The matrix is Hadamard.** $M_{i,j}$ depends only on $i \oplus j$ (a compile-time
    ///    assertion enforces it), so one row is the entire matrix and the product is an
    ///    XOR-convolution.
    /// 2. **The coefficients are sums of small powers of $x$.** A product is therefore a short
    ///    chain of `xtime` steps — shift up, fold the carry — and an XOR, not a carry-less
    ///    multiply and a reduction. The whole layer is shifts and XORs, so it runs at the same
    ///    speed on every machine rather than falling off a cliff on one without `PCLMULQDQ`,
    ///    and the only thing left to dispatch on is register width.
    ///
    /// # Notes
    /// - This method mutates the grid in-place.
    /// - Grid heights outside the supported set will panic.
    #[inline(always)]
    pub fn mix_columns(&mut self)
    {
        #[cfg(target_arch = "x86_64")]
        if gf::has_avx2()
        {
            unsafe { self.mix_columns_avx2() };
            return;
        }

        self.mix_columns_inner();
    }

    /// AVX2 entry point for [`mix_columns`](Self::mix_columns).
    ///
    /// The body is nothing but 64-bit shifts and XORs over [`gf::LANE`]-wide arrays; all this
    /// adds is the register width to fold them into.
    ///
    /// # Safety
    /// Only reachable when [`gf::has_avx2`] reports true.
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    unsafe fn mix_columns_avx2(&mut self)
    {
        self.mix_columns_inner();
    }

    /// Body of [`mix_columns`](Self::mix_columns).
    ///
    /// Columns are handled [`gf::LANE`] at a time, so the scratch space is bounded by the grid
    /// *height* alone and does not grow with `W`.
    #[inline(always)]
    fn mix_columns_inner(&mut self)
    {
        let mut col = 0;
        while col < W
        {
            let take = if W - col < gf::LANE { W - col } else { gf::LANE };

            //GATHER A LANE OF COLUMNS (THE TAIL IS ZERO-PADDED, WHICH MIXES TO ZERO)
            let mut x = [[0u64; gf::LANE]; 16];
            for k in 0..H
            {
                for c in 0..take { x[k][c] = self.0[k][col + c] as u64; }
            }

            let mut out = [[0u64; gf::LANE]; 16];

            match H
            {
                4  => out[..4].copy_from_slice(&mix_4((&x[..4]).try_into().unwrap())),
                8  => out[..8].copy_from_slice(&mix_8((&x[..8]).try_into().unwrap())),
                16 => out = mix_16(&x),
                _  => unreachable!("tf")
            }

            //SCATTER BACK
            for k in 0..H
            {
                for c in 0..take { self.0[k][col + c] = out[k][c] as i64; }
            }

            col += take;
        }
    }

    //UTILS
    /// Increments the [`Grid`] value by a specified amount, treating it as a large Little-Endian integer.
    ///
    /// This method performs modular addition of a 64-bit value to the multi-precision integer
    /// represented by the grid:
    ///
    /// $$ G \leftarrow (G + \text{amount}) \bmod 2^{64 \times W \times H} $$
    ///
    /// # Parameters
    /// - `amount`: The unsigned 64-bit value to add to the grid.
    ///   - Pass `1` for standard sequential counter increment.
    ///   - Pass a block index $i$ (offset) when initializing parallel CTR counters.
    ///
    /// # Behavior
    /// - The [`Grid`] is treated as a single large integer in **Little-Endian** format
    ///   (the cell at `[0][0]` is the least significant limb).
    /// - The `amount` is added to the first cell, and any resulting carry is propagated
    ///   sequentially through the remaining cells.
    /// - If the entire grid overflows (wraps around), the value resets modulo the grid size.
    ///
    /// # Security
    /// - When the **`constant-time`** feature is enabled, this function always iterates
    ///   through the entire grid to prevent timing leaks via carry propagation analysis.
    #[inline(always)]
    pub fn increment(&mut self, amount: u64)
    {
        //FLATTEN
        let data: &mut [i64] = unsafe
        {
            slice::from_raw_parts_mut(self.0.as_mut_ptr() as *mut i64, W * H)
        };

        let mut carry = amount;
        for cell in data.iter_mut()
        {
            let (result, overflow) = (*cell as u64).overflowing_add(carry);
            *cell = result as i64;

            //NO CARRY (OVERFLOW), DONE
            #[cfg(not(feature = "constant-time"))]
            {
                if !overflow { return; }
                carry = 1;
            }

            #[cfg(feature = "constant-time")]
            {
                carry = overflow as u64;
            }
        }
    }
}

//INTO ITERATOR
impl<const W: usize, const H: usize> IntoIterator for Grid<W, H>
{
    //TYPES
    type Item = i64;
    type IntoIter = Flatten<IntoIter<[i64; W], H>>;

    //INTO ITERATOR
    #[inline]
    fn into_iter(self) -> Self::IntoIter
    {
        self.0.into_iter().flatten()
    }
}

//INDEXING
impl<const W: usize, const H: usize> Index<usize> for Grid<W, H>
{
    type Output = [i64; W];

    #[inline]
    fn index(&self, y: usize) -> &Self::Output
    {
        &self.0[y]
    }
}

//MUTABLE INDEXING
impl<const W: usize, const H: usize> IndexMut<usize> for Grid<W, H>
{
    #[inline]
    fn index_mut(&mut self, y: usize) -> &mut Self::Output
    {
        &mut self.0[y]
    }
}

//XOR ASSIGN
impl<const W: usize, const H: usize> BitXorAssign<&Grid<W, H>> for Grid<W, H>
{
    #[inline]
    fn bitxor_assign(&mut self, rhs: &Grid<W, H>)
    {
        self.xor_grids(rhs);
    }
}

//CONSTANT-TIME EQ
#[cfg(feature = "constant-time")]
impl<const W: usize, const H: usize> ConstantTimeEq for Grid<W, H>
{
    fn ct_eq(&self, other: &Self) -> Choice
    {
        let mut result = Choice::from(1);

        for (row_a, row_b) in self.iter().zip(other.iter())
        {
            for (cell_a, cell_b) in row_a.iter().zip(row_b.iter())
            {
                result &= cell_a.ct_eq(cell_b);
            }
        }

        result
    }
}

impl<const W: usize, const H: usize> PartialEq for Grid<W, H>
{
    fn eq(&self, other: &Self) -> bool
    {
        #[cfg(feature = "constant-time")]
        {
            self.ct_eq(other).into()
        }

        #[cfg(not(feature = "constant-time"))]
        {
            self.0 == other.0
        }
    }
}

//DISPLAY
impl<const W: usize, const H: usize> Display for Grid<W, H>
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result
    {
        //CONVERT EACH VALUE TO 4 LINES
        let cells: Vec<Vec<[String; 4]>> = self.iter().map(|row|
        {
            row.iter().map(|val|
            {
                let s = val.to_string();
                let chunk_size = (s.len() + 3) / 4;
                let mut lines = [String::new(), String::new(), String::new(), String::new()];

                for (i, chunk) in s.chars().collect::<Vec<_>>().chunks(chunk_size).enumerate()
                {
                    lines[i] = chunk.iter().collect();
                }

                lines
            }).collect()
        }).collect();

        //DETERMINE MAX WIDTH
        let max_width = cells.iter()
            .flat_map(|row| row.iter())
            .flat_map(|lines| lines.iter())
            .map(|line| line.len())
            .max()
            .unwrap_or(1);

        //BUILD HORIZONTAL BORDER
        let border = format!
        (
            "+{}+\n",
            (0..self.width()).map(|_| "-".repeat(max_width + 2)).collect::<Vec<_>>().join("+")
        );

        //PRINT
        for row in &cells
        {
            f.write_str(&border)?;
            for line_idx in 0..4
            {
                for cell in row
                {
                    write!(f, "| {:>width$} ", cell[line_idx], width = max_width)?;
                }

                writeln!(f, "|")?;
            }
        }

        f.write_str(&border)
    }
}

impl<const W: usize, const H: usize> LowerHex for Grid<W, H>
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result
    {
        for row in self.iter()
        {
            for cell in row
            {
                write!(f, "{:016x}", cell)?;
            }
        }

        Ok(())
    }
}

impl Display for GridError
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result
    {
        match self
        {
            GridError::InvalidDimensions { width, height } =>
            {
                if *width == 0
                {
                    write!(f, "Invalid dimensions: expected width larger than 0")
                } else
                {
                    write!(f, "Invalid dimensions: expected height is 4, 8 or 16, got {height}")
                }
            },

            GridError::InvalidByteLength { expected_mod, actual_len } =>
            {
                write!(f, "Invalid byte length: expected multiple of {expected_mod} bytes for this Grid, got {actual_len}")
            },

            GridError::InvalidKeyLength { expected_len, actual_len } =>
            {
                write!(f, "Invalid key length: expected length {expected_len}, got {actual_len}")
            },

            GridError::InvalidUnicode { value } =>
            {
                write!(f, "Invalid unicode scalar value: {value:#X} (possible wrong key)")
            },
        }
    }
}

impl Error for GridError {}
