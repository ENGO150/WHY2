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

use crate::consts::{ self, mds };

#[cfg(feature = "constant-time")]
use subtle::
{
    Choice,
    ConstantTimeEq,
    ConditionallySelectable,
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

//PRIVATE HELPERS FOR FINITE FIELD ARITHMETICS
#[inline(always)]
fn gf_mul(a: u64, b: u64) -> u64
{
    gf_mul2(a, b, 0, 0).0
}

#[inline(always)]
fn gf_mul2(a0: u64, b0: u64, a1: u64, b1: u64) -> (u64, u64)
{
    #[cfg(target_arch = "x86_64")]
    if is_x86_feature_detected!("pclmulqdq")
    {
        return unsafe
        {
            use std::arch::x86_64::*;

            let a_vec = _mm_set_epi64x(a1 as i64, a0 as i64);
            let b_vec = _mm_set_epi64x(b1 as i64, b0 as i64);

            let lo_vec = _mm_clmulepi64_si128(a_vec, b_vec, 0x00); // a0 * b0
            let hi_vec = _mm_clmulepi64_si128(a_vec, b_vec, 0x11); // a1 * b1

            let lo0 = _mm_extract_epi64(lo_vec, 0) as u64;
            let hi0 = _mm_extract_epi64(lo_vec, 1) as u64;
            let lo1 = _mm_extract_epi64(hi_vec, 0) as u64;
            let hi1 = _mm_extract_epi64(hi_vec, 1) as u64;

            (gf_reduce(lo0, hi0), gf_reduce(lo1, hi1))
        };
    }

    #[cfg(target_arch = "aarch64")]
    return unsafe
    {
        use std::arch::aarch64::*;

        let a_vec = vcombine_u64(vcreate_u64(a0), vcreate_u64(a1));
        let b_vec = vcombine_u64(vcreate_u64(b0), vcreate_u64(b1));

        let lo_vec = vmull_p64
        (
            vgetq_lane_u64(a_vec, 0),
            vgetq_lane_u64(b_vec, 0)
        );

        let hi_vec = vmull_high_p64
        (
            vreinterpretq_p64_u64(a_vec),
            vreinterpretq_p64_u64(b_vec)
        );

        let lo0 = vgetq_lane_u64(vreinterpretq_u64_p128(lo_vec), 0);
        let hi0 = vgetq_lane_u64(vreinterpretq_u64_p128(lo_vec), 1);
        let lo1 = vgetq_lane_u64(vreinterpretq_u64_p128(hi_vec), 0);
        let hi1 = vgetq_lane_u64(vreinterpretq_u64_p128(hi_vec), 1);

        (gf_reduce(lo0, hi0), gf_reduce(lo1, hi1))
    };

    //SW FALLBACK
    #[cfg(not(target_arch = "aarch64"))]
    (gf_mul_soft(a0, b0), gf_mul_soft(a1, b1))
}

#[inline(always)]
fn gf_mul_const2(a0: u64, a1: u64, coeff: u64) -> (u64, u64)
{
    #[cfg(target_arch = "x86_64")]
    if is_x86_feature_detected!("pclmulqdq")
    {
        return unsafe
        {
            use std::arch::x86_64::*;

            let a_vec = _mm_set_epi64x(a1 as i64, a0 as i64);
            let b_vec = _mm_set1_epi64x(coeff as i64); //BROADCAST

            let lo_vec = _mm_clmulepi64_si128(a_vec, b_vec, 0x00); // a0 * coeff
            let hi_vec = _mm_clmulepi64_si128(a_vec, b_vec, 0x11); // a1 * coeff

            let lo0 = _mm_extract_epi64(lo_vec, 0) as u64;
            let hi0 = _mm_extract_epi64(lo_vec, 1) as u64;
            let lo1 = _mm_extract_epi64(hi_vec, 0) as u64;
            let hi1 = _mm_extract_epi64(hi_vec, 1) as u64;

            (gf_reduce(lo0, hi0), gf_reduce(lo1, hi1))
        };
    }

    #[cfg(target_arch = "aarch64")]
    return unsafe
    {
        use std::arch::aarch64::*;

        let a_vec = vcombine_u64(vcreate_u64(a0), vcreate_u64(a1));
        let b_vec = vdupq_n_u64(coeff); //BROADCAST

        let lo_vec = vmull_p64
        (
            vgetq_lane_u64(a_vec, 0),
            vgetq_lane_u64(b_vec, 0)
        );

        let hi_vec = vmull_high_p64
        (
            vreinterpretq_p64_u64(a_vec),
            vreinterpretq_p64_u64(b_vec)
        );

        let lo0 = vgetq_lane_u64(vreinterpretq_u64_p128(lo_vec), 0);
        let hi0 = vgetq_lane_u64(vreinterpretq_u64_p128(lo_vec), 1);
        let lo1 = vgetq_lane_u64(vreinterpretq_u64_p128(hi_vec), 0);
        let hi1 = vgetq_lane_u64(vreinterpretq_u64_p128(hi_vec), 1);

        (gf_reduce(lo0, hi0), gf_reduce(lo1, hi1))
    };

    #[cfg(not(target_arch = "aarch64"))]
    (gf_mul_soft(a0, coeff), gf_mul_soft(a1, coeff))
}

#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
fn gf_mul_soft(a: u64, b: u64) -> u64
{
    let mut result: u64 = 0;
    let mut a = a;
    let mut b = b;
    let poly = 0x1Bu64;

    for _ in 0..64
    {
        if b & 1 == 1 { result ^= a; }
        let carry = (a >> 63) & 1;
        a <<= 1;
        if carry == 1 { a ^= poly; }
        b >>= 1;
    }

    result
}

#[inline(always)]
fn gf_reduce(lo: u64, hi: u64) -> u64
{
    let mid = (hi << 4) ^ (hi << 3) ^ (hi << 1) ^ hi;
    let overflow = (hi >> 60) ^ (hi >> 61) ^ (hi >> 63);
    let extra = (overflow << 4) ^ (overflow << 3) ^ (overflow << 1) ^ overflow;

    lo ^ mid ^ extra
}

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
    /// Each cell is built from two key parts using nonlinear mixing.
    /// addition, XOR, and rotation. This improves diffusion and avoids
    /// simple linear patterns in the key.
    ///
    /// # Algorithm
    /// For each cell index $i$, the key parts $A$ and $B$ are derived from the input vector $V$:
    ///
    /// $$ A = (V_i + V_{i + \text{Area}}) \lll (i \bmod 64) $$
    ///
    /// $$ B = (V_i \oplus V_{i + \text{Area}}) \ggg (i \bmod 64) $$
    ///
    /// where $\lll$ and $\ggg$ denote left and right rotation respectively.
    ///
    /// The final grid value is computed as:
    /// $$ Grid_{x,y} = A \oplus B \oplus i $$
    ///
    /// # Parameters
    /// - `vec`: A slice of signed 64-bit integers representing the raw key.
    ///
    /// # Returns
    /// - Ok(`Grid`) with mixed key values if dimensions are valid.
    /// - Err(`GridError`) if the grid area is too small.
    pub fn from_key(vec: &[i64]) -> result::Result<Self, GridError>
    {
        //GRID OPTIONS
        let grid_area = W * H;

        //SHAPE
        let mut key_grid = Self::new()?;
        for i in 0..grid_area
        {
            //APPLY NONLINEAR MIX TO KEY
            let mut a = vec[i].wrapping_add(vec[i + grid_area]);
            let mut b = vec[i] ^ vec[i + grid_area];
            let rot = (i & 63) as u32;

            //ROTATE
            a = a.rotate_left(rot);
            b = b.rotate_right(rot);

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
    /// The function uses SIMD acceleration via 256-bit AVX2 (or 2×128-bit NEON) vector operations
    /// to process 4 cells simultaneously. Remaining cells are handled with a scalar fallback.
    ///
    /// # Notes
    /// - This method mutates the [`Grid`] in-place.
    /// - It is inspired by TEA/XTEA but adapted for WHY2's [`Grid`] architecture.
    /// - The transformation is deterministic for a given round and [`Grid`] state.
    /// - SIMD implementation provides 2.5-4× speedup on modern CPUs compared to scalar code.
    #[inline(always)]
    pub fn subcell(&mut self, round: usize)
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
        let mask_scalar = 0xFFFF_FFFF;
        for cell in chunks_iter.into_remainder()
        {
            //SPLIT CELL TO HIGH32 AND LOW32
            let x = *cell as u64;
            let mut v0 = (x & mask_scalar) as u32;
            let mut v1 = ((x >> 32) & mask_scalar) as u32;

            let mut sum = 0u32;

            //MIX
            subcell!
            (
                v0,
                v1,
                sum,
                consts::DELTA_32,
                round as u32,
                mask_scalar as u32
            );

            //RECONSTRUCT AND STORE
            *cell = (((v1 as u64) << 32) | (v0 as u64)) as i64;
        }
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
    ///
    /// **Constant-time variant:**
    /// $$ S_i = \left\lfloor \frac{H_i \cdot W}{2^{64}} \right\rfloor $$
    ///
    /// **Non-constant-time variant:**
    /// $$ S_i = H_i \bmod W $$
    ///
    /// where $G_{i,j}$ represents the cell at row $i$, column $j$.
    ///
    /// # Returns
    /// An array of length $H$ containing shift amounts in the range $[0, W)$ for each row.
    ///
    /// # Security Notes
    /// - The constant-time variant uses Barrett reduction to prevent timing attacks.
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
            let hash_chunk = row.iter().fold(0i64, |acc, &x| acc ^ x);

            #[cfg(feature = "constant-time")]
            {
                shifts[i] = ((hash_chunk as u64 as u128 * W as u128) >> 64) as usize;
            }

            #[cfg(not(feature = "constant-time"))]
            {
                //SPLIT key_grid TO 8 PARTS & XOR EACH VALUE TO GET SHIFT
                shifts[i] = hash_chunk.rem_euclid(W as i64) as usize;
            }
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
                    let do_rotate = Choice::from(((shift >> bit) & 1) as u8);
                    let mut rotated = [0i64; W];

                    for j in 0..W
                    {
                        rotated[j] = tmp[(j + stride) % W];
                    }

                    for j in 0..W
                    {
                        tmp[j].conditional_assign(&rotated[j], do_rotate);
                    }

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

    /// Applies column-wise mixing using a fixed Cauchy MDS matrix over $\mathbb{F}_{2^{64}}$.
    ///
    /// This transformation provides vertical diffusion by treating each column as a vector
    /// of elements in $\mathbb{F}_{2^{64}}$ and multiplying it by a precomputed Cauchy MDS matrix.
    /// Unlike the previous implementation, the matrix is **fixed and independent of the round key**,
    /// enabling formal cryptographic analysis.
    ///
    /// # Algorithm
    /// For each column $c$, the output vector is computed as:
    ///
    /// $$ \text{out}\[r\] = \sum_{k=0}^{H-1} M\[r\]\[k\] \cdot \text{col}\[k\] $$
    ///
    /// Multiplication uses carry-less multiplication modulo the irreducible polynomial
    /// $p(x) = x^{64} + x^4 + x^3 + x + 1$ in $\mathbb{F}_{2^{64}}$.
    /// Addition corresponds to XOR.
    ///
    /// The matrix $M$ is a Cauchy MDS matrix constructed from disjoint sets
    /// $x = \{0, \ldots, H-1\}$ and $y = \{H, \ldots, 2H-1\}$ over $\mathbb{F}_{2^{64}}$:
    ///
    /// $$ M_{ij} = (x_i \oplus y_j)^{-1} $$
    ///
    /// # Security Properties
    /// - **True MDS**: Branch number is provably $H + 1$ — the theoretical maximum.
    ///   Any nonzero input with $k$ nonzero elements produces an output with at least $H+1-k$
    ///   nonzero elements.
    /// - **Formally analyzable**: Fixed matrix enables standard differential/linear cryptanalysis bounds.
    /// - **Nothing-up-my-sleeve**: Matrix derived from smallest possible disjoint sets,
    ///   demonstrating no hidden weaknesses.
    ///
    /// # Implementation
    /// Uses hardware-accelerated carry-less multiplication:
    /// - **x86_64**: `PCLMULQDQ` instruction, processing 2 multiplications per instruction.
    /// - **AArch64**: `PMULL`/`PMULL2` instructions, processing 2 multiplications per instruction.
    /// - **Fallback**: Software implementation for other architectures.
    ///
    /// # Supported Grid Heights
    /// - $H = 4$: Uses [`MDS_4`](mds::MDS_4)
    /// - $H = 8$: Uses [`MDS_8`](mds::MDS_8)
    /// - $H = 16$: Uses [`MDS_16`](mds::MDS_16)
    ///
    /// # Notes
    /// - This method mutates the grid in-place.
    /// - Grid heights outside the supported set will panic.
    #[inline(always)]
    pub fn mix_columns(&mut self)
    {
        let mut out = [[0u64; W]; H];

        for k in 0..H
        {
            for r in 0..H
            {
                //PICK CORRECT MATRIX (AND ITS COEFFICIENT)
                let coeff = match H
                {
                    4  => mds::MDS_4[r][k],
                    8  => mds::MDS_8[r][k],
                    16 => mds::MDS_16[r][k],
                    _  => unreachable!("tf")
                };

                let acc = &mut out[r];
                let src = &self.0[k]; // ROW-MAJOR READ, SEQUENTIAL
                let mut c = 0usize;

                //PROCESS 4 COLUMNS AT ONCE
                while c + 3 < W
                {
                    let (p0, p1) = gf_mul_const2(src[c] as u64, src[c + 1] as u64, coeff);
                    let (p2, p3) = gf_mul_const2(src[c + 2] as u64, src[c + 3] as u64, coeff);

                    acc[c]     ^= p0;
                    acc[c + 1] ^= p1;
                    acc[c + 2] ^= p2;
                    acc[c + 3] ^= p3;

                    c += 4;
                }

                while c + 1 < W
                {
                    let (p0, p1) = gf_mul_const2(src[c] as u64, src[c + 1] as u64, coeff);

                    acc[c]     ^= p0;
                    acc[c + 1] ^= p1;

                    c += 2;
                }

                //REMAINDER
                if c < W
                {
                    acc[c] ^= gf_mul(src[c] as u64, coeff);
                }
            }
        }

        //SAVE RESULT
        for r in 0..H
        {
            for c in 0..W
            {
                self[r][c] = out[r][c] as i64;
            }
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
