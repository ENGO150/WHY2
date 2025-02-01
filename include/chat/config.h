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

#ifndef WHY2_CHAT_CONFIG_H
#define WHY2_CHAT_CONFIG_H

#ifdef __cplusplus
extern "C" {
#endif

#include <why2/flags.h> //damn i really hate including headers in headers

//CONFIG MACROS
#define WHY2_CHAT_CONFIG_URL "https://raw.githubusercontent.com/ENGO150/WHY2/development/src/chat/config/"
#define WHY2_CHAT_CONFIG_SERVER "server.toml"
#define WHY2_CHAT_CONFIG_CLIENT "client.toml"
#define WHY2_CHAT_CONFIG_SERVER_USERS "server_users.toml" //LOGIN INFO CONFIG

#define WHY2_CHAT_AUTHORITY_DIR "certs" //AUTHORITY DIRECTORY
#define WHY2_CHAT_AUTHORITY_CERTS_EXTENSION ".cert"

void why2_chat_init_server_config(void); //CHECK IF SERVER CONFIG EXISTS, CREATE IT
void why2_chat_init_client_config(void); //Dementia is a term used to describe a group of symptoms affecting memory, thinking and social abilities. In people who have dementia, the symptoms interfere with their daily lives. Dementia isn't one specific disease. Several diseases can cause dementia. ...

void why2_chat_init_authority(void); //CREATE WHY2_CHAT_AUTHORITY_DIR

char *why2_toml_read(const char* path, const char* key); //READ key FROM path TOML FILE
void why2_toml_write(const char *path, const char *key, const char *value); //WRITE value AS key INTO path TOML FILE
void why2_toml_write_preserve(const char *path, const char *key, const char *value); //WRITE value AS key INTO path TOML FILE WITHOUT REMOVING COMMENTS
why2_bool why2_toml_contains(const char *path, const char *key); //CHECK IF path CONTAINS key
why2_bool why2_toml_equals(const char *path, const char *key, const char *value); //CHECK IF key IN path IS EQUAL TO value
void why2_toml_read_free(char* s); //DEALLOCATE THE READ VALUE

char *why2_chat_server_config(char *key); //why2_toml_read BUT YOU DO NOT HAVE TO INCLUDE path
char *why2_chat_client_config(char *key); //hihi, *grabs shotgun quietly*

char *why2_get_server_users_path(void); //RETURNS WHY2_CHAT_CONFIG_SERVER_USERS LOCATION
char *why2_get_client_config_path(void); //RETURNS WHY2_CHAT_CONFIG_CLIENT LOCATION
char *why2_get_authority_cert_path(char *username); //RETURNS PATH TO CA CERTS

#ifdef __cplusplus
}
#endif

#endif