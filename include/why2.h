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

/*
    This file basically contains all header files that are needed to use WHY2.

    *You can use them individually ofc*
*/

#ifndef WHY2_WHY2_H
#define WHY2_WHY2_H

#ifdef __cplusplus
extern "C" {
#endif

//CORE
#include <why2/decrypter.h>
#include <why2/encrypter.h>
#include <why2/flags.h>
#include <why2/llist.h>
#include <why2/memory.h>
#include <why2/misc.h>

//LOGGER
#include <why2/logger/flags.h>
#include <why2/logger/logger.h>
#include <why2/logger/utils.h>

//CHAT
#include <why2/chat/config.h>
#include <why2/chat/flags.h>
#include <why2/chat/misc.h>

#ifdef __cplusplus
}
#endif

#endif