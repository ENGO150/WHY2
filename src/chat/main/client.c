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
#include <arpa/inet.h>

#include <why2/chat/misc.h>

int main(void)
{
    int listen_socket = socket(AF_INET, SOCK_STREAM, 0); //CREATE SERVER SOCKET
    char *line = NULL;
    size_t line_length = 0;
    pthread_t thread_buffer;

    //DEFINE SERVER ADDRESS
    struct sockaddr_in server_addr;
    server_addr.sin_family = AF_INET;
    server_addr.sin_port = htons(SERVER_PORT);
    server_addr.sin_addr.s_addr = INADDR_ANY;

    //GET IP
    printf("Welcome.\n\nEnter IP Address:\n>>> ");
    getline(&line, &line_length, stdin);

    server_addr.sin_addr.s_addr = inet_addr(line);

    //PRINT UNDERLINE
    for (unsigned long i = 0; i < strlen(line) + 3; i++)
    {
        printf("#");
    }
    printf("\n\n\n");

    int connectStatus = connect(listen_socket, (SA *) &server_addr, sizeof(server_addr)); //CONNECT

    if (connectStatus < 0) why2_die("Connecting failed.");

    pthread_create(&thread_buffer, NULL, why2_listen_server, &listen_socket); //LISTEN TO OTHER USERS

    for (;;)
    {
        getline(&line, &line_length, stdin);
        printf(CLEAR_AND_GO_UP);

        why2_send_socket(line, listen_socket);

        if (strcmp(line, "!exit\n") == 0) //USER REQUESTED PROGRAM EXIT
        {
            printf("Exiting...\n");
            break;
        }
    }

    pthread_cancel(thread_buffer);
    free(line);
    return 0;
}