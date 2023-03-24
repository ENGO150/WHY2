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
#include <time.h>
#include <sys/socket.h>
#include <unistd.h>

#include <pthread.h>

#include <json-c/json.h>

#include <why2/chat/flags.h>
#include <why2/memory.h>

//LINKED LIST STUFF (BIT CHANGED memory.c'S VERSION)
typedef struct node
{
    int connection;
    pthread_t thread;
    struct node *next;
} node_t; //SINGLE LINKED LIST

node_t *head = NULL;

void push_to_list(int connection, pthread_t thread)
{
    //CREATE NODE
    node_t *new_node = malloc(sizeof(node_t));
    node_t *buffer = head;

    new_node -> connection = connection;
    new_node -> thread = thread;
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

void *send_to_all(void *message)
{
    if (head == NULL) return NULL;

    node_t *node_buffer = head;

    do //SEND TO ALL CONNECTIONS
    {
        why2_send_socket((char*) message, node_buffer -> connection); //SEND TO CLIENT

        node_buffer = node_buffer -> next;
    } while (node_buffer -> next != NULL);

    return NULL;
}

void append(char **destination, char *key, char *value)
{
    char *output = why2_calloc(strlen(*destination) + strlen(key) + strlen(value) + 7, sizeof(char));

    sprintf(output, "%s%s\"%s\":\"%s\"", *destination, strcmp(*destination, "") == 0 ? "" : ",", key, value);

    why2_deallocate(*destination);

    *destination = output;
}

void add_brackets(char **json)
{
    char *output = why2_calloc(strlen(*json) + 3, sizeof(char));

    sprintf(output, "{%s}", *json);

    why2_deallocate(*json);

    *json = output;
}

//GLOBAL
void why2_send_socket(char *text, int socket)
{
    char *output = why2_strdup("");
    size_t length_buffer = strlen(text);
    char *text_copy = why2_calloc(length_buffer, sizeof(char));
    struct json_object *json = json_tokener_parse("{}");

    //COPY text INTO text_copy WITHOUT LAST CHARACTER
    strncpy(text_copy, text, length_buffer - 1);

    //ADD OBJECTS
    json_object_object_add(json, "message", json_object_new_string(text_copy));
    json_object_object_add(json, "username", json_object_new_string("anon"));

    //GENERATE JSON STRING
    json_object_object_foreach(json, key, value)
    {
        append(&output, key, (char*) json_object_get_string(value));
    }
    add_brackets(&output);

    why2_deallocate(text_copy);
    json_object_put(json);

    //printf("NEGR\n");
    unsigned short text_length = (unsigned short) strlen(output) + 2;
    char *final = why2_calloc(text_length, sizeof(char));

    //SPLIT LENGTH INTO TWO CHARS
    final[0] = (unsigned) text_length & 0xff;
    final[1] = (unsigned) text_length >> 8;

    for (int i = 2; i < text_length; i++) //APPEND
    {
        final[i] = output[i - 2];
    }

    //SEND
    send(socket, final, text_length, 0);

    //DEALLOCATION
    why2_deallocate(final);
    why2_deallocate(output);
}

void *why2_communicate_thread(void *arg)
{
    why2_connection_t connection = *(why2_connection_t*) arg;

    printf("User connected.\t%d\n", connection.connection);

    push_to_list(connection.connection, pthread_self()); //TODO: Do something with why2_connection_t

    const time_t startTime = time(NULL);
    char *received = NULL;
    pthread_t thread_buffer;

    while (time(NULL) - startTime < 86400) //KEEP COMMUNICATION ALIVE FOR 24 HOURS
    {
        received = why2_read_socket(connection.connection); //READ

        if (received == NULL) return NULL; //FAILED; EXIT THREAD

        if (received[0] == '!') //COMMANDS
        {
            if (strcmp(received, "!exit\n") == 0) break; //USER REQUESTED EXIT

            continue; //IGNORE MESSAGES BEGINNING WITH '!'
        }

        printf("Received:\n%s\n\n", received);

        pthread_create(&thread_buffer, NULL, send_to_all, received);
        pthread_join(thread_buffer, NULL);

        why2_deallocate(received);
    }

    printf("User exited.\t%d\n", connection.connection);

    //DEALLOCATION
    why2_deallocate(received);
    close(connection.connection);
    remove_node(get_node(connection.connection));

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
    char *content_buffer = why2_calloc(3, sizeof(char));
    char *final_message;

    //GET LENGTH
    if (recv(socket, content_buffer, 2, 0) != 2)
    {
        fprintf(stderr, "Getting message length failed!\n");
        return NULL;
    }

    content_size = (unsigned short) (((unsigned) content_buffer[1] << 8) | content_buffer[0]);

    why2_deallocate(content_buffer);

    //ALLOCATE
    content_buffer = why2_calloc(content_size + 1, sizeof(char));

    //READ JSON MESSAGE
    if (recv(socket, content_buffer, content_size, 0) != content_size - 2) fprintf(stderr, "Socket probably read wrongly!\n");

    content_buffer[content_size - 2] = '\0'; //TODO: Possible problems

    //DECODE
    struct json_object *json_obj = json_tokener_parse(content_buffer);
    struct json_object *message_obj;
    struct json_object *username_obj;

	json_object_object_get_ex(json_obj, "message", &message_obj);
	json_object_object_get_ex(json_obj, "username", &username_obj);

    //GET STRINGS
    const char *message = json_object_get_string(message_obj);
    const char *username = json_object_get_string(username_obj);

    //ALLOCATE final_message;
    final_message = why2_calloc(strlen(message) + strlen(username) + 3, sizeof(char));

    //BUILD final_message
    sprintf(final_message, "%s: %s", username, message);

    //DEALLOCATION
    why2_deallocate(content_buffer);
    json_object_put(json_obj);

    return final_message;
}

void *why2_accept_thread(void *socket)
{
    int accepted;
    pthread_t thread;

    why2_connection_t connection_buffer;

    //LOOP ACCEPT
    for (;;)
    {
        accepted = accept(*((int*) socket), (WHY2_SA *) NULL, NULL); //ACCEPT NEW SOCKET //TODO: CLOSE (not only this one)

        if (accepted == -1) continue;

        connection_buffer = (why2_connection_t)
        {
            accepted,
            thread
        };

        pthread_create(&thread, NULL, why2_communicate_thread, &connection_buffer);
    }
}

void why2_clean_threads(void)
{
    if (head == NULL) return; //EMPTY LIST

    node_t *node_buffer = head;
    node_t *node_buffer_2;

    while (node_buffer -> next != NULL) //GO TROUGH LIST
    {
        node_buffer_2 = node_buffer;
        node_buffer = node_buffer -> next;

        close(node_buffer_2 -> connection);
        pthread_cancel(node_buffer_2 -> thread);

        remove_node(node_buffer_2); //REMOVE
    }
}

void *why2_listen_server(void *socket)
{
    char *read = NULL;

    printf(">>> "); //TODO: Make this smart
    fflush(stdout);

    for (;;)
    {
        read = why2_read_socket(*((int*) socket)); //TODO: Fix other user message formatting
        printf(WHY2_CLEAR_AND_GO_UP);
        printf("%s\n>>> ", read);
        fflush(stdout);

        why2_deallocate(read);
    }
}