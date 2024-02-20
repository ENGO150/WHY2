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
#include <ctype.h>
#include <sys/socket.h>
#include <unistd.h>

#include <pthread.h>

#include <json-c/json.h>

#include <why2/chat/config.h>
#include <why2/chat/flags.h>

#include <why2/llist.h>
#include <why2/memory.h>
#include <why2/misc.h>

pthread_t getline_thread;

//LINKED LIST STUFF
typedef struct _connection_node
{
    int connection;
    pthread_t thread;
    char *username;
} connection_node_t; //SINGLE LINKED LIST

why2_list_t connection_list = WHY2_LIST_EMPTY;
why2_list_t waiting_list = WHY2_LIST_EMPTY;

char *get_string_from_json(struct json_object *json, char *string)
{
    struct json_object *object;
	json_object_object_get_ex(json, string, &object);

    return (char*) json_object_get_string(object);
}

char *get_string_from_json_string(char *json, char *string)
{
    struct json_object *json_obj = json_tokener_parse(json);

    if (json_obj == NULL) return NULL; //INVALID SYNTAX WAS SENT BY SOME FUCKING SCRIPT KIDDIE

    char *returning = get_string_from_json(json_obj, string);

    //DEALLOCATION
    json_object_put(json_obj);

    //GET STRINGS
    return returning;
}

void send_to_all(char *json)
{
    why2_node_t _first_node = (why2_node_t) { NULL, connection_list.head };
    why2_node_t *node_buffer = &_first_node;
    connection_node_t connection_buffer;

    //PARSE
    struct json_object *json_obj = json_tokener_parse(json);
    char *message = get_string_from_json(json_obj, "message");
    char *username = get_string_from_json(json_obj, "username");

    if (json_obj == NULL) return; //EXIT IF INVALID SYNTAX WAS SENT

    while (node_buffer -> next != NULL) //SEND TO ALL CONNECTIONS
    {
        node_buffer = node_buffer -> next;

        connection_buffer = *(connection_node_t*) node_buffer -> value;

        why2_send_socket(message, username, connection_buffer.connection); //SEND TO CLIENT
    }

    //DEALLOCATION
    json_object_put(json_obj);
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

    sprintf(output, "{%s}%c", *json, '\0');

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
    for (int i = 0; strncmp(content_buffer + strlen(content_buffer) - 2, "\"}", 2) != 0; i++)
    {
        if (recv(socket, content_buffer + i, 1, 0) != 1) //READ THE MESSAGE BY CHARACTERS
        {
            fprintf(stderr, "Socket probably read wrongly!\n");
        }
    }

    content_buffer[content_size - 1] = '\0';

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
    char *message = get_string_from_json(json_obj, "message");
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
    for (int i = 0; text[i] != '\0'; i++) //TODO: DO SOMETHING MORE
    {
        if (text[i] == '\"')
        {
            text[i] = '\'';
        }
    }
}

void lowercase(char *string)
{
    for (int i = 0; string[i] != '\0'; i++)
    {
        string[i] = tolower(string[i]);
    }
}

why2_bool username_equal(char *u1, char *u2)
{
    char *one = why2_strdup(u1);
    char *two = why2_strdup(u2);

    //LOWERCASE
    lowercase(one);
    lowercase(two);

    return strcmp(one, two) == 0;
}

why2_bool check_username(char *username)
{
    if (username == NULL) return 0;

    if (username_equal(username, WHY2_CHAT_SERVER_USERNAME)) return 0; //DISABLE 'server' USERNAME
    if (username_equal(username, WHY2_DEFAULT_USERNAME)) return 0; //DISABLE 'anon' USERNAME DUE TO ONE USERNAME PER SERVER

    for (int i = 0; username[i] != '\0'; i++)
    {
        if (!((username[i] >= 48 && username[i] <= 57) ||
        (username[i] >= 65 && username[i] <= 90) ||     //CHECK ONLY FOR a-Z & 0-9
        (username[i] >= 97 && username[i] <= 122)))
        {
            return 0;
        }
    }

    return 1;
}

void *stop_oldest_thread(void *id)
{
    sleep(WHY2_COMMUNICATION_TIME); //yk wait
    if (waiting_list.head == NULL) return NULL;
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

why2_node_t *find_connection(int connection)
{
    why2_node_t *head = connection_list.head;
    if (head == NULL) return NULL; //EMPTY LIST

    why2_node_t *buffer = head;

    while (buffer -> next != NULL)
    {
        if ((*(connection_node_t*) buffer -> value).connection == connection) return buffer;

        buffer = buffer -> next;
    }

    if (connection != (*(connection_node_t*) buffer -> value).connection) buffer = NULL; //PREVENT FROM RETURNING INVALID NODE

    return buffer;
}

char *read_user(int connection, void **raw_ptr)
{
    //VARIABLES
    void *buffer;
    pthread_t thread_buffer;
    pthread_t thread_deletion_buffer;

    //RESET VARIABLES
    *raw_ptr = NULL;

    buffer = &thread_buffer;

    //READ
    pthread_create(&thread_buffer, NULL, read_socket_raw_thread, &connection);
    why2_list_push(&waiting_list, &buffer, sizeof(buffer));

    //RUN DELETION THREAD
    pthread_create(&thread_deletion_buffer, NULL, stop_oldest_thread, &thread_buffer);

    //WAIT FOR MESSAGE
    pthread_join(thread_buffer, raw_ptr);
    why2_list_remove(&waiting_list, find_request(&thread_buffer));
    pthread_cancel(thread_deletion_buffer);

    if (*raw_ptr == WHY2_INVALID_POINTER || *raw_ptr == NULL) return NULL; //QUIT COMMUNICATION IF INVALID PACKET WAS RECEIVED

    return (char*) *raw_ptr;
}

char *get_username(int connection)
{
    why2_node_t *node = find_connection(connection);
    connection_node_t c_node = *(connection_node_t*) node -> value;

    return c_node.username;
}

//GLOBAL
void why2_send_socket(char *text, char *username, int socket)
{
    char *output = why2_strdup("");
    size_t length_buffer = strlen(text);
    char *text_copy = why2_calloc(length_buffer + 2, sizeof(char));
    struct json_object *json = json_tokener_parse("{}");

    //COPY text INTO text_copy (WITHOUT LAST CHARACTER WHEN NEWLINE IS AT THE END)
    if (text[length_buffer - 1] == '\n') length_buffer--;
    strncpy(text_copy, text, length_buffer);

    //UNFUCK QUOTES FROM text_copy
    remove_json_syntax_characters(text_copy);

    //REMOVE NON ASCII CHARS
    int j = 0;
    for (int i = 0; text_copy[i] != '\0'; i++)
    {
        if ((20 <= text_copy[i] && text_copy[i] <= 126)) text_copy[i] = text_copy[j++] = text_copy[i];
    }

    text_copy[j] = '\0';

    //ADD OBJECTS
    json_object_object_add(json, "message", json_object_new_string(text_copy));
    if (username != NULL) json_object_object_add(json, "username", json_object_new_string(username)); //WAS SENT FROM SERVER

    //GENERATE JSON STRING
    json_object_object_foreach(json, key, value)
    {
        append(&output, key, (char*) json_object_get_string(value));
    }
    add_brackets(&output);

    why2_deallocate(text_copy);
    json_object_put(json);

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

    printf("User connected.\t\t%d\n", connection);

    //GET USERNAME
    char *string_buffer = why2_replace(WHY2_CHAT_CONFIG_DIR "/" WHY2_CHAT_CONFIG_SERVER, "{USER}", getenv("USER"));
    char *config_username = why2_toml_read(string_buffer, "user_pick_username");

    char *raw = NULL;
    void *raw_ptr = NULL;
    char *raw_output = NULL;
    why2_bool force_exiting = 0;
    why2_bool invalid_username = 1;
    why2_bool exiting = 0;
    char *decoded_buffer = NULL;
    char *username = why2_strdup(WHY2_DEFAULT_USERNAME);
    int usernames_n = 0;
    struct json_object *json = json_tokener_parse("{}");

    why2_deallocate(string_buffer);

    if (config_username == NULL || strcmp(config_username, "true") == 0)
    {
        if (config_username == NULL) fprintf(stderr, "Your config doesn't contain 'user_pick_username'. Please update your configuration.\n");

        why2_send_socket(WHY2_CHAT_CODE_PICK_USERNAME, WHY2_CHAT_SERVER_USERNAME, connection); //ASK USER FOR USERNAME

        while (invalid_username)
        {
            if (usernames_n++ == WHY2_MAX_USERNAME_TRIES) //ASKED CLIENT WAY TOO FUCKING MANY TIMES FOR USERNAME, KICK HIM
            {
                exiting = 1;
                goto deallocation;
            }

            if ((raw = read_user(connection, &raw_ptr)) == NULL) //READ
            {
                force_exiting = 1; //FAILURE
                goto deallocation;
            }

            decoded_buffer = get_string_from_json_string(raw, "message"); //DECODE

            invalid_username = decoded_buffer == NULL || (strlen(decoded_buffer) > WHY2_MAX_USERNAME_LENGTH) || (strlen(decoded_buffer) < WHY2_MIN_USERNAME_LENGTH) || (!check_username(decoded_buffer)); //CHECK FOR USERNAMES LONGER THAN 20 CHARACTERS

            why2_deallocate(username);
            username = decoded_buffer;

            if (!invalid_username)
            {
                //GO TROUGH LIST AND CHECK FOR DUPLICITY
                why2_node_t *buffer = connection_list.head;

                while (buffer != NULL)
                {
                    //GET USERNAME
                    connection_node_t *co_node = (connection_node_t*) buffer -> value;

                    if (strcmp(co_node -> username, decoded_buffer) == 0) //COMPARE
                    {
                        invalid_username = 1;
                        break;
                    }

                    buffer = buffer -> next; //NEXT
                }
            }

            //DEALLOCATE STUFF HERE
            why2_deallocate(raw);
            why2_deallocate(raw_ptr);

            if (invalid_username)
            {
                why2_send_socket(WHY2_CHAT_CODE_INVALID_USERNAME, WHY2_CHAT_SERVER_USERNAME, connection); //TELL THE USER HE IS DUMB AS FUCK
                continue;
            }

            printf("User set username.\t%d\t%s\n", connection, decoded_buffer);
        }
    }

    why2_toml_read_free(config_username);

    connection_node_t node = (connection_node_t)
    {
        connection,
        pthread_self(),
        why2_strdup(username)
    };

    why2_list_push(&connection_list, &node, sizeof(node)); //ADD TO LIST

    raw = why2_strdup("");
    char *connection_message = why2_malloc(strlen(username) + 11);

    //BUILD connection_message
    sprintf(connection_message, "%s connected", username);

    //SEND CONNECTION MESSAGE
    json_object_object_add(json, "message", json_object_new_string(connection_message));
    json_object_object_add(json, "username", json_object_new_string(WHY2_CHAT_SERVER_USERNAME));

    json_object_object_foreach(json, key, value) //GENERATE JSON STRING
    {
        append(&raw, key, (char*) json_object_get_string(value));
    }
    add_brackets(&raw);
    json_object_put(json);

    send_to_all(raw); //FUCKING SEND TO ALL YOU TWAT

    why2_send_socket(WHY2_CHAT_CODE_ACCEPT_MESSAGES, WHY2_CHAT_SERVER_USERNAME, connection); //TELL USER HE CAN START COMMUNICATING

    why2_deallocate(raw);
    why2_deallocate(connection_message);
    why2_deallocate(username);

    while (!(exiting || force_exiting)) //KEEP COMMUNICATION ALIVE FOR 5 MINUTES [RESET TIMER AT MESSAGE SENT]
    {
        if ((raw = read_user(connection, &raw_ptr)) == NULL) break; //READ
        json = json_tokener_parse("{}");
        raw_output = why2_strdup("");

        //REMOVE CONTROL CHARACTERS FROM raw
        for (int i = 0; raw[i] != '\0'; i++)
        {
            if (raw[i] == '\\') raw[i] = '/';
        }

        decoded_buffer = get_string_from_json_string(raw, "message"); //DECODE

        if (decoded_buffer == NULL) //idk sometimes this happen, idk why
        {
            force_exiting = 1; //force exit <3
        } else
        {
            if (strlen(decoded_buffer) != 0)
            {
                if (decoded_buffer[0] == '!') //COMMANDS
                {
                    if (strcmp(decoded_buffer, "!exit") == 0) //USER REQUESTED EXIT
                    {
                        exiting = 1;
                    } else
                    {
                        why2_send_socket(WHY2_CHAT_CODE_INVALID_COMMAND, WHY2_CHAT_SERVER_USERNAME, connection); //INFORM USER THAT HE'S DUMB
                    }

                    //IGNORE MESSAGES BEGINNING WITH '!'
                } else
                {
                    //REBUILD MESSAGE WITH USERNAME
                    json_object_object_add(json, "message", json_object_new_string(decoded_buffer));
                    json_object_object_add(json, "username", json_object_new_string(get_username(connection)));

                    json_object_object_foreach(json, key, value) //GENERATE JSON STRING
                    {
                        append(&raw_output, key, (char*) json_object_get_string(value));
                    }
                    add_brackets(&raw_output);

                    send_to_all(raw_output); //e
                }
            }
        }

        deallocation:

        //DEALLOCATION
        why2_deallocate(raw);
        why2_deallocate(raw_ptr);
        why2_deallocate(raw_output);
        why2_deallocate(decoded_buffer);
        json_object_put(json);
    }

    if (exiting) why2_send_socket(WHY2_CHAT_CODE_SSQC, WHY2_CHAT_SERVER_USERNAME, connection);

    printf("User disconnected.\t%d\n", connection);

    //DEALLOCATION
    close(connection);
    why2_deallocate(node.username);

    why2_list_remove(&connection_list, find_connection(connection));

    return NULL;
}

char *why2_read_socket(int socket)
{
    char *raw_socket = read_socket_raw(socket);

    char *final_message;
    struct json_object *json_obj = json_tokener_parse(raw_socket);

    if (raw_socket == NULL || json_obj == NULL) return NULL;

    //GET STRINGS
    char *message = get_string_from_json(json_obj, "message");
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

    return NULL;
}

void why2_clean_connections(void)
{
    why2_node_t *head = connection_list.head;
    if (head == NULL) return; //EMPTY LIST

    why2_node_t *node_buffer = head;
    why2_node_t *node_buffer_2;
    connection_node_t connection_buffer;

    do //GO TROUGH LIST
    {
        node_buffer_2 = node_buffer;
        node_buffer = node_buffer -> next;

        connection_buffer = *(connection_node_t*) node_buffer_2 -> value;

        why2_send_socket(WHY2_CHAT_CODE_SSQC, WHY2_CHAT_SERVER_USERNAME, connection_buffer.connection);

        close(connection_buffer.connection);
        why2_list_remove(&connection_list, node_buffer_2); //REMOVE
    } while (node_buffer != NULL);
}

void why2_clean_threads(void)
{
    why2_node_t *head = waiting_list.head;
    if (head == NULL) return; //EMPTY LIST

    why2_node_t *node_buffer = head;
    why2_node_t *node_buffer_2;

    do //GO TROUGH LIST
    {
        node_buffer_2 = node_buffer;
        node_buffer = node_buffer -> next;

        pthread_cancel(**(pthread_t**)(node_buffer_2 -> value));
        why2_list_remove(&waiting_list, node_buffer_2); //REMOVE
    } while (node_buffer != NULL);
}

void *why2_listen_server(void *socket)
{
    char *read = NULL;
    why2_bool exiting = 0;
    unsigned char asking_username = 0;

    printf(">>> ");
    fflush(stdout);

    while (!exiting)
    {
        read = why2_read_socket(*((int*) socket));

        if (read == NULL) continue;

        if (strncmp(read, WHY2_CHAT_SERVER_USERNAME ": code", 12) == 0) //CODE WAS SENT
        {
            if (strcmp(read + 8, WHY2_CHAT_CODE_SSQC) == 0) //SERVER BROKE UP WITH YOU
            {
                printf("%s%s%s\nServer closed the connection.\n", asking_username > WHY2_MAX_USERNAME_TRIES ? WHY2_CLEAR_AND_GO_UP : "", WHY2_CLEAR_AND_GO_UP WHY2_CLEAR_AND_GO_UP, (asking_username == 0 ? "\n": ""));
                fflush(stdout);

                pthread_cancel(getline_thread); //CANCEL CLIENT getline
                exiting = 1; //EXIT THIS THREAD
            } else if (strcmp(read + 8, WHY2_CHAT_CODE_ACCEPT_MESSAGES) == 0)
            {
                continue;
            } else if (strcmp(read + 8, WHY2_CHAT_CODE_PICK_USERNAME) == 0 || strcmp(read + 8, WHY2_CHAT_CODE_INVALID_USERNAME) == 0) //PICK USERNAME (COULD BE CAUSE BY INVALID USERNAME)
            {
                if (strcmp(read + 8, WHY2_CHAT_CODE_INVALID_USERNAME) == 0) //INVALID USERNAME
                {
                    printf(WHY2_CLEAR_AND_GO_UP WHY2_CLEAR_AND_GO_UP "%s\nInvalid username!\n\n\n", asking_username > 1 ? WHY2_CLEAR_AND_GO_UP : "");
                    fflush(stdout);
                }

                printf("%s%sEnter username (a-Z, 0-9; %d-%d characters):\n", asking_username++ > 0 ? WHY2_CLEAR_AND_GO_UP : "", WHY2_CLEAR_AND_GO_UP, WHY2_MIN_USERNAME_LENGTH, WHY2_MAX_USERNAME_LENGTH);
                fflush(stdout);
            } else if (strcmp(read + 8, WHY2_CHAT_CODE_INVALID_COMMAND) == 0) //INVALID CMD
            {
                printf("\nInvalid command!\n\n");
                fflush(stdout);
            }
        } else
        {
            printf(WHY2_CLEAR_AND_GO_UP WHY2_CLEAR_AND_GO_UP); //do not fucking ask me how the fucking formatting fucking works, i dont fucking know

            if (asking_username > 1)
            {
                asking_username = 0;
                printf(WHY2_CLEAR_AND_GO_UP);
            }

            printf("\n\n%s%s\n\n", WHY2_CLEAR_AND_GO_UP, read);
        }


        if (!exiting)
        {
            printf(">>> ");
            fflush(stdout);
        }

        why2_deallocate(read);
    }

    return NULL;
}

void *why2_getline_thread(WHY2_UNUSED void* arg)
{
    getline_thread = pthread_self();

    char *line = NULL;
    size_t line_length = 0;
    if (getline(&line, &line_length, stdin) == -1) why2_die("Reading input failed.");

    return line;
}