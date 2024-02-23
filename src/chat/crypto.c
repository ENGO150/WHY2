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
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/stat.h>
#include <sys/random.h>

#include <why2/memory.h>
#include <why2/misc.h>

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

//GLOBAL
void why2_chat_generate_keys(void)
{
    //GET PATH TO KEY DIR
    char *path = why2_replace(WHY2_CHAT_KEY_LOCATION, "{USER}", getenv("USER"));

    //CHECK IF KEYS EXIST
    if (access(path, R_OK) != 0)
    {
        mkdir(path, 0700);

        //SOME USER OUTPUT
        printf("You are probably running WHY2-Chat for the first time now.\nGenerating RSA keys...\n");

        //VARIABLES
        mpz_t p, q, e, d, n, phi_n, buffer_1, buffer_2;
        mpz_inits(p, q, e, d, n, phi_n, buffer_1, buffer_2, NULL);

        //GENERATE PRIMES
        generate_prime(p);
        generate_prime(q);

        //SET e
        mpz_set_ui(e, WHY2_CHAT_RSA_EXPONENT);

        //GET n
        mpz_mul(n, p, q);

        //GET phi
        mpz_sub_ui(buffer_1, p, 1);
        mpz_sub_ui(buffer_2, q, 1);
        mpz_mul(phi_n, buffer_1, buffer_2);

        //COUNT d
        mpz_invert(d, e, phi_n);

        printf("Saving keys...\n");

        //ALLOCATE THE KEY PATHS
        char *public_path = why2_malloc(strlen(path) + strlen(WHY2_CHAT_PUB_KEY) + 3);
        char *private_path = why2_malloc(strlen(path) + strlen(WHY2_CHAT_PRI_KEY) + 3);

        //GET THE ACTUAL KEY PATHS
        sprintf(public_path, "%s/%s%c", path, WHY2_CHAT_PUB_KEY, '\0');
        sprintf(private_path, "%s/%s%c", path, WHY2_CHAT_PRI_KEY, '\0');

        //WRITE THE KEYS INTO KEY-FILES
        FILE *public = fopen(public_path, "w+");
        FILE *private = fopen(private_path, "w+");

        mpz_out_str(public, WHY2_CHAT_KEY_BASE, n);
        mpz_out_str(private, WHY2_CHAT_KEY_BASE, d);

        //KEYGEN DEALLOCATION
        mpz_clears(p, q, e, d, n, phi_n, buffer_1, buffer_2, NULL);
        why2_deallocate(public_path);
        why2_deallocate(private_path);
        fclose(public);
        fclose(private);
    }

    //DEALLOCATION
    why2_deallocate(path);
}