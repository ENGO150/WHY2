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

#ifndef WHY2_CHAT_CRYPTO_H
#define WHY2_CHAT_CRYPTO_H

#ifdef __cplusplus
extern "C" {
#endif

#include <openssl/types.h>

#include <why2/flags.h>

#include <why2/chat/config.h>

//MACROS
#define WHY2_CHAT_ECC NID_secp521r1 //CURVE NAME

#define WHY2_CHAT_KEY_LOCATION WHY2_CONFIG_DIR "/keys" //KEYS LOCATION
#define WHY2_CHAT_KEY "secp521r1.pem"

#define WHY2_CHAT_BASE64_LENGTH_DELIMITER ':' //SEPARATES BASE64 FROM LENGTH (YnJhbWJvcmFrCg==:9)

#define WHY2_CHAT_PADDING(input_len) ((unsigned long) input_len / 2)

//FUNCTIONS
void why2_chat_init_keys(void); //INIT (POSSIBLY GENERATE) ECC KEYS
void why2_chat_deallocate_keys(void); //DEALLOCATE :) (NO SLUR HERE)

char *why2_chat_base64_encode(char *message, size_t *length); //ENCODE message OF length INTO BASE64 WITH LENGTH DELIMITER (WHY2_CHAT_BASE64_LENGTH_DELIMITER)
char *why2_chat_base64_decode(char *encoded_message, size_t *length); //DECODE encoded_message AND SET length TO OUTPUT LENGTH

char *why2_chat_ecc_sign(char *message); //SIGN message WITH ECC KEY
why2_bool why2_chat_ecc_verify_signature(char *message, char *signature, EVP_PKEY *key);

char *why2_chat_ecc_serialize_public_key(); //GET PUBLIC ECC KEY IN BASE64
EVP_PKEY* why2_chat_ecc_deserialize_public_key(char *pubkey); //GET EVP_PKEY FROM BASE64 PUBLIC ECC KEY

char *why2_chat_ecc_shared_key(char *ecc_key); //ENCRYPT message WITH ECC key

char *why2_sha256(char *input, size_t length); //HASH input USING SHA256 AND RETURN IN STRING

#ifdef __cplusplus
}
#endif

#endif