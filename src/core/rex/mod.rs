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

//! # REX
//!
//! This module implements the core encryption logic behind WHY2 algorithm.
//!
//! ## Design Overview
//! - Input and key are formatted into 2D grids of 64-bit cells.
//! - The key grid is shuffled and seeded to generate round keys.
//! - Each round applies a nonlinear transformation to the input grids.
//! - The transofrmation avoid traditional S-boxes, relying instead on symmetric diffusion.
//! - Round tweaks ensure variability across rounds without requiring per-round constants.

//MODULES
pub mod crypto;
pub mod decrypter;
pub mod encrypter;
pub mod options;

use std::
{
    result,
    vec::IntoIter as IntoVecIter,
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
    },
};

//TYPES
/// A 2D matrix of 64-bit signed integers used as the core data structure in WHY2 encryption.
///
/// The `Grid` represents either input data or a key, formatted into rows and columns of `i64` cells.
/// All transformations—round mixing, key scheduling, and nonlinear diffusion—operate directly on this structure.
///
/// Grids are flexible and can be transformed in-place.
/// This abstraction allows WHY2 to generalize encryption over variable-sized blocks.
///
/// # Grid Size Consistency
///
/// WHY2 requires that the same grid dimensions (rows × columns) be used consistently
/// throughout encryption and decryption. Mixing grid sizes within a single session or
/// across rounds is unsupported and may lead to incorrect results or undefined behavior.

#[derive(Clone, Debug)]
pub struct Grid<const W: usize, const H: usize>([[i64; W]; H]); //GRID FOR REX DATA

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
    /// - Err(String) if the area is 1
    ///
    /// # Notes
    /// - This method does not perform any encryption or transformation.
    pub fn new() -> result::Result<Self, String>
    {
        let area = W * H;
        if area > 1 && W > 1
        {
            Ok(Self([[0i64; W]; H]))
        } else
        {
            Err(if W == 1
            {
                format!("Invalid dimensions: expected width larger than 1, got {W}")
            } else
            {
                format!("Invalid dimensions: expected area larger than 1, got {W}x{H} ({area})")
            })
        }
    }

    /// Initializes a key Grid from a vector of signed 64-bit integers.
    ///
    /// Each cell is built from two key parts using nonlinear mixing:
    /// addition, XOR, and rotation. This improves diffusion and avoids
    /// simple linear patterns in the key.
    ///
    /// # Parameters
    /// - `vec`: A vector of signed 64-bit integers representing the raw key.
    ///
    /// # Returns
    /// - Ok(`Grid`) with mixed key values if dimensions are valid.
    /// - Err(String) if the grid area is too small.
    pub fn from_key(vec: Vec<i64>) -> result::Result<Self, String>
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

    /// Initializes Grid from vector of unsigned 8-bit integers.
    ///
    /// This function constructs Grid by chunking the input vector into `i64` cells. It expects
    /// exactly `W × H × 8` bytes and returns an error if the input length does not match.
    ///
    /// # Parameters
    /// - `bytes`: A vector of unsigned 8-bit integers
    ///
    /// # Returns
    /// - `Ok(Grid)` if the byte length matches the expected grid size
    /// - `Err(String)` if the input length is not divisible by matrix size.
    ///
    /// # Notes
    /// - No transformation is applied
    /// - Use this for raw Grid construction, not for secure key loading
    pub fn from_bytes(bytes: Vec<u8>) -> result::Result<Vec<Self>, String>
    {
        let matrix_size = W * H * 8; //EACH i64 IS 8 BYTES

        //CHECK FOR VALID GRID
        if bytes.len() % matrix_size != 0
        {
            return Err(format!
            (
                "Invalid byte length: expected multiply of {} bytes for a {}x{} Grid, got {}",
                matrix_size, W, H, bytes.len()
            ));
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
    pub fn iter(&self) -> Iter<'_, [i64; W]>
    {
        self.0.iter()
    }

    /// Returns a mutable iterator over rows in the Grid
    pub fn iter_mut(&mut self) -> IterMut<'_, [i64; W]>
    {
        self.0.iter_mut()
    }

    /// Returns width (number of columns) in the Grid
    pub fn width(&self) -> usize
    {
        W
    }

    /// Returns height (number of rows) in the Grid
    pub fn height(&self) -> usize
    {
        H
    }

    //ENCRYPTION
    //PRIVATE
    fn shift_rows_handler(&mut self, key_grid: &Grid<W, H>, invert: bool) //SHIFT ROWS IN grid BASED ON key_grid
    {
        let rows = self.width() as i64; //ROWS IN grid & key_grid

        //SHIFT EACH ROW
        for (i, row) in self.iter_mut().enumerate()
        {
            //SPLIT key_grid TO 8 PARTS & XOR EACH VALUE TO GET SHIFT
            let shift = key_grid[i].iter().fold(0i64, |acc, &x| acc ^ x).rem_euclid(rows) as usize;

            //ROTATE THE ROW
            if invert
            {
                row.rotate_right(shift); //RIGHT ON DECRYPTION
            } else
            {
                row.rotate_left(shift); //LEFT ON ENCRYPTION
            }
        }
    }

    fn mix_columns_handler(&mut self, invert: bool) //MIX COLUMNS IN grid GRID
    {
        //GET COLUMNS
        let cols: Box<dyn Iterator<Item = usize>> = if invert
        {
            Box::new((0..self.width()).rev()) //REVERSE ON DECRYPTION
        } else
        {
            Box::new(0..self.width()) //ENCRYPTION
        };

        //XOR COLUMNS IN LINEAR ORDER (0^1 ... 7^8, 8^0)
        for col in cols
        {
            let next_col = (col + 1) % W;
            for row in 0..self.height()
            {
                self[row][col] ^= self[row][next_col];
            }
        }
    }

    //PUBLIC
    /// Computes the cell-wise XOR of two Grids.
    ///
    /// This function takes two Grids of equal dimensions and modifies the Grid in-place, each cell
    /// being the bitwise XOR of the corresponding cell from the input Grid. It is used in WHY2
    /// for mixing round keys, applying masks, or combining intermediate states.
    ///
    /// # Parameters
    /// - `key_grid`: Input Grid for XOR
    pub fn xor_grids(&mut self, key_grid: &Grid<W, H>)
    {
        for y in 0..(self.height()) //Y DIM
        {
            for x in 0..(self.width()) //X DIM
            {
                //XOR
                self[y][x] ^= key_grid[y][x];
            }
        }
    }

    /// Applies nonlinear ARX-style mixing to each cell in the grid.
    ///
    /// This transformation introduces symmetric diffusion by modifying each `i64` cell
    /// using a combination of addition, rotation, and XOR operations. The process is
    /// round-dependent and designed to obscure bit patterns across the Grid.
    ///
    /// For decryption, use [`inv_subcell`](Grid::inv_subcell).
    ///
    /// # Parameters
    /// - `round`: A round index used to tweak the transformation logic.
    ///
    /// # Behavior
    /// - Each cell is split into two 32-bit halves.
    /// - The halves are mixed using ARX (Add-Rotate-XOR) operations.
    /// - The result replaces the original cell value.
    ///
    /// # Notes
    /// - This method mutates the Grid in-place.
    /// - It is inspired by TEA/XTEA but adapted for WHY2’s Grid architecture.
    /// - The transformation is deterministic for a given round and Grid state.
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
                for _ in 0..(options::SUBCELL_ROUNDS)
                {
                    sum = sum.wrapping_add(options::SUBCELL_DELTA);

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

    /// Inverts transformation done by [`subcell`](Grid::subcell) method
    pub fn inv_subcell(&mut self, round: usize) //REMOVES NONLINEAR MIX
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

                //UNDO XOR TWEAK
                v1 ^= round as u32;

                //PREPARE SUM VALUE TO SUM AFTER ROUND ADDITIONS (DELTA * ROUNDS)
                let mut sum: u32 = options::SUBCELL_DELTA.wrapping_mul(options::SUBCELL_ROUNDS);

                //RUN ROUNDS IN REVERSE ORDER
                for _ in 0..(options::SUBCELL_ROUNDS)
                {
                    /*
                    REVERSE MIXING IN OPPOSITE ORDER
                    v1 = v1 + F(v0) ^ sum
                    v0 = v0 + F(v1') ^ sum
                    */

                    v1 = v1.wrapping_sub(((v0 << 4) ^ (v0 >> 5)).wrapping_add(v0) ^ sum);
                    v0 = v0.wrapping_sub(((v1 << 4) ^ (v1 >> 5)).wrapping_add(v1) ^ sum);

                    sum = sum.wrapping_sub(options::SUBCELL_DELTA);
                }

                //UNDO INITIAL XOR TWEAK
                v0 ^= round as u32;

                //REBUILD AND APPLY
                let out = ((v1 as u64) << 32) | (v0 as u64);
                *cell = out as i64;
            }
        }
    }

    /// Applies row-wise shifting to the Grid based on a key Grid.
    ///
    /// This transformation rotates each row of the Grid by a variable amount derived from
    /// the corresponding row in `key_grid`. The shift amount is computed by XORing all
    /// values in the key row and reducing modulo the Grid width.
    ///
    /// For decryption, use [`inv_shift_rows`](Grid::inv_shift_rows).
    ///
    /// # Parameters
    /// - `key_grid`: A Grid of the same dimensions used to derive row-wise shift values.
    ///
    /// # Behavior
    /// - Each row is rotated left by a computed amount.
    /// - The shift amount is:
    ///   `XOR(key_row) % width`
    ///
    /// # Notes
    /// - This method mutates the grid in-place.
    /// - The key grid must match the grid dimensions exactly.
    pub fn shift_rows(&mut self, key_grid: &Grid<W, H>)
    {
        self.shift_rows_handler(key_grid, false); //USE HANDLER
    }

    /// Inverts transformation done by [`shift_rows`](Grid::shift_rows) method
    pub fn inv_shift_rows(&mut self, key_grid: &Grid<W, H>)
    {
        self.shift_rows_handler(key_grid, true); //USE HANDLER
    }

    /// Applies column-wise mixing to the grid using linear XOR diffusion.
    ///
    /// This transformation modifies each column by XORing it with its adjacent column,
    /// introducing horizontal diffusion across the grid. The operation is performed in
    /// left-to-right order during encryption.
    ///
    /// For decryption, use [`inv_mix_columns`](Grid::inv_mix_columns).
    ///
    /// # Behavior
    /// - For each column `c`, compute:
    ///   `grid[row][c] ^= grid[row][(c + 1) % W]`
    /// - The last column wraps around to the first.
    ///
    /// # Notes
    /// - This method mutates the grid in-place.
    pub fn mix_columns(&mut self)
    {
        self.mix_columns_handler(false); //USE HANDLER
    }

    /// Inverts transformation done by [`mix_columns`](Grid::mix_columns) method
    pub fn inv_mix_columns(&mut self)
    {
        self.mix_columns_handler(true); //USE HANDLER
    }
}

//INTO ITERATOR
impl<const W: usize, const H: usize> IntoIterator for Grid<W, H>
{
    //TYPES
    type Item = i64;
    type IntoIter = IntoVecIter<i64>;

    //INTO ITERATOR
    fn into_iter(self) -> Self::IntoIter
    {
        self.0.into_iter().flat_map(|row| row.into_iter()).collect::<Vec<i64>>().into_iter()
    }
}

//INDEXING
impl<const W: usize, const H: usize> Index<usize> for Grid<W, H>
{
    type Output = [i64; W];

    fn index(&self, y: usize) -> &Self::Output
    {
        &self.0[y]
    }
}

//MUTABLE INDEXING
impl<const W: usize, const H: usize> IndexMut<usize> for Grid<W, H>
{
    fn index_mut(&mut self, y: usize) -> &mut Self::Output
    {
        &mut self.0[y]
    }
}

//XOR ASSIGN
impl<const W: usize, const H: usize> BitXorAssign<&Grid<W, H>> for &mut Grid<W, H>
{
    fn bitxor_assign(&mut self, rhs: &Grid<W, H>)
    {
        self.xor_grids(&rhs);
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
