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
#define WHY2_CHECKSUM_SEGMENT_SIZE 4 //SEGMENT SIZE FOR CALCULATING CHECKSUM
#define WHY2_CHECKSUM_PRIME 5 //PRIME NUMBER FOR ROTATION

//FUNCTIONS
unsigned long why2_checksum_segment(char *input); //TOO LONG TO EXPLAIN, DEAL WITH IT. TREAT IT LIKE A NORMAL CHECKSUM

#ifdef __cplusplus
}
#endif

#endif