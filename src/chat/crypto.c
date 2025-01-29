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

#include <why2/memory.h>
#include <why2/misc.h>

#include <openssl/sha.h>
#include <openssl/evp.h>
#include <openssl/pem.h>
#include <openssl/ec.h>

char *ecc_pub = NULL;
char *ecc_pri = NULL;

void read_file(FILE *file, char **output)
{
    //VARIABLES
    int buffer_size;
    char *buffer;

    //GET LENGTH
    fseek(file, 0, SEEK_END);
    buffer_size = ftell(file);
    rewind(file);

    //READ
    buffer = why2_calloc(buffer_size + 1, sizeof(char));
    if (fread(buffer, buffer_size, 1, file) != 1) why2_die("Reading keyfile failed!");
    buffer[buffer_size] = '\0';

    //ASSIGN OUTPUT
    *output = buffer;
}

//GLOBAL
void why2_chat_init_keys(void)
{
    //KEY FILES
    FILE *public;
    FILE *private;

    //GET PATH TO KEY DIR
    char *path = why2_replace(WHY2_CHAT_KEY_LOCATION, "{HOME}", getenv("HOME"));

    //ALLOCATE THE KEY PATHS
    char *public_path = why2_malloc(strlen(path) + strlen(WHY2_CHAT_PUB_KEY) + 3);
    char *private_path = why2_malloc(strlen(path) + strlen(WHY2_CHAT_PRI_KEY) + 3);

    //GET THE ACTUAL KEY PATHS
    sprintf(public_path, "%s/%s%c", path, WHY2_CHAT_PUB_KEY, '\0');
    sprintf(private_path, "%s/%s%c", path, WHY2_CHAT_PRI_KEY, '\0');

    //CHECK IF KEYS EXIST
    if (access(path, R_OK) != 0)
    {
        mkdir(path, 0700);

        //SOME USER OUTPUT
        printf("You are probably running WHY2-Chat for the first time now.\nGenerating ECC keys...\n");

        //VARIABLES
        EVP_PKEY *pkey = NULL; //KEYPAIR
        EVP_PKEY_CTX *ctx = EVP_PKEY_CTX_new_id(EVP_PKEY_EC, NULL); //CREATE CTX

        EVP_PKEY_keygen_init(ctx); //INIT KEYGEN

        EVP_PKEY_CTX_set_ec_paramgen_curve_nid(ctx, WHY2_CHAT_ECC); //SETUP ECC

        EVP_PKEY_keygen(ctx, &pkey); //GENERATE ECC KEYPAIR

        printf("Saving keys...\n");

        //WRITE THE KEYS INTO KEY-FILES
        public = why2_fopen(public_path, "w+");
        private = why2_fopen(private_path, "w+");

        PEM_write_PrivateKey(private, pkey, NULL, NULL, 0, NULL, NULL); //WRITE PRI KEY
        PEM_write_PUBKEY(public, pkey); //WRITE PUB KEY

        //DEALLOCATION
        EVP_PKEY_CTX_free(ctx);
        EVP_PKEY_free(pkey);
    } else
    {
        //OPEN FILES
        public = why2_fopen(public_path, "r");
        private = why2_fopen(private_path, "r");

        //READ THE KEYS
        read_file(public, &ecc_pub);
        read_file(private, &ecc_pri);
    }

    //DEALLOCATION
    why2_deallocate(path);
    why2_deallocate(public_path);
    why2_deallocate(private_path);
    why2_deallocate(public);
    why2_deallocate(private);
}

void why2_chat_deallocate_keys(void)
{
    why2_deallocate(ecc_pub);
    why2_deallocate(ecc_pri);
}

char *why2_sha256(char *input)
{
    unsigned char *output = why2_malloc(SHA256_DIGEST_LENGTH + 1);
    char *formatted_output = why2_malloc(SHA256_DIGEST_LENGTH * 2 + 2);

    SHA256((unsigned char*) input, strlen(input), output);

    //SAVE AS STRING IN HEX
    for (int i = 0; i < SHA256_DIGEST_LENGTH; i++)
    {
        sprintf(formatted_output + (i * 2), "%02x", output[i]);
    }
    formatted_output[SHA256_DIGEST_LENGTH * 2] = '\0';

    //DEALLOCATION
    why2_deallocate(output);

    return formatted_output;
}