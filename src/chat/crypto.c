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

#include <why2/chat/crypto.h>

#include <stdio.h>
#include <sys/random.h>

#include <gmp.h>

//LOCAL
void generate_prime(mpz_t x)
{
    //RANDOM
    gmp_randstate_t state;
    gmp_randinit_default(state);
    unsigned long random_buffer; //SEED

    do
    {
        if (getrandom(&random_buffer, sizeof(unsigned long), GRND_NONBLOCK) == -1) why2_die("getrandom fn failed!");

        //GENERATE RANDOM PRIME USING random_buffer SEED
        gmp_randseed_ui(state, random_buffer);
        mpz_urandomb(x, state, WHY2_CHAT_KEY_BITS);
        mpz_nextprime(x, x);
    } while (mpz_probab_prime_p(x, WHY2_CHAT_PRIME_ITERS) == 0); //CHECK FOR PRIME PROBABILITY

    //DEALLOCATION
    gmp_randclear(state);
}