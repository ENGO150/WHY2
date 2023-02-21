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

#include <why2/chat/misc.h>

#include <string.h>
#include <sys/socket.h>

#include <why2/memory.h>

void send_socket(char *text, int socket)
{
    unsigned short text_length = (unsigned short) strlen(text);
    char *final = why2_calloc(strlen(text) + 2, sizeof(char));

    //SPLIT LENGTH INTO TWO CHARS
    final[0] = (unsigned) text_length & 0xff;
    final[1] = (unsigned) text_length >> 8;

    for (int i = 2; i < text_length + 2; i++) //APPEND
    {
        final[i] = text[i - 2];
    }

    //SEND
    send(socket, final, text_length + 2, 0);

    why2_deallocate(final);
}