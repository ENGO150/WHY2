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

#ifndef WHY2_ENCRYPTER_H
#define WHY2_ENCRYPTER_H

#ifdef __cplusplus
extern "C" {
#endif

#include <why2/flags.h>

why2_output_flags why2_encrypt_text(char *text, char *keyNew); //TEXT from WILL BE ENCRYPTED WITH KEY AND RETURNED

#ifdef __cplusplus
}
#endif

#endif