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
//! This module implements all arithmetic and logical transformations applied to the state.
//! The logic follows a defined SPN (Substitution-Permutation Network) structure:
//! - **Nonlinear Mixing**: ARX-based [`subcell`](Grid::subcell) operations acting as a variable S-box.
//! - **Row Permutation**: Cyclical row shifting via [`shift_rows`](Grid::shift_rows) for horizontal diffusion.
//! - **Column Diffusion**: MDS-based mixing via [`mix_columns`](Grid::mix_columns) for vertical diffusion and high avalanche effect.
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
    slice::{ Iter, IterMut },
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

#[cfg(feature = "constant-time")]
use subtle::
{
    Choice,
    ConstantTimeEq,
    ConditionallySelectable,
};

use crate::consts;

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
    /// - Ok(`Grid`) instance with all values set to zero and area is larger than 1.
    /// - Err(`GridError`) if the area is 1
    ///
    /// # Notes
    /// - This method does not perform any encryption or transformation.
    #[inline]
    pub fn new() -> result::Result<Self, GridError>
    {
        let area = W * H;
        if area > 1 && W > 1
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
            let rot = (i % 64) as u32;

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

        bytes.chunks(matrix_size).map(|chunk|
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
    #[inline]
    pub fn iter(&self) -> Iter<'_, [i64; W]>
    {
        self.0.iter()
    }

    /// Returns a mutable iterator over rows in the Grid
    #[inline]
    pub fn iter_mut(&mut self) -> IterMut<'_, [i64; W]>
    {
        self.0.iter_mut()
    }

    /// Returns width (number of columns) in the Grid
    #[inline]
    pub fn width(&self) -> usize
    {
        W
    }

    /// Returns height (number of rows) in the Grid
    #[inline]
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
    #[inline(always)]
    pub fn xor_grids(&mut self, key_grid: &Grid<W, H>)
    {
        self.0.iter_mut().flatten()
            .zip(key_grid.0.iter().flatten())
            .for_each(|(a, b)| *a ^= *b);
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
    /// # Notes
    /// - This method mutates the [`Grid`] in-place.
    /// - It is inspired by TEA/XTEA but adapted for WHY2’s [`Grid`] architecture.
    /// - The transformation is deterministic for a given round and [`Grid`] state.
    #[inline(always)]
    pub fn subcell(&mut self, round: usize)
    {
        //APPLY ON EACH CELL
        for col in self.iter_mut()
        {
            for cell in col
            {
                //SPLIT CELL TO HIGH32 AND LOW32
                let x = *cell as u64;
                let mut v0 = (x & 0xFFFF_FFFF) as u32; //LOW
                let mut v1 = ((x >> 32) & 0xFFFF_FFFF) as u32; //HIGH

                //XOR TWEAK -> MAKE ROUNDS DIFFERENT
                v0 ^= round as u32;

                //ARX-LIKE ROUNDS (INSPIRED BY XTEA/TEA)
                let mut sum: u32 = 0;
                for _ in 0..(consts::SUBCELL_ROUNDS)
                {
                    sum = sum.wrapping_add(consts::DELTA_32);

                    //MIX V1 INTO V0
                    v0 = v0.wrapping_add(((v1 << 4) ^ (v1 >> 5)).wrapping_add(v1) ^ sum);

                    //MIX V0 INTO V1
                    v1 = v1.wrapping_add(((v0 << 4) ^ (v0 >> 5)).wrapping_add(v0) ^ sum);
                }

                //XOR TWEAK
                v1 ^= round as u32;

                //REBUILD AND APPLY
                let out = ((v1 as u64) << 32) | (v0 as u64);
                *cell = out as i64;
            }
        }
    }

    /// Applies row-wise shifting to the [`Grid`] based on a key [`Grid`].
    ///
    /// This transformation rotates each row of the [`Grid`] by a variable amount derived from
    /// the corresponding row in `key_grid`. The shift amount $S_i$ for row $i$ is computed as:
    ///
    /// $$ S_i = \left( \bigoplus_{j=0}^{W-1} K_{i,j} \right) \bmod W $$
    ///
    /// # Behavior
    /// - Each row is rotated left by $S_i$.
    ///
    /// # Notes
    /// - This method mutates the grid in-place.
    #[inline(always)]
    pub fn shift_rows(&mut self, key_grid: &Grid<W, H>)
    {
        #[cfg(not(feature = "constant-time"))]
        let rows = self.width() as i64;

        //SHIFT EACH ROW
        for (i, row) in self.iter_mut().enumerate()
        {
            let hash_chunk = key_grid[i].iter().fold(0i64, |acc, &x| acc ^ x);
            let shift: usize;

            #[cfg(feature = "constant-time")]
            {
                shift = ((hash_chunk as u64 as u128 * W as u128) >> 64) as usize;
            }

            #[cfg(not(feature = "constant-time"))]
            {
                //SPLIT key_grid TO 8 PARTS & XOR EACH VALUE TO GET SHIFT
                shift = hash_chunk.rem_euclid(rows) as usize;
            }

            //ROTATE THE ROW
            #[cfg(feature = "constant-time")]
            {
                let mut new_row = [0i64; W]; //BUFFER

                for i in 0..W
                {
                    let target_src_idx = (i + shift) % W;
                    for src_idx in 0..W
                    {
                        new_row[i].conditional_assign(&row[src_idx], src_idx.ct_eq(&target_src_idx));
                    }
                }

                //WRITE RESULT
                *row = new_row;
            }

            #[cfg(not(feature = "constant-time"))]
            {
                row.rotate_left(shift);
            }
        }
    }

    /// Applies column-wise mixing using a key-dependent MDS-like matrix transformation.
    ///
    /// This transformation provides vertical diffusion by treating each column as a vector
    /// and multiplying it by a circulating matrix of large odd coefficients. The matrix
    /// rotation is derived from both the round key and the round index, making each round
    /// structurally unique and preventing slide attacks.
    ///
    /// # Algorithm
    /// For each column $c$ in round $r$, compute a key-dependent rotation:
    /// $$ R_{c,r} = \left( \left(\sum_{i=0}^{H-1} K_{i,c} \right) \cdot c \cdot r \cdot \delta_{64} \right) \bmod |C| $$
    ///
    /// Where:
    /// - $K$ is the round key grid.
    /// - $c$ is the column index.
    /// - $r$ is the round index (0-based).
    /// - $\delta_{64}$ is the [`DELTA_64`](crate::consts::DELTA_64) constant.
    /// - $C$ are the constants defined in [`MC_COEFFICIENTS`](crate::consts::MC_COEFFICIENTS).
    ///
    /// The coefficient selection for matrix multiplication then becomes:
    /// $$ \text{coeff}\_\text{idx} = (k + \text{row} + R_{c,r}) \bmod |C| $$
    ///
    /// # Parameters
    /// - `round_index`: The current round number (0 to [`ROUND_KEYS`](crate::consts::ROUND_KEYS) - 1)
    /// - `key_grid`: The round key used to derive column-specific rotations
    ///
    /// # Security Properties
    /// - **MDS-like**: Near-optimal branch number for diffusion
    /// - **Key-dependent**: Each column uses a different coefficient rotation
    /// - **Round-variant**: Different rounds produce different transformations, preventing slide attacks
    /// - **Domain separation**: Round index ensures structural uniqueness across rounds
    ///
    /// # Notes
    /// - This method mutates the grid in-place.
    /// - The round index multiplication prevents related-key attacks by ensuring
    ///   that identical keys in different rounds still produce distinct transformations.
    #[inline(always)]
    pub fn mix_columns(&mut self, round_index: usize, key_grid: &Grid<W, H>)
    {
        //RESULT BUFFER
        let mut new_grid = [[0i64; W]; H];

        for col in 0..W
        {
            //DERIVE KEY-DEPENDENT ROTATION FOR THIS COLUMN
            let mut key_sum: i64 = 0;
            for row in 0..H
            {
                key_sum = key_sum.wrapping_add(key_grid[row][col]);
            }

            //MULTIPLY BY COLUMN INDEX TO ENSURE DIFFERENT ROTATION PER COLUMN
            //USE WRAPPING ARITHMETIC TO STAY IN i64 RANGE
            let rotation = ((key_sum as u64)
                .wrapping_mul(col as u64)
                .wrapping_mul(round_index as u64)
                .wrapping_mul(consts::DELTA_64)) as usize & (consts::MC_COEFFICIENTS.len() - 1);

            for row in 0..H
            {
                let mut sum: i64 = 0;

                for k in 0..H
                {
                    //USE ROTATED COEFFICIENTS BASED ON KEY
                    let coeff_idx = (k + row + rotation) & (consts::MC_COEFFICIENTS.len() - 1);

                    let coeff: i64;

                    #[cfg(feature = "constant-time")]
                    {
                        let mut selected = 0i64;
                        for (i, &val) in consts::MC_COEFFICIENTS.iter().enumerate()
                        {
                            selected.conditional_assign(&val, i.ct_eq(&coeff_idx));
                        }

                        coeff = selected;
                    }

                    #[cfg(not(feature = "constant-time"))]
                    {
                        coeff = consts::MC_COEFFICIENTS[coeff_idx];
                    }

                    //ACCUMULATE (MDS MIXING)
                    sum = sum.wrapping_add(self[k][col].wrapping_mul(coeff));
                }

                new_grid[row][col] = sum;
            }
        }

        //STORE RESULT
        self.0 = new_grid;
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
    pub fn increment(&mut self, amount: &mut u64)
    {
        for row in self.iter_mut()
        {
            for cell in row.iter_mut()
            {
                let (result, overflow) = (*cell as u64).overflowing_add(*amount);
                *cell = result as i64;

                //NO CARRY (OVERFLOW), DONE
                #[cfg(not(feature = "constant-time"))]
                {
                    if !overflow { return; }
                    *amount = 1;
                }

                #[cfg(feature = "constant-time")]
                {
                    *amount = overflow as u64;
                }
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
                if *width <= 1
                {
                    write!(f, "Invalid dimensions: expected width larger than 1, got {width}")
                } else
                {
                    write!(f, "Invalid dimensions: expected area larger than 1, got {width}x{height} ({})", width * height)
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
