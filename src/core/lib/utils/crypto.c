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

#include <stdlib.h>
#include <string.h>
#include <math.h>
#include <sys/types.h>
#include <sys/random.h>

unsigned long long why2_sum_segment(char *input) //THE OUTPUT IS GOING TO GROW A LOT WITH LONG input, BUT IT SHOULDN'T BE A BIG PROBLEM. I TESTED FOR OVERFLOWS UP TO 4096-CHAR input AND ONLY GOT TO (14*10^(-7))% OF FULL ULL RANGE LMAO
{
    unsigned long input_size = strlen(input);
    unsigned long segmented_input_size = ceil(input_size / (double) WHY2_SUM_SEGMENT_SIZE) * WHY2_SUM_SEGMENT_SIZE; //CALCULATE CLOSEST 32*n (OR WHY2_SUM_SEGMENT_SIZE*n, IF YOU WILL) TO input
    unsigned long long output = 0;

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

ssize_t why2_random(void *dest, size_t size)
{
    return getrandom(dest, size, GRND_NONBLOCK);
}

void why2_seed_random(unsigned int seed)
{
    srand(seed);
}

int why2_seeded_random()
{
    return rand();
}