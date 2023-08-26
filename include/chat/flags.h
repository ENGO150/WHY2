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

#ifndef WHY2_CHAT_COMMON_H
#define WHY2_CHAT_COMMON_H

//MACROS
#define WHY2_SA struct sockaddr
#define WHY2_SERVER_PORT 1204 //PORT
#define WHY2_MAX_CONNECTIONS 1000 //MAX USERS CONNECTED AT ONE TIME //TODO: Move all to configs
#define WHY2_COMMUNICATION_TIME 300 //SECONDS WAITING BEFORE KICKING USER (TIMEOUT)

#define WHY2_CLEAR_AND_GO_UP "\33[2K\r\033[A" //i mean read the name

#define WHY2_INVALID_POINTER (void*) 0xffffffffffffffff

#define WHY2_CHAT_CONFIG_DIR "/home/{USER}/.config/WHY2"
#define WHY2_CHAT_CONFIG_SERVER "server.yml"
#define WHY2_CHAT_CONFIG_SERVER_URL "https://raw.githubusercontent.com/ENGO150/WHY2/development/src/chat/server.yml"

#endif