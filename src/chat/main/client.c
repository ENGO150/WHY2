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
#include <signal.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>

#include <pthread.h>

#include <why2/chat/config.h>
#include <why2/chat/crypto.h>
#include <why2/chat/flags.h>
#include <why2/chat/misc.h>

#include <why2/flags.h>
#include <why2/memory.h>
#include <why2/misc.h>

why2_bool exited = 0; //USER ALREADY REQUESTED EXIT
int listen_socket; //THE SERVER SOCKET

void exit_client(WHY2_UNUSED int i) //guess what
{
    if (exited) return;
    exited = 1;

    why2_send_socket(WHY2_CHAT_CODE_EXIT, NULL, listen_socket);
}

why2_bool command(char *input, char *command, char **arg)
{
    why2_bool returning = 0;

    //GET THE COMMANDS
    char *full_cmd = why2_malloc(strlen(WHY2_CHAT_COMMAND_PREFIX) + strlen(WHY2_CHAT_COMMAND_EXIT) + 2);
    sprintf(full_cmd, WHY2_CHAT_COMMAND_PREFIX "%s", command);

    //CLEAR arg
    why2_deallocate(*arg); //DEALLOCATION (why2_deallocate fn checks for NULL, don't you worry)
    *arg = NULL;

    if (strncmp(input, full_cmd, strlen(full_cmd)) == 0) //COMMAND WAS EXECUTED
    {
        if (strlen(full_cmd) == strlen(input) - 1) //COMMAND DOESN'T HAVE ARGS
        {
            returning = 1;
        } else if (strlen(input) - 2 > strlen(full_cmd) && input[strlen(full_cmd)] == ' ') //COMMAND CONTAINS ARGS
        {
            returning = 1;

            //GET THE ARGS
            *arg = why2_malloc(strlen(input) - strlen(full_cmd) - 1); //ALLOCATE
            for (unsigned long i = strlen(full_cmd) + 1; i < strlen(input) - 1; i++)
            {
                (*arg)[i - (strlen(full_cmd) + 1)] = input[i];
            }

            (*arg)[strlen(input) - strlen(full_cmd) - 2] = '\0'; //NULL TERM

            why2_trim_string(arg);
        }
    }

    //DEALLOCATE BUFFERS
    why2_deallocate(full_cmd);

    return returning;
}

void invalid(char *type)
{
    printf("\nInvalid %s! Use \"" WHY2_CHAT_COMMAND_PREFIX WHY2_CHAT_COMMAND_HELP "\" for list of commands.\n\n>>> ", type);
    fflush(stdout);
}

int main(void)
{
    signal(SIGINT, exit_client); //HANDLE ^C

    why2_check_version(); //CHECK FOR UPDATES
    why2_chat_init_client_config(); //CREATE client.toml CONFIGURATION
    why2_chat_init_keys(); //CREATE ECC KEY

    listen_socket = socket(AF_INET, SOCK_STREAM, 0); //CREATE SERVER SOCKET
    char *line = NULL;
    void *return_line = NULL;
    size_t line_length = 0;
    pthread_t thread_buffer;
    pthread_t thread_getline;
    why2_bool ssqc = 0;
    char *cmd_arg = NULL;

    //DEFINE SERVER ADDRESS
    struct sockaddr_in server_addr;
    server_addr.sin_family = AF_INET;
    server_addr.sin_port = htons(WHY2_CHAT_SERVER_PORT);

    //GET IP
    printf("Welcome.\n\n");

    char *auto_connect = why2_chat_client_config("auto_connect");

    if (strcmp(auto_connect, "true") == 0) //USER ENABLED AUTOMATIC CONNECTION
    {
        char *auto_connect_ip = why2_chat_client_config("auto_connect_ip"); //GET IP

        line = strdup(auto_connect_ip);
        printf("%s\n", line);

        why2_toml_read_free(auto_connect_ip);
    } else
    {
        printf("Enter IP Address:\n>>> ");
        if (getline(&line, &line_length, stdin) == -1) why2_die("Reading input failed.");

        line_length = 3; //THIS IS FOR THE UNDERLINE THINGY
    }

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
        why2_trim_string(&line);

        if (line == NULL) line = strdup("");

        printf(WHY2_CLEAR_AND_GO_UP);

        //TODO: Remove accents

        if (strncmp(line, WHY2_CHAT_COMMAND_PREFIX, strlen(WHY2_CHAT_COMMAND_PREFIX)) == 0) //OPTIMIZE COMMANDS
        {
            //COMMANDS
            if (command(line, WHY2_CHAT_COMMAND_EXIT, &cmd_arg)) //USER REQUESTED PROGRAM EXIT
            {
                printf("Exiting...\n");
                exit_client(0);
            } else if (command(line, WHY2_CHAT_COMMAND_HELP, &cmd_arg)) //HELP CMD
            {
                printf
                (
                    "\nCommands:\n---------\n%s\n\n>>> ",

                    WHY2_CHAT_COMMAND_PREFIX WHY2_CHAT_COMMAND_HELP "\t\tPrints out all the commands. :)\n"
                    WHY2_CHAT_COMMAND_PREFIX WHY2_CHAT_COMMAND_DM " <ID> <MSG>\tSends direct message to user.\n"
                    WHY2_CHAT_COMMAND_PREFIX WHY2_CHAT_COMMAND_PM " <ID> <MSG>\tSends encrypted message to user.\n" //TODO: Implement
                    WHY2_CHAT_COMMAND_PREFIX WHY2_CHAT_COMMAND_LIST "\t\tLists all users and their IDs.\n"
                    WHY2_CHAT_COMMAND_PREFIX WHY2_CHAT_COMMAND_VERSION "\tCheck server version.\n"
                    WHY2_CHAT_COMMAND_PREFIX WHY2_CHAT_COMMAND_EXIT "\t\tExits the program."
                );

                fflush(stdout);
            } else if (command(line, WHY2_CHAT_COMMAND_DM, &cmd_arg))
            {
                char *id; //PM RECEIVER
                char *msg; //something racial

                //CHECK ARGS VALIDITY
                why2_bool valid_param = cmd_arg != NULL && strlen(cmd_arg) >= 3;
                if (valid_param)
                {
                    valid_param = 0;
                    for (unsigned long i = 1; i < strlen(cmd_arg); i++)
                    {
                        if (cmd_arg[i] == ' ')
                        {
                            valid_param = 1;

                            //EXTRACT FIRST ARG (ID)
                            id = why2_malloc(i + 1);
                            for (unsigned long j = 0; j < i; j++)
                            {
                                id[j] = cmd_arg[j];
                            }
                            id[i] = '\0';

                            if (atoi(id) <= 0) //INVALID ID PASSED
                            {
                                valid_param = 0;
                                why2_deallocate(id);
                                break;
                            }

                            //EXTRACT MESSAGE
                            msg = cmd_arg + i + 1;
                            break;
                        }
                    }
                }

                if (!valid_param) //INVALID ARGS
                {
                    invalid("usage");
                    continue;
                }

                //BUILD MESSAGE TO SEND TO SERVER
                char *final_message = why2_malloc(strlen(id) + strlen(msg) + 2);
                sprintf(final_message, "%s;%s%c", id, msg, '\0');

                why2_send_socket_code(final_message, NULL, listen_socket, WHY2_CHAT_CODE_PM); //SEND

                //DEALLOCATION
                why2_deallocate(id);
                why2_deallocate(final_message);
            } else if (command(line, WHY2_CHAT_COMMAND_LIST, &cmd_arg)) //LIST CMD
            {
                why2_send_socket_code(NULL, NULL, listen_socket, WHY2_CHAT_CODE_LIST);
            } else if (command(line, WHY2_CHAT_COMMAND_VERSION, &cmd_arg)) //VERSION CMD
            {
                why2_send_socket(WHY2_CHAT_CODE_VERSION, NULL, listen_socket);
            } else
            {
                invalid("command");
            }
        } else
        {
            if (__why2_get_asking_password())
            {
                //REMOVE \n AT THE END OF line
                line[strlen(line) - 1] = '\0';

                char *hash = why2_sha256(line); //HASHISH

                why2_send_socket(hash, NULL, listen_socket); //SEND BUT HASHED

                //DEALLOCATION
                why2_deallocate(hash);
                __why2_set_asking_password(0);
            } else
            {
                why2_send_socket(line, NULL, listen_socket); //NULL IS SENT BECAUSE IT IS USELESS TO SEND USER FROM CLIENT - SERVER WON'T USE IT
            }

            free(line);
        }
    }

    //DEALLOCATION
    if (!ssqc)
    {
        pthread_cancel(thread_buffer);
        free(line);
    }

    why2_deallocate(cmd_arg);
    why2_chat_deallocate_keys(); //DEALLOCATE GETTERS FOR KEYS

    why2_clean_memory(""); //RUN GARBAGE COLLECTOR

    return 0;
}