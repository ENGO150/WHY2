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
#define WHY2_CHAT_KEY_BITS 4096 //BITS..
#define WHY2_CHAT_PRIME_ITERS 100 //NUMBER OF ITERATIONS WHEN CHECKING PRIME NUMBER
#define WHY2_CHAT_RSA_EXPONENT 65537 //DEFAULT e

#define WHY2_CHAT_KEY_LOCATION WHY2_CHAT_CONFIG_DIR "/keys" //KEYS LOCATION
#define WHY2_CHAT_PUB_KEY "pub"
#define WHY2_CHAT_PRI_KEY "pri"
#define WHY2_CHAT_KEY_BASE 16 //BASE IN THE GENERATED KEYS ARE STORED IN WHY2_CHAT_KEY_LOCATION

void why2_chat_generate_keys(void); //GENERATE RSA KEYS

#ifdef __cplusplus
}
#endif

#endif