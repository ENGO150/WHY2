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

#include <why2/chat/config.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#include <curl/curl.h>

#include <why2/flags.h>
#include <why2/memory.h>
#include <why2/misc.h>

//LOCAL
enum CONFIG_TYPES
{
    CLIENT,
    SERVER,
    SERVER_USERS,
    AUTHORITY
};

void init_config(char *filename)
{
    //CREATE USER CONFIG FOLDER [THIS SHOULDN'T HAPPEN ON CLIENT, BUT IT'S NEEDED ON FRESH SERVERS]
    why2_directory();

    //GET THE CONFIG TYPE
    char *buffer = why2_malloc(strlen(WHY2_CONFIG_DIR) + strlen(filename) + 2);
    sprintf(buffer, "%s/%s", WHY2_CONFIG_DIR, filename);

    char *path = why2_replace(buffer, "{HOME}", getenv("HOME"));

    if (access(path, R_OK) != 0) //CONFIG DOESN'T EXIST
    {
        char *config_dir = why2_replace(WHY2_CONFIG_DIR, "{HOME}", getenv("HOME"));

        //CREATE CONFIG DIRECTORY
        mkdir(config_dir, 0700);

        CURL *curl = curl_easy_init();
        FILE *file_buffer = why2_fopen(path, "w+");

        buffer = why2_realloc(buffer, strlen(WHY2_CHAT_CONFIG_URL) + strlen(filename) + 1);
        sprintf(buffer, "%s%s", WHY2_CHAT_CONFIG_URL, filename);

        //DOWNLOAD CONTENT
        curl_easy_setopt(curl, CURLOPT_URL, buffer);
        curl_easy_setopt(curl, CURLOPT_WRITEDATA, file_buffer);
        curl_easy_setopt(curl, CURLOPT_TIMEOUT, WHY2_CURL_TIMEOUT);
        curl_easy_perform(curl);

        //CLEANUP
        curl_easy_cleanup(curl);
        why2_deallocate(config_dir);
        why2_deallocate(file_buffer);
    }

    //DEALLOCATION
    why2_deallocate(path);
    why2_deallocate(buffer);
}

char *config_path(enum CONFIG_TYPES type)
{
    char *path = NULL;

    switch (type) //GET path
    {
        case CLIENT:
            path = why2_replace(WHY2_CONFIG_DIR "/" WHY2_CHAT_CONFIG_CLIENT, "{HOME}", getenv("HOME"));
            break;

        case SERVER:
            path = why2_replace(WHY2_CONFIG_DIR "/" WHY2_CHAT_CONFIG_SERVER, "{HOME}", getenv("HOME"));
            break;

        case SERVER_USERS:
            path = why2_replace(WHY2_CONFIG_DIR "/" WHY2_CHAT_CONFIG_SERVER_USERS, "{HOME}", getenv("HOME"));
            break;

        case AUTHORITY:
            path = why2_replace(WHY2_CONFIG_DIR "/" WHY2_CHAT_AUTHORITY_DIR, "{HOME}", getenv("HOME"));
            break;

        default:
            why2_die("CONFIG_TYPE not implemented!");
            break;
    }

    return path;
}

char *config(char *key, enum CONFIG_TYPES type)
{
    //GET CONFIGURATION PATH
    char *path = config_path(type);

    //VALUE
    char *value = why2_toml_read(path, key);

    //DEALLOCATION
    why2_deallocate(path);

    return value;
}

//GLOBAL
void why2_chat_init_server_config(void)
{
    init_config(WHY2_CHAT_CONFIG_SERVER);

    //CHECK FOR USER CONFIG
    char *user_pick_username = why2_chat_server_config("user_pick_username");
    char *config_path = why2_get_server_users_path();

    if (strcmp(user_pick_username, "true") == 0 && access(config_path, R_OK) != 0)
    {
        //CREATE THE CONFIG
        FILE *config_file = why2_fopen(config_path, "w");

        //WRITE SOMETHING POSITIVE TO THE CONFIG :) (i love you, ignore my aggressive ass)
        fwrite("#haha no users registered, what a loser lol", 1, 43, config_file);

        //CLEANUP
        why2_deallocate(config_file);
    }

    //DEALLOCATION
    why2_toml_read_free(user_pick_username);
    why2_deallocate(config_path);
}

void why2_chat_init_client_config(void)
{
    init_config(WHY2_CHAT_CONFIG_CLIENT);
}

void why2_chat_init_authority(void)
{
    //CREATE USER CONFIG FOLDER [THIS SHOULDN'T HAPPEN ON CLIENT, BUT IT'S NEEDED ON FRESH SERVERS]
    why2_directory();

    //GET THE CONFIG TYPE
    char *path = config_path(AUTHORITY);
    if (access(path, R_OK) != 0) mkdir(path, 0700); //DIRECTORY DOESN'T EXIST; CREATE IT

    //DEALLOCATION
    why2_deallocate(path);
}

char *why2_chat_server_config(char *key)
{
    return config(key, SERVER);
}

char *why2_chat_client_config(char *key)
{
    return config(key, CLIENT);
}

char *why2_get_server_users_path(void)
{
    return config_path(SERVER_USERS);
}

char *why2_get_client_config_path(void)
{
    return config_path(CLIENT);
}

char *why2_get_authority_cert_path(char *username)
{
    char *buffer = config_path(AUTHORITY);
    char *path = why2_malloc(strlen(buffer) + strlen(username) + strlen(WHY2_CHAT_AUTHORITY_CERTS_EXTENSION) + 3);

    //GET THE FILE
    sprintf(path, "%s/%s%s%c", buffer, username, WHY2_CHAT_AUTHORITY_CERTS_EXTENSION, '\0');

    //DEALLOCATION
    why2_deallocate(buffer);

    return path;
}