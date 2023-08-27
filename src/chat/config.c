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

#include <why2/chat/flags.h>
#include <why2/flags.h>
#include <why2/memory.h>
#include <why2/misc.h>

//LOCAL
void init_config(char *filename)
{
    struct stat st;
    char *buffer = why2_replace(WHY2_CONFIG_DIR, "{USER}", getenv("USER"));

    //CREATE USER CONFIG FOLDER [THIS SHOULDN'T HAPPEN ON CLIENT, BUT IT'S NEEDED ON FRESH SERVERS]
    if (stat(buffer, &st) == -1)
    {
        mkdir(buffer, 0700);
    }

    //GET THE CONFIG TYPE
    buffer = why2_realloc(buffer, strlen(WHY2_CHAT_CONFIG_DIR) + strlen(filename) + 2);
    sprintf(buffer, "%s/%s", WHY2_CHAT_CONFIG_DIR, filename);

    char *path = why2_replace(buffer, "{USER}", getenv("USER"));

    if (access(path, R_OK) != 0) //CONFIG DOESN'T EXIST
    {
        char *config_dir = why2_replace(WHY2_CHAT_CONFIG_DIR, "{USER}", getenv("USER"));

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
        why2_deallocate(buffer);
        why2_deallocate(path);
        why2_deallocate(config_dir);
        why2_deallocate(file_buffer);
    }
}

//GLOBAL
void why2_chat_init_server_config(void)
{
    init_config(WHY2_CHAT_CONFIG_SERVER);
}

void why2_chat_init_client_config(void)
{
    init_config(WHY2_CHAT_CONFIG_CLIENT);
}