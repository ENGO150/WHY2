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

    char *returning = why2_strdup(get_string_from_json(json_obj, string));

    //DEALLOCATION
    json_object_put(json_obj);

    //GET STRINGS
    return returning;
}

int get_int_from_json_string(char *json, char *string)
{
    char *value = get_string_from_json_string(json, string); //GET VALUE

    int returning = atoi(value); //PARSE

    why2_deallocate(value);

    return returning;
}

int server_config_int(char *key)
{
    char *value_str = why2_chat_server_config(key); //GET STRING

    int returning = atoi(value_str); //PARSE INT

    why2_toml_read_free(value_str);

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
    char *output = why2_calloc(strlen(*json) + 4, sizeof(char));

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

void lowercase(char *string)
{
    for (unsigned long i = 0; i < strlen(string); i++)
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

why2_bool username_equal_deallocate(char *u1, char *u2) //SAME THING AS username_equal BUT IT DEALLOCATES u2
{
    why2_bool returning = username_equal(u1, u2);

    why2_toml_read_free(u2);

    return returning;
}

why2_bool check_username(char *username)
{
    if (username == NULL) return 0;

    if (username_equal_deallocate(username, why2_chat_server_config("server_username"))) return 0; //DISABLE 'server' USERNAME
    if (username_equal_deallocate(username, why2_chat_server_config("default_username"))) return 0; //DISABLE 'anon' USERNAME DUE TO ONE USERNAME PER SERVER

    for (unsigned long i = 0; i < strlen(username); i++)
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
    sleep(server_config_int("communication_time")); //yk wait

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

void send_socket_deallocate(char *text, char *username, int socket) //SAME AS why2_send_socket BUT IT DEALLOCATES username
{
    why2_send_socket(text, username, socket);

    why2_toml_read_free(username);
}

void send_socket(char *text, char *username, int socket, why2_bool welcome)
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

    if (welcome) //SENDING WELCOME MESSAGE TO USER
    {
        //GET FROM CONFIG
        char *max_uname = why2_chat_server_config("max_username_length");
        char *min_uname = why2_chat_server_config("min_username_length");
        char *max_tries = why2_chat_server_config("max_username_tries");

        //ADD THE INFO OBJS
        json_object_object_add(json, "max_uname", json_object_new_string(max_uname));
        json_object_object_add(json, "min_uname", json_object_new_string(min_uname));
        json_object_object_add(json, "max_tries", json_object_new_string(max_tries));

        //DEALLOCATION
        why2_toml_read_free(max_uname);
        why2_toml_read_free(min_uname);
        why2_toml_read_free(max_tries);
    }

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

void send_welcome_socket_deallocate(char *text, char *username, int socket) //SAME AS why2_send_socket BUT IT DEALLOCATES username
{
    send_socket(text, username, socket, 1);

    why2_toml_read_free(username);
}

void send_welcome_packet(int connection)
{
    send_welcome_socket_deallocate(WHY2_CHAT_CODE_ACCEPT_MESSAGES, why2_chat_server_config("server_username"), connection);
}

void trim_string(char **s)
{
    unsigned long start_spaces = 0;
    unsigned long end_spaces = 0;
    unsigned long actual_length;

    //COUNT start_spaces (HOW MANY SPACES ARE IN THE START)
    for (unsigned long i = 0; i < strlen(*s); i++)
    {
        if ((*s)[i] != ' ') break;
        start_spaces++;
    }

    //COUNT end_spaces
    for (long long i = (long long) strlen(*s) - 1; i >= 0; i--)
    {
        if ((*s)[i] != ' ') break;
        end_spaces++;
    }

    //USER'S HEAD HAS FELL ON THE SPACEBAR
    if (start_spaces + end_spaces > strlen(*s))
    {
        why2_deallocate(*s);
        *s = NULL;

        return;
    }

    //COUNT actual_length
    actual_length = strlen(*s) - (end_spaces + start_spaces);

    if (actual_length == strlen(*s)) return; //NO SPACES TO REMOVE

    char *st = why2_malloc(actual_length + 2); //TRIMMED s

    for (unsigned long i = start_spaces; i < (start_spaces + actual_length); i++)
    {
        st[i - start_spaces] = (*s)[i];
    }

    st[actual_length] = '\0';

    //DEALLOCATE UNTRIMMED s
    why2_deallocate(*s);

    //SET NEW s
    *s = st;
}

//GLOBAL
void why2_send_socket(char *text, char *username, int socket)
{
    send_socket(text, username, socket, 0);
}

void *why2_communicate_thread(void *arg)
{
    int connection = *(int*) arg;

    printf("User connected.\t\t%d\n", connection);

    send_welcome_packet(connection); //TELL USER HE ALL THE INFO HE NEEDS

    //GET USERNAME
    char *config_username = why2_chat_server_config("user_pick_username");

    char *raw = NULL;
    void *raw_ptr = NULL;
    char *raw_output = NULL;
    why2_bool force_exiting = 0;
    why2_bool invalid_username = 1;
    why2_bool exiting = 0;
    char *decoded_buffer = NULL;
    char *username;
    int usernames_n = 0;
    struct json_object *json = json_tokener_parse("{}");

    //GET DEFAULT USERNAME
    char *default_username = why2_chat_server_config("default_username");
    username = why2_strdup(default_username);
    why2_toml_read_free(default_username);

    if (config_username == NULL || strcmp(config_username, "true") == 0)
    {
        if (config_username == NULL) fprintf(stderr, "Your config doesn't contain 'user_pick_username'. Please update your configuration.\n");

        send_socket_deallocate(WHY2_CHAT_CODE_PICK_USERNAME, why2_chat_server_config("server_username"), connection); //ASK USER FOR USERNAME

        while (invalid_username)
        {
            why2_deallocate(username);
            if (usernames_n++ == server_config_int("max_username_tries")) //ASKED CLIENT WAY TOO FUCKING MANY TIMES FOR USERNAME, KICK HIM
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

            invalid_username = decoded_buffer == NULL || (strlen(decoded_buffer) > (unsigned long) server_config_int("max_username_length")) || (strlen(decoded_buffer) < (unsigned long) server_config_int("min_username_length")) || (!check_username(decoded_buffer)); //CHECK FOR USERNAMES LONGER THAN 20 CHARACTERS

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
                send_socket_deallocate(WHY2_CHAT_CODE_INVALID_USERNAME, why2_chat_server_config("server_username"), connection); //TELL THE USER HE IS DUMB AS FUCK
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
    char *server_username = why2_chat_server_config("server_username");

    //BUILD connection_message
    sprintf(connection_message, "%s connected", username);

    //SEND CONNECTION MESSAGE
    json_object_object_add(json, "message", json_object_new_string(connection_message));
    json_object_object_add(json, "username", json_object_new_string(server_username));

    json_object_object_foreach(json, key, value) //GENERATE JSON STRING
    {
        append(&raw, key, (char*) json_object_get_string(value));
    }
    add_brackets(&raw);
    json_object_put(json);

    send_to_all(raw); //FUCKING SEND TO ALL YOU TWAT

    why2_deallocate(raw);
    why2_deallocate(connection_message);
    why2_deallocate(username);
    why2_toml_read_free(server_username);

    while (!(exiting || force_exiting)) //KEEP COMMUNICATION ALIVE FOR 5 MINUTES [RESET TIMER AT MESSAGE SENT]
    {
        if ((raw = read_user(connection, &raw_ptr)) == NULL) break; //READ
        json = json_tokener_parse("{}");
        raw_output = why2_strdup("");

        //REMOVE CONTROL CHARACTERS FROM raw
        for (size_t i = 0; i < strlen(raw); i++)
        {
            if (raw[i] == '\\') raw[i] = '/';
        }

        decoded_buffer = get_string_from_json_string(raw, "message"); //DECODE

        //TRIM MESSAGE
        trim_string(&decoded_buffer);

        if (decoded_buffer != NULL && strlen(decoded_buffer) != 0)
        {
            if (decoded_buffer[0] == '!') //COMMANDS
            {
                if (strcmp(decoded_buffer, "!exit") == 0) //USER REQUESTED EXIT
                {
                    exiting = 1;
                } else
                {
                    send_socket_deallocate(WHY2_CHAT_CODE_INVALID_COMMAND, why2_chat_server_config("server_username"), connection); //INFORM USER THAT HE'S DUMB
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

        deallocation:

        //DEALLOCATION
        why2_deallocate(raw);
        why2_deallocate(raw_ptr);
        why2_deallocate(raw_output);
        why2_deallocate(decoded_buffer);
        json_object_put(json);
    }

    if (exiting) send_socket_deallocate(WHY2_CHAT_CODE_SSQC, why2_chat_server_config("server_username"), connection);

    printf("User disconnected.\t%d\n", connection);

    //DEALLOCATION
    close(connection);
    why2_deallocate(node.username);

    why2_list_remove(&connection_list, find_connection(connection));

    return NULL;
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

        send_socket_deallocate(WHY2_CHAT_CODE_SSQC, why2_chat_server_config("server_username"), connection_buffer.connection);

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
    //STUFF
    char *read = NULL;
    why2_bool exiting = 0;
    why2_bool continuing;
    unsigned char asking_username = 0;
    char *server_uname = NULL;

    //CONTENT
    char *username = NULL;
    char *message = NULL;

    //SERVER SETTINGS
    int max_uname = -1;
    int min_uname = -1;
    int max_tries = -1;

    printf(">>> ");
    fflush(stdout);

    while (!exiting)
    {
        continuing = 0;

        read = read_socket_raw(*((int*) socket));
        if (read == NULL) continue;

        //GET CONTENT
        username = get_string_from_json_string(read, "username");
        message = get_string_from_json_string(read, "message");

        if (server_uname == NULL) //GET SERVER USERNAME
        {
            server_uname = why2_strdup(username); //GET USERNAME

            //GET INFO
            max_uname = get_int_from_json_string(read, "max_uname");
            min_uname = get_int_from_json_string(read, "min_uname");
            max_tries = get_int_from_json_string(read, "max_tries");

            continuing = 1;
        }

        if ((strcmp(username, server_uname) == 0 && strncmp(message, "code", 4) == 0) && !continuing) //CODE WAS SENT
        {
            if (strcmp(message, WHY2_CHAT_CODE_SSQC) == 0) //SERVER BROKE UP WITH YOU
            {
                printf("%s%s%s\nServer closed the connection.\n", asking_username > max_tries ? WHY2_CLEAR_AND_GO_UP : "", WHY2_CLEAR_AND_GO_UP WHY2_CLEAR_AND_GO_UP, (asking_username == 0 ? "\n": ""));
                fflush(stdout);

                pthread_cancel(getline_thread); //CANCEL CLIENT getline
                exiting = 1; //EXIT THIS THREAD
            } else if (strcmp(message, WHY2_CHAT_CODE_PICK_USERNAME) == 0 || strcmp(message, WHY2_CHAT_CODE_INVALID_USERNAME) == 0) //PICK USERNAME (COULD BE CAUSE BY INVALID USERNAME)
            {
                if (strcmp(message, WHY2_CHAT_CODE_INVALID_USERNAME) == 0) //INVALID USERNAME
                {
                    printf(WHY2_CLEAR_AND_GO_UP WHY2_CLEAR_AND_GO_UP "%s\nInvalid username!\n\n\n", asking_username > 1 ? WHY2_CLEAR_AND_GO_UP : "");
                    fflush(stdout);
                }

                printf("%s%sEnter username (a-Z, 0-9; %d-%d characters):\n", asking_username++ > 0 ? WHY2_CLEAR_AND_GO_UP : "", WHY2_CLEAR_AND_GO_UP, min_uname, max_uname);
                fflush(stdout);
            } else if (strcmp(message, WHY2_CHAT_CODE_INVALID_COMMAND) == 0) //INVALID CMD
            {
                printf("\nInvalid command!\n\n");
                fflush(stdout);
            }
        } else if (!continuing)
        {
            printf(WHY2_CLEAR_AND_GO_UP WHY2_CLEAR_AND_GO_UP); //do not fucking ask me how the fucking formatting fucking works, i dont fucking know

            if (asking_username > 1)
            {
                asking_username = 0;
                printf(WHY2_CLEAR_AND_GO_UP);
            }

            printf("\n\n%s%s: %s\n\n", WHY2_CLEAR_AND_GO_UP, username, message);
        }


        if (!exiting && !continuing)
        {
            printf(">>> ");
            fflush(stdout);
        }

        //DEALLOCATION
        why2_deallocate(read);
        why2_deallocate(username);
        why2_deallocate(message);
    }

    why2_deallocate(server_uname);

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