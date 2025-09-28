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

use crate::core::rex::Grid;

//CONSTS (DO NOT CHANGE THOSE UNTIL YOU ARE COMPLETELY SURE WHAT ARE YOU DOING)
pub const GRID_DIMENSIONS: (usize, usize) = (8, 8);                                                   //DIMENSIONS OF REX GRID
pub const ROUND_KEYS: usize               = 14;                                                       //NUMBER OF ITERATIONS TO RUN WITH ROUND KEYS
pub const SUBCELL_ROUNDS: u32             = 6;                                                        //ITERATIONS FOR MIXING
pub const SUBCELL_DELTA: u32              = 0x9E3779B9;                                               //USED TO BREAK SYMMETRY ((2 ^ 32) / PHI)

const GRID_W: usize = GRID_DIMENSIONS.0;
const GRID_H: usize = GRID_DIMENSIONS.1;

//STRUCTS
pub struct EncryptedData //DATA FOR REX ENCRYPTER
{
    pub output: Vec<Grid<GRID_W, GRID_H>>, //OUTPUT VALUE
    pub key: Grid<GRID_W, GRID_H>,         //KEY USED FOR ENCRYPTION
}

pub struct DecryptedData //DATA FOR REX DECRYPTER
{
    pub output: Vec<i64>, //OUTPUT VALUE
    pub key: Vec<i64>,    //KEY USED FOR DECRYPTION
}
