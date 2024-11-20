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

#include <why2/crypto.h>

#include <string.h>
#include <math.h>

unsigned long why2_sum_segment(char *input) //I ABSOLUTELY DO NOT RECOMMEND USING THIS WITH LARGE KEYS!!!
{
    unsigned long input_size = strlen(input);
    unsigned long segmented_input_size = ceil(input_size / (double) WHY2_SUM_SEGMENT_SIZE) * WHY2_SUM_SEGMENT_SIZE; //CALCULATE CLOSEST 32*n (OR WHY2_SUM_SEGMENT_SIZE*n, IF YOU WILL) TO input
    unsigned long output = 0;

    for (unsigned long i = 0; i < segmented_input_size / WHY2_SUM_SEGMENT_SIZE; i++) //DIVIDE buffer INTO SEGMENTS, XOR EACH OTHER AND ADD TO output
    {
        unsigned long output_buffer = 0;
        for (unsigned long j = 0; j < WHY2_SUM_SEGMENT_SIZE; j++)
        {
            unsigned long index_buffer = i * WHY2_SUM_SEGMENT_SIZE + j;
            char value_buffer = (input_size > index_buffer) ? input[index_buffer] : 0;

            output_buffer ^= value_buffer; //XORING
            output_buffer = (output_buffer * WHY2_SUM_BASE_PRIME + value_buffer) % WHY2_SUM_MOD_PRIME;
        }

        output += output_buffer; //ADD
    }

    return output;
}