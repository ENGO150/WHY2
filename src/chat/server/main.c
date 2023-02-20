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

#include <why2/chat/common.h>

#include <unistd.h>

char *read_socket(int socket);
void *communicate_thread(void *arg);

int main(void)
{
    int listen_socket = socket(AF_INET, SOCK_STREAM, 0); //CREATE SERVER SOCKET
    int accepted;
    pthread_t thread;

    if (listen_socket < 0) why2_die("Failed creating socket.");

    //DEFINE SERVER ADDRESS
    struct sockaddr_in server_addr;
    server_addr.sin_family = AF_INET;
    server_addr.sin_port = htons(SERVER_PORT);
    server_addr.sin_addr.s_addr = INADDR_ANY;

    //BIND SOCKET
    if (bind(listen_socket, (SA *) &server_addr, sizeof(server_addr)) < 0) why2_die("Failed binding socket.");

    //LISTEN
    if (listen(listen_socket, MAX_CONNECTIONS) < 0) why2_die("Binding failed.");

    //LOOP ACCEPT
    for (;;)
    {
        accepted = accept(listen_socket, (SA *) NULL, NULL); //ACCEPT NEW SOCKET //TODO: CLOSE (not only this one)

        if (accepted == -1) continue;

        pthread_create(&thread, NULL, communicate_thread, &accepted);
    }

    return 0;
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
