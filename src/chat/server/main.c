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

void die(char *exit_message);

int main(void)
{
    int listen_socket = socket(AF_INET, SOCK_STREAM, 0); //CREATE SERVER SOCKET

    if (listen_socket < 0) die("Failed creating socket.");

    //DEFINE SERVER ADDRESS
    struct sockaddr_in server_addr;
    server_addr.sin_family = AF_INET;
    server_addr.sin_port = htons(SERVER_PORT);
    server_addr.sin_addr.s_addr = INADDR_ANY;

    //BIND SOCKET
    if (bind(listen_socket, (SA *) &server_addr, sizeof(server_addr)) < 0) die("Failed binding socket.");

    //LISTEN
    if (listen(listen_socket, 1) < 0) die("Binding failed.");

    return 0;
}

void die(char *exit_msg)
{
    fprintf(stderr, "%s\n", exit_msg);
    exit(0);
}