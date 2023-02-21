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

#ifndef WHY2_CHAT_MISC_H
#define WHY2_CHAT_MISC_H

void send_socket(char *text, int socket); //send socket.... wtf did you expect
char *read_socket(int socket); //read lol
void *communicate_thread(void *arg); //COMMUNICATION THREAD
void register_connection(int socket); //ADD SOCKET TO LIST

#endif