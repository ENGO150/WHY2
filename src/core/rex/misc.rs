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

use crate::core::rex::options::
{
    self,
    Data,
    Grid,
};

pub fn empty_grid() -> Grid //RETURN EMPTY ALLOCATED GRID
{
    [[0i64; options::GRID_DIMENSIONS.0]; options::GRID_DIMENSIONS.1]
}

pub fn shape_key(key: Vec<i64>) -> Grid //RESHAPE KEY FROM Vec<i64> TO GRID
{
    //GRID OPTIONS
    let grid_dims = options::GRID_DIMENSIONS;
    let grid_area = grid_dims.0 * grid_dims.1;

    //SHAPE
    let mut key_grid = empty_grid();
    for i in 0..grid_area
    {
        key_grid[i / grid_dims.1][i % grid_dims.0] = key[i] ^ key[i + grid_area]; //COMBINE EVERY PART OF KEY
    }

    key_grid
}

pub fn xor_grids(chunk: &mut Grid, key: &Grid) //XOR TWO GRIDS
{
    for y in 0..chunk.len() //Y DIM
    {
        for x in 0..chunk[y].len() //X DIM
        {
            //XOR
            chunk[y][x] ^= key[y][x];
        }
    }
}

pub fn subcell(chunk: &mut Grid, round: usize) //APPLIES NONLINEAR MIX
{
    //APPLY ON EACH CELL
    for col in chunk
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

pub fn shift_rows(chunk: &mut Grid, key: &Grid) //SHIFT ROWS IN chunk BASED ON key
{
    let rows = chunk.len() as i64; //ROWS IN chunk & key

    //SHIFT EACH ROW
    for (i, row) in chunk.iter_mut().enumerate()
    {
        //SPLIT key TO 8 PARTS & XOR EACH VALUE TO GET SHIFT
        let shift = key[i].iter().fold(0i64, |acc, &x| acc ^ x).rem_euclid(rows) as usize;

        //ROTATE THE ROW
        row.rotate_left(shift);
    }
}

pub fn mix_columns(chunk: &mut Grid) //MIX COLUMNS IN chunk GRID
{
    //XOR COLUMNS IN LINEAR ORDER (0^1 ... 7^8, 8^0)
    for col in 0..(options::GRID_DIMENSIONS.1)
    {
        let next_col = (col + 1) % (options::GRID_DIMENSIONS.1);
        for row in 0..(options::GRID_DIMENSIONS.0)
        {
            chunk[row][col] ^= chunk[row][next_col];
        }
    }
}
