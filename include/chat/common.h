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
    DO NOT USE THIS FILE IN YOUR PROJECT!
*/

#ifndef WHY2_CHAT_COMMON_H
#define WHY2_CHAT_COMMON_H

//INCLUDES
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <netinet/in.h> //TODO: Remove not 100% common includes
#include <why2/misc.h>
#include <why2/memory.h>
#include <pthread.h>

//DEFINES
#define SA struct sockaddr
#define SERVER_PORT 1204
#define MAX_CONNECTIONS 1000

#define CLEAR_AND_GO_UP "\33[2K\r\033[A"

#endif