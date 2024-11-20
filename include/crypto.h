/*
This is part of WHY2
Copyright (C) 2022 Václav Šmejkal

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

#ifndef WHY2_CRYPTO_H
#define WHY2_CRYPTO_H

#ifdef __cplusplus
extern "C" {
#endif

//MACROS
#define WHY2_SUM_SEGMENT_SIZE 32 //SEGMENT SIZE FOR CALCULATING SUM
#define WHY2_SUM_BASE_PRIME 31 //PRIME FOR SUM BASE
#define WHY2_SUM_MOD_PRIME 4294967295UL //PRIME FOR SUM MODULUS

//FUNCTIONS
unsigned long long why2_sum_segment(char *input); //CALCULATE SUM++ FOR input; USED FOR PADDING SEED

#ifdef __cplusplus
}
#endif

#endif