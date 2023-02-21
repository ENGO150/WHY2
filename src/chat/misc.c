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
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

#include <why2/memory.h>

//LINKED LIST STUFF (BIT CHANGED memory.c'S VERSION)
typedef struct node
{
    int connection;
    struct node *next;
} node_t; //SINGLE LINKED LIST

node_t *head = NULL;

void push_to_list(int connection)
{
    //CREATE NODE
    node_t *new_node = malloc(sizeof(node_t));
    node_t *buffer = head;

    new_node -> connection = connection;
    new_node -> next = NULL;

    if (head == NULL) //INIT LIST
    {
        head = new_node;
    } else
    {
        while (buffer -> next != NULL) buffer = buffer -> next; //GET TO THE END OF LIST

        buffer -> next = new_node; //LINK
    }
}

void remove_node(node_t *node)
{
    if (node == NULL) return; //NULL NODE

    node_t *buffer_1 = head;
    node_t *buffer_2;

    while (buffer_1 -> next != NULL) //GO TROUGH EVERY ELEMENT IN LIST
    {
        if (buffer_1 == node) break; //FOUND (IF THE WHILE GOES TROUGH THE WHOLE LIST WITHOUT THE break, I ASSUME THE LAST NODE IS THE CORRECT ONE)

        buffer_1 = buffer_1 -> next;
    }

    if (node != buffer_1) return; //node WASN'T FOUND

    if (buffer_1 == head) //node WAS THE FIRST NODE IN THE LIST
    {
        //UNLINK
        head = buffer_1 -> next;
    } else //wdyt
    {
        //GET THE NODE BEFORE node
        buffer_2 = head;

        while (buffer_2 -> next != buffer_1) buffer_2 = buffer_2 -> next;

        //UNLINK
        buffer_2 -> next = buffer_1 -> next;
    }

    //DEALLOCATION
    free(node);
}

node_t *get_node(int connection)
{
    if (head == NULL) return NULL; //EMPTY LIST

    node_t *buffer = head;
    while (buffer -> next != NULL)
    {
        if (buffer -> connection == connection) return buffer;

        buffer = buffer -> next;
    }

    if (connection != buffer -> connection) buffer = NULL; //PREVENT FROM RETURNING INVALID NODE

    return buffer;
}

//GLOBAL
void why2_send_socket(char *text, int socket)
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

void *why2_communicate_thread(void *arg)
{
    printf("User connected.\t%d\n", *((int*) arg));

    const time_t startTime = time(NULL);
    char *received = NULL;

    while (time(NULL) - startTime < 86400) //KEEP COMMUNICATION ALIVE FOR 24 HOURS
    {
        received = why2_read_socket(*((int*) arg)); //READ

        if (received == NULL) return NULL; //FAILED; EXIT THREAD

        if (strcmp(received, "!exit\n") == 0) break; //USER REQUESTED PROGRAM EXIT

        printf("Received:\n%s\n\n", received);

        why2_deallocate(received);
    }

    printf("User exited.\t%d\n", *((int*) arg));

    //DEALLOCATION
    remove_node(get_node(why2_communicate_thread));
    close(*((int*) arg));
    why2_deallocate(received);

    return NULL;
}

char *why2_read_socket(int socket)
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

void why2_register_connection(int socket)
{
    push_to_list(socket); //SEND
}