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

//MACROS
#define WHY2_CHAT_CODE_ACCEPT_MESSAGES "code_000"
#define WHY2_CHAT_CODE_PICK_USERNAME "code_001"
#define WHY2_CHAT_CODE_SERVER_SIDE_QUIT_COMMUNICATION "code_002"

//SHORTCUTS CAUSE I'M LAZY BITCH
#define WHY2_CHAT_CODE_SSQC WHY2_CHAT_CODE_SERVER_SIDE_QUIT_COMMUNICATION

#define WHY2_CONFIG_DIR "/home/{USER}/.config"
#define WHY2_CHAT_CONFIG_DIR WHY2_CONFIG_DIR "/WHY2"
#define WHY2_CHAT_CONFIG_URL "https://raw.githubusercontent.com/ENGO150/WHY2/development/src/chat/"
#define WHY2_CHAT_CONFIG_SERVER "server.yml"
#define WHY2_CHAT_CONFIG_CLIENT "client.yml"

void why2_chat_init_server_config(void); //CHECK IF SERVER CONFIG EXISTS, CREATE IT
void why2_chat_init_client_config(void); //Dementia is a term used to describe a group of symptoms affecting memory, thinking and social abilities. In people who have dementia, the symptoms interfere with their daily lives. Dementia isn't one specific disease. Several diseases can cause dementia. ...

char *why2_config_read(const char* path, const char* key);
void why2_config_read_free(char* s);

#ifdef __cplusplus
}
#endif

#endif