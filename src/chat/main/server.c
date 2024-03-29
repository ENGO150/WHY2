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

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <unistd.h>

#include <pthread.h>

#include <why2/chat/config.h>
#include <why2/chat/flags.h>
#include <why2/chat/misc.h>

#include <why2/memory.h>
#include <why2/misc.h>

int main(void)
{
    why2_chat_init_server_config();

    int listen_socket = socket(AF_INET, SOCK_STREAM, 0); //CREATE SERVER SOCKET
    pthread_t thread;
    size_t line_length_buffer = 0;
    char *line_buffer = NULL;

    if (listen_socket < 0) why2_die("Failed creating socket.");

    //DEFINE SERVER ADDRESS
    struct sockaddr_in server_addr;
    server_addr.sin_family = AF_INET;
    server_addr.sin_port = htons(WHY2_CHAT_SERVER_PORT);
    server_addr.sin_addr.s_addr = INADDR_ANY;

    //BIND SOCKET
    if (bind(listen_socket, (WHY2_SA *) &server_addr, sizeof(server_addr)) < 0) why2_die("Failed binding socket.");

    //LISTEN
    if (listen(listen_socket, WHY2_MAX_CONNECTIONS) < 0) why2_die("Binding failed.");

    printf("Server enabled.\n\n");

    pthread_create(&thread, NULL, why2_accept_thread, &listen_socket);

    for (;;)
    {
        if (getline(&line_buffer, &line_length_buffer, stdin) == -1) why2_die("Reading input failed.");

        if (strcmp(line_buffer, "!exit\n") == 0) //USER REQUESTED PROGRAM EXIT
        {
            printf("Exiting...\n");
            break;
        }
    }

    //QUIT COMMUNICATION
    why2_clean_connections(); //STOP LISTENING; DISCONNECT ALL USERS
    why2_clean_threads(); //STOP WAITING FOR MESSAGES

    //DEALLOCATION
    free(line_buffer);
    close(listen_socket);
    pthread_cancel(thread);

    why2_clean_memory(""); //RUN GARBAGE COLLECTOR

    return 0;
}