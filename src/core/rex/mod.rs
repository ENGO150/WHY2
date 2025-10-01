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

//MODULES
pub mod crypto;
pub mod decrypter;
pub mod encrypter;
pub mod options;

use std::
{
    vec::IntoIter as IntoVecIter,
    slice::{ Iter, IterMut },
    ops::{ Index, IndexMut },
    fmt::
    {
        Display,
        Formatter,
        Result,
    },
};

//TYPES
#[derive(Clone, Debug)]
pub struct Grid<const W: usize, const H: usize>([[i64; W]; H]); //GRID FOR REX DATA

//IMPLEMENTATIONS
impl<const W: usize, const H: usize> Grid<W, H>
{
    //CREATE EMPTY GRID
    pub fn new() -> Self
    {
        Self([[0i64; W]; H])
    }

    //CREATE KEY GRID
    pub fn from_key(vec: Vec<i64>) -> Self
    {
        //GRID OPTIONS
        let grid_area = W * H;

        //SHAPE
        let mut key_grid = Self::new();
        for i in 0..grid_area
        {
            key_grid[i / W][i % W] = vec[i] ^ vec[i + grid_area]; //COMBINE EVERY PART OF KEY
        }

        key_grid
    }

    //CREATE VECTOR OF GRIDS FROM BYTES
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

        Ok(bytes.chunks(matrix_size).map(|chunk|
        {
            let mut grid = Grid::new();
            for j in 0..H
            {
                for i in 0..W
                {
                    let start = (j * W + i) * 8;
                    let slice = &chunk[start..start + 8];
                    grid[j][i] = i64::from_be_bytes(slice.try_into().unwrap());
                }
            }

            grid
        }).collect())
    }

    //ITERATOR
    pub fn iter(&self) -> Iter<'_, [i64; W]>
    {
        self.0.iter()
    }

    //MUTABLE ITERATOR
    pub fn iter_mut(&mut self) -> IterMut<'_, [i64; W]>
    {
        self.0.iter_mut()
    }

    //GET WIDTH (COLUMNS)
    pub fn width(&self) -> usize
    {
        W
    }

    //GET HEIGHT (HEIGTH)
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
            Box::new((0..W).rev()) //REVERSE ON DECRYPTION
        } else
        {
            Box::new(0..W) //ENCRYPTION
        };

        //XOR COLUMNS IN LINEAR ORDER (0^1 ... 7^8, 8^0)
        for col in cols
        {
            let next_col = (col + 1) % W;
            for row in 0..H
            {
                self[row][col] ^= self[row][next_col];
            }
        }
    }

    //PUBLIC
    pub fn xor_grids(&mut self, key_grid: &Grid<W, H>) //XOR TWO GRIDS
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

    pub fn subcell(&mut self, round: usize) //APPLIES NONLINEAR MIX
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

    pub fn shift_rows(&mut self, key_grid: &Grid<W, H>) //SHIFT ROWS IN grid BASED ON key_grid
    {
        self.shift_rows_handler(key_grid, false); //USE HANDLER
    }

    pub fn inv_shift_rows(&mut self, key_grid: &Grid<W, H>) //UNSHIFT ROWS IN grid BASED ON key_grid
    {
        self.shift_rows_handler(key_grid, true); //USE HANDLER
    }

    pub fn mix_columns(&mut self) //MIX COLUMNS IN grid GRID
    {
        self.mix_columns_handler(false); //USE HANDLER
    }

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
            (0..W).map(|_| "-".repeat(max_width + 2)).collect::<Vec<_>>().join("+")
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
