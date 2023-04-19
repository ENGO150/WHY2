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
#include <unistd.h>

#include <pthread.h>

#include <json-c/json.h>

#include <why2/chat/flags.h>
#include <why2/llist.h>
#include <why2/memory.h>
#include <why2/misc.h>

//LINKED LIST STUFF (BIT CHANGED memory.c'S VERSION) //TODO: Move llist in some single separate file
typedef struct node
{
    int connection;
    pthread_t thread;
    struct node *next;
} node_t; //SINGLE LINKED LIST

typedef struct waiting_node
{
    pthread_t thread;
    struct waiting_node *next;
} waiting_node_t; //SINGLE LINKED LIST

node_t *head = NULL;
why2_list_t waiting_list = WHY2_LIST_EMPTY;

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

char *get_string_from_json(struct json_object *json, char *string)
{
    struct json_object *object;
	json_object_object_get_ex(json, string, &object);

    return (char*) json_object_get_string(object);
}

char *get_string_from_json_string(char *json, char *string)
{
    struct json_object *json_obj = json_tokener_parse(json);
    char *returning = get_string_from_json(json_obj, string);

    //DEALLOCATION
    json_object_put(json_obj);

    //GET STRINGS
    return returning;
}

void *send_to_all(void *json)
{
    if (head == NULL) return NULL;

    node_t *node_buffer = head;

    //PARSE
    struct json_object *json_obj = json_tokener_parse((char*) json);

    if (json_obj == NULL) return NULL; //EXIT IF INVALID SYNTAX WAS SENT

    do //SEND TO ALL CONNECTIONS
    {
        why2_send_socket(get_string_from_json(json_obj, "message"), get_string_from_json(json_obj, "username"), node_buffer -> connection); //SEND TO CLIENT

        node_buffer = node_buffer -> next;
    } while (node_buffer -> next != NULL);

    //DEALLOCATION
    json_object_put(json_obj);

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

char *read_socket_raw(int socket)
{
    if (socket == -1)
    {
        fprintf(stderr, "Reading socket failed.\n");
        return NULL;
    }

    unsigned short content_size = 0;
    char *content_buffer = why2_calloc(3, sizeof(char));
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

    return content_buffer;
}

void *read_socket_raw_thread(void *socket)
{
    return read_socket_raw(*(int*) socket);
}

char *read_socket_from_raw(char *raw)
{
    char *final_message;
    struct json_object *json_obj = json_tokener_parse(raw);

    if (json_obj == NULL) return "ERR"; //RETURN IF INVALID SYNTAX WAS SENT BY SOME FUCKING SCRIPT KIDDIE

    //GET STRINGS
    char *message = get_string_from_json(json_obj, "message"); //TODO: Check deallocation problems
    char *username = get_string_from_json(json_obj, "username");

    //ALLOCATE final_message
    final_message = why2_calloc(strlen(message) + strlen(username) + 3, sizeof(char));

    //BUILD final_message
    sprintf(final_message, "%s: %s", username, message);

    //DEALLOCATION
    json_object_put(json_obj);

    return final_message;
}

void remove_json_syntax_characters(char *text)
{
    for (size_t i = 0; i < strlen(text); i++) //TODO: DO SOMETHING MORE
    {
        if (text[i] == '\"')
        {
            text[i] = '\'';
        }
    }
}

void *stop_oldest_thread(void *id)
{
    sleep(WHY2_COMMUNICATION_TIME); //yk wait
    if (waiting_list.head == NULL) return NULL;
        printf("A\n");
    if (**(pthread_t**) waiting_list.head -> value == *(pthread_t*) id) //THREAD IS STILL ALIVE, I HOPE
    {
        //KILL THE THREAD
        pthread_cancel(*(pthread_t*) id);
        why2_list_remove(&waiting_list, why2_list_find(&waiting_list, id));
    }

    return NULL;
}

why2_node_t *find_request(void *thread) //COPIED FROM why2_list_find; using double pointers
{
    why2_node_t *head = waiting_list.head;
    if (head == NULL) return NULL; //EMPTY LIST

    why2_node_t *buffer = head;

    while (buffer -> next != NULL)
    {
        if (*(void**) buffer -> value == thread) return buffer;

        buffer = buffer -> next;
    }

    if (thread != *(void**) buffer -> value) buffer = NULL; //PREVENT FROM RETURNING INVALID NODE

    return buffer;
}

//GLOBAL
void why2_send_socket(char *text, char *username, int socket)
{
    char *output = why2_strdup("");
    size_t length_buffer = strlen(text);
    char *text_copy = why2_calloc(length_buffer, sizeof(char));
    struct json_object *json = json_tokener_parse("{}");

    //COPY text INTO text_copy (WITHOUT LAST CHARACTER WHEN NEWLINE IS AT THE END)
    if (text[length_buffer - 1] == '\n') length_buffer--;
    strncpy(text_copy, text, length_buffer);

    //UNFUCK QUOTES FROM text_copy
    remove_json_syntax_characters(text_copy);

    //ADD OBJECTS
    json_object_object_add(json, "message", json_object_new_string(text_copy));
    json_object_object_add(json, "username", json_object_new_string(username));

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
    int connection = *(int*) arg;

    printf("User connected.\t%d\n", connection);

    push_to_list(connection, pthread_self()); //ADD TO LIST

    void *buffer;
    char *received = NULL;
    char *raw = NULL;
    void *raw_ptr = NULL;
    char *decoded_buffer;
    pthread_t thread_buffer;
    pthread_t thread_deletion_buffer;
    why2_bool exiting = 0;

    while (!exiting) //KEEP COMMUNICATION ALIVE FOR 5 MINUTES [RESET TIMER AT MESSAGE SENT]
    {
        buffer = &thread_buffer;

        //READ
        pthread_create(&thread_buffer, NULL, read_socket_raw_thread, &connection);
        why2_list_push(&waiting_list, &buffer, sizeof(thread_buffer));

        //RUN DELETION THREAD
        pthread_create(&thread_deletion_buffer, NULL, stop_oldest_thread, &thread_buffer);

        pthread_join(thread_buffer, &raw_ptr);
        why2_list_remove(&waiting_list, find_request(&thread_buffer)); //TODO: SEGFAULT (asi)

        if (raw_ptr == WHY2_INVALID_POINTER || raw_ptr == NULL) break; //QUIT COMMUNICATION IF INVALID PACKET WAS RECEIVED

        raw = (char*) raw_ptr;

        //REMOVE CONTROL CHARACTERS FROM raw
        for (size_t i = 0; i < strlen(raw); i++)
        {
            if (raw[i] == '\\') raw[i] = '/';
        }

        received = read_socket_from_raw(raw);

        if (received == NULL) break; //FAILED; EXIT THREAD

        decoded_buffer = get_string_from_json_string(raw, "message"); //DECODE

        if (decoded_buffer[0] == '!') //COMMANDS
        {
            if (strcmp(decoded_buffer, "!exit") == 0) exiting = 1; //USER REQUESTED EXIT

            goto deallocation; //IGNORE MESSAGES BEGINNING WITH '!'
        }

        printf("Received:\n%s\n\n", received);

        pthread_create(&thread_buffer, NULL, send_to_all, raw);
        pthread_join(thread_buffer, NULL);

        deallocation:

        why2_deallocate(received);
        why2_deallocate(raw);
        why2_deallocate(raw_ptr);
        why2_deallocate(decoded_buffer);

        //RESET VARIABLES
        raw_ptr = NULL;
    }

    printf("User exited.\t%d\n", connection);

    //DEALLOCATION
    why2_deallocate(received);
    close(connection);
    remove_node(get_node(connection));

    return NULL; //TODO: Fix client segfault on timeout
}

char *why2_read_socket(int socket)
{
    char *raw_socket = read_socket_raw(socket);
    char *final_message;
    struct json_object *json_obj = json_tokener_parse(raw_socket);

    //GET STRINGS
    char *message = get_string_from_json(json_obj, "message"); //TODO: Check deallocation problems
    char *username = get_string_from_json(json_obj, "username");

    //ALLOCATE final_message
    final_message = why2_calloc(strlen(message) + strlen(username) + 3, sizeof(char));

    //BUILD final_message
    sprintf(final_message, "%s: %s", username, message);

    //DEALLOCATION
    why2_deallocate(raw_socket);
    json_object_put(json_obj);

    return final_message;
}

void *why2_accept_thread(void *socket)
{
    int accepted;
    pthread_t thread;

    //LOOP ACCEPT
    for (;;)
    {
        accepted = accept(*((int*) socket), (WHY2_SA *) NULL, NULL); //ACCEPT NEW SOCKET

        if (accepted == -1) continue;

        pthread_create(&thread, NULL, why2_communicate_thread, &accepted);
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

    printf(">>> ");
    fflush(stdout);

    for (;;)
    {
        read = why2_read_socket(*((int*) socket));
        printf(WHY2_CLEAR_AND_GO_UP);
        printf("%s\n\n>>> ", read);
        fflush(stdout);

        why2_deallocate(read);
    }
}