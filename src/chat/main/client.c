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
#include <arpa/inet.h>

#include <pthread.h>

#include <why2/chat/config.h>
#include <why2/chat/flags.h>
#include <why2/chat/misc.h>

#include <why2/memory.h>
#include <why2/misc.h>

int main(void)
{
    why2_chat_init_client_config();

    int listen_socket = socket(AF_INET, SOCK_STREAM, 0); //CREATE SERVER SOCKET
    char *line = NULL;
    void *return_line = NULL;
    size_t line_length = 0;
    pthread_t thread_buffer;
    pthread_t thread_getline;
    why2_bool ssqc = 0;

    //DEFINE SERVER ADDRESS
    struct sockaddr_in server_addr;
    server_addr.sin_family = AF_INET;
    server_addr.sin_port = htons(WHY2_SERVER_PORT);

    //GET IP
    printf("Welcome.\n\n");

    char *path = why2_replace(WHY2_CHAT_CONFIG_DIR "/" WHY2_CHAT_CONFIG_CLIENT, "{USER}", getenv("USER"));
    char *auto_connect = why2_toml_read(path, "auto_connect");

    if (strcmp(auto_connect, "true") == 0) //USER ENABLED AUTOMATIC CONNECTION
    {
        char *auto_connect_ip = why2_toml_read(path, "auto_connect_ip"); //GET IP

        line = strdup(auto_connect_ip);
        printf("%s\n", line);

        why2_toml_read_free(auto_connect_ip);
    } else
    {
        printf("Enter IP Address:\n>>> ");
        if (getline(&line, &line_length, stdin) == -1) why2_die("Reading input failed.");

        line_length = 3; //THIS IS FOR THE UNDERLINE THINGY
    }

    why2_deallocate(path);
    why2_toml_read_free(auto_connect);

    server_addr.sin_addr.s_addr = inet_addr(line);

    //PRINT UNDERLINE
    for (unsigned long i = 0; i < strlen(line) + line_length; i++)
    {
        printf("#");
    }
    printf("\n\n\n");

    free(line); //PREVENT FROM MEMORY LEAK

    int connectStatus = connect(listen_socket, (WHY2_SA *) &server_addr, sizeof(server_addr)); //CONNECT

    if (connectStatus < 0) why2_die("Connecting failed.");

    pthread_create(&thread_buffer, NULL, why2_listen_server, &listen_socket); //LISTEN TO OTHER USERS

    for (;;)
    {
        pthread_create(&thread_getline, NULL, why2_getline_thread, NULL);
        pthread_join(thread_getline, &return_line);

        if (return_line == WHY2_INVALID_POINTER) //SERVER QUIT COMMUNICATION
        {
            ssqc = 1;
            break;
        }

        line = (char*) return_line;

        printf(WHY2_CLEAR_AND_GO_UP);

        why2_send_socket(line, "anon", listen_socket);

        if (strcmp(line, "!exit\n") == 0) //USER REQUESTED PROGRAM EXIT
        {
            printf("Exiting...\n");
            break;
        }

        free(return_line);
    }

    //DEALLOCATION
    if (!ssqc)
    {
        pthread_cancel(thread_buffer);
        free(return_line);
    }

    why2_clean_memory(""); //RUN GARBAGE COLLECTOR

    return 0;
}