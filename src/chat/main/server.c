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

#include <why2/chat/misc.h>

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

    printf("Server enabled.\n\n");

    //LOOP ACCEPT
    for (;;)
    {
        accepted = accept(listen_socket, (SA *) NULL, NULL); //ACCEPT NEW SOCKET //TODO: CLOSE (not only this one)

        if (accepted == -1) continue;

        pthread_create(&thread, NULL, why2_communicate_thread, &accepted);
        why2_register_connection(accepted); //PUSH TO LIST
    }

    return 0;
}