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

EVP_PKEY *keypair = NULL; //KEYPAIR

//LOCAL
char* base64_encode(char *message)
{
    //VARIABLES
    BIO *bio;
    BIO *b64;
    BUF_MEM *buffer_ptr;
    char* encoded_message;

    //INIT BIOs
    b64 = BIO_new(BIO_f_base64());
    bio = BIO_new(BIO_s_mem());
    bio = BIO_push(b64, bio);

    //ENCODE
    BIO_write(bio, message, strlen(message));
    BIO_flush(bio);
    BIO_get_mem_ptr(bio, &buffer_ptr);

    //COPY
    encoded_message = why2_malloc(buffer_ptr -> length + 1);
    memcpy(encoded_message, buffer_ptr -> data, buffer_ptr -> length);
    encoded_message[buffer_ptr -> length] = '\0';

    //DEALLOCATION
    BIO_free_all(bio);

    return encoded_message;
}

char* base64_decode(char *encoded_message)
{
    //VARIABLES
    BIO *bio;
    BIO *b64;
    size_t length = strlen(encoded_message);
    char* decoded_message = why2_malloc(length);

    //INIT BIOs
    b64 = BIO_new(BIO_f_base64());
    bio = BIO_new_mem_buf(encoded_message, length);
    bio = BIO_push(b64, bio);

    //NULL-TERM
    decoded_message[BIO_read(bio, decoded_message, length)] = '\0';

    //DEALLOCATION
    BIO_free_all(bio);

    return decoded_message;
}

//GLOBAL
void why2_chat_init_keys(void)
{
    FILE *key; //KEY FILE

    char *path = why2_replace(WHY2_CHAT_KEY_LOCATION, "{HOME}", getenv("HOME")); //GET PATH TO KEY DIR
    char *key_path = why2_malloc(strlen(path) + strlen(WHY2_CHAT_KEY) + 3); //ALLOCATE THE KEY PATH

    //GET THE ACTUAL KEY PATH
    sprintf(key_path, "%s/%s%c", path, WHY2_CHAT_KEY, '\0');

    //CHECK IF KEY EXIST
    if (access(path, R_OK) != 0) //NOT FOUND - CREATE IT
    {
        mkdir(path, 0700);

        //SOME USER OUTPUT
        printf("No ECC key found.\nGenerating...\n\n");

        EVP_PKEY_CTX *ctx = EVP_PKEY_CTX_new_id(EVP_PKEY_EC, NULL); //CREATE CTX
        EVP_PKEY_keygen_init(ctx); //INIT KEYGEN

        EVP_PKEY_CTX_set_ec_paramgen_curve_nid(ctx, WHY2_CHAT_ECC); //SETUP ECC
        EVP_PKEY_keygen(ctx, &keypair); //GENERATE ECC KEYPAIR

        //WRITE THE KEYS INTO KEY-FILE
        key = why2_fopen(key_path, "w+");
        PEM_write_PrivateKey(key, keypair, NULL, NULL, 0, NULL, NULL); //WRITE THE KEY

        //DEALLOCATION
        EVP_PKEY_CTX_free(ctx);
    } else
    {
        key = why2_fopen(key_path, "r"); //OPEN KEY FILE
        keypair = PEM_read_PrivateKey(key, NULL, NULL, NULL); //LOAD KEYPAIR
    }

    //DEALLOCATION
    why2_deallocate(path);
    why2_deallocate(key_path);
    why2_deallocate(key);
}

char *why2_chat_ecc_sign(char *message)
{
    //VARIABLES
    EVP_MD_CTX *mdctx = NULL; //SIGNING CONTEXT
    size_t siglen;
    char *sig; //SIGNATURE
    char *encoded_sig; //FINAL (ENCODED) SIGNATURE

    //INIT mdctx
    mdctx = EVP_MD_CTX_new();
    EVP_DigestSignInit(mdctx, NULL, EVP_sha256(), NULL, keypair);

    EVP_DigestSignUpdate(mdctx, message, strlen(message)); //UPDATE MESSAGE TO SIGN
    EVP_DigestSignFinal(mdctx, NULL, &siglen); //COUNT LENGTH

    //GENERATE SIGNATURE
    sig = why2_malloc(siglen); //ALLOCATE SIGNATURE
    EVP_DigestSignFinal(mdctx, (unsigned char*) sig, &siglen);

    encoded_sig = base64_encode(sig); //CONVERT sig TO BASE64

    //DEALLOCATION
    why2_deallocate(sig);
    EVP_MD_CTX_free(mdctx);

    return encoded_sig;
}

void why2_chat_deallocate_keys(void)
{
    //DEALLOCATE THE pkey
    EVP_PKEY_free(keypair);
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