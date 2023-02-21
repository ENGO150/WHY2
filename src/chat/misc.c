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

#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

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

void *communicate_thread(void *arg)
{
    printf("User connected.\t%d\n", *((int*) arg));

    const time_t startTime = time(NULL);
    char *received = NULL;

    while (time(NULL) - startTime < 86400) //KEEP COMMUNICATION ALIVE FOR 24 HOURS
    {
        received = read_socket(*((int*) arg)); //READ

        if (received == NULL) return NULL; //FAILED; EXIT THREAD

        if (strcmp(received, "!exit\n") == 0) break; //USER REQUESTED PROGRAM EXIT

        printf("Received:\n%s\n\n", received);

        why2_deallocate(received);
    }

    printf("User exited.\t%d\n", *((int*) arg));

    //DEALLOCATION
    close(*((int*) arg));
    why2_deallocate(received);

    return NULL;
}

char *read_socket(int socket)
{
    if (socket == -1)
    {
        fprintf(stderr, "Reading socket failed.\n");
        return NULL;
    }

    unsigned short content_size = 0;
    char *content_buffer = why2_calloc(2, sizeof(char));

    //GET LENGTH
    if (recv(socket, content_buffer, 2, 0) != 2) return NULL;

    content_size = (unsigned short) (((unsigned) content_buffer[1] << 8) | content_buffer[0]);

    why2_deallocate(content_buffer);

    //ALLOCATE
    content_buffer = why2_calloc(content_size + 1, sizeof(char));

    //READ FINAL MESSAGE
    if (recv(socket, content_buffer, content_size, 0) != content_size) fprintf(stderr, "Socket probably read wrongly!\n");

    return content_buffer;
}
