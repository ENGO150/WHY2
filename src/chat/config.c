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
#include <unistd.h>

#include <curl/curl.h>

#include <why2/chat/flags.h>
#include <why2/flags.h>

void why2_chat_init_config(void)
{
    if (access(WHY2_CHAT_CONFIG, R_OK) != 0) //CONFIG DOESN'T EXIST
    {
        CURL *curl = curl_easy_init();
        FILE *fileBuffer = why2_fopen(WHY2_CHAT_CONFIG, "w+");

        curl_easy_setopt(curl, CURLOPT_URL, WHY2_CHAT_CONFIG_URL);
        curl_easy_setopt(curl, CURLOPT_WRITEDATA, fileBuffer);
        curl_easy_setopt(curl, CURLOPT_TIMEOUT, WHY2_CURL_TIMEOUT);
        curl_easy_perform(curl);

        //CLEANUP
        curl_easy_cleanup(curl);
        why2_deallocate(fileBuffer);
    }
}