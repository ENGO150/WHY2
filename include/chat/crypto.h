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

#include <why2/chat/config.h>

//MACROS
#define WHY2_CHAT_ECC NID_secp521r1 //CURVE NAME

#define WHY2_CHAT_KEY_LOCATION WHY2_CONFIG_DIR "/keys" //KEYS LOCATION
#define WHY2_CHAT_KEY "secp521r1.pem"

void why2_chat_init_keys(void); //INIT (POSSIBLY GENERATE) ECC KEYS
void why2_chat_deallocate_keys(void); //DEALLOCATE :) (NO SLUR HERE)

char *why2_chat_ecc_sign(char *message); //SIGN message WITH ECC KEY

char *why2_sha256(char *input); //HASH input USING SHA256 AND RETURN IN STRING

#ifdef __cplusplus
}
#endif

#endif