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

void why2_send_socket(char *text, char *username, int socket); //send socket.... wtf did you expect
char *why2_read_socket(int socket); //read lol
void *why2_communicate_thread(void *arg); //COMMUNICATION THREAD
void *why2_accept_thread(void *socket); //LOOP ACCEPTING CONNECTIONS
void why2_clean_threads(void); //CLOSE EVERY RUNNING THREAD
void *why2_listen_server(void *socket); //LISTEN FOR OTHER's USERS MESSAGES

typedef struct why2_connection
{
    int connection;
    unsigned long thread; //pthread_t
} why2_connection_t;

#endif