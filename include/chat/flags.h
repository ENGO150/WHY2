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

#ifndef WHY2_CHAT_COMMON_H
#define WHY2_CHAT_COMMON_H

#ifdef __cplusplus
extern "C" {
#endif

//MACROS
#define WHY2_SA struct sockaddr
#define WHY2_CHAT_SERVER_PORT 1204 //PORT
#define WHY2_MAX_CONNECTIONS 1000 //MAX USERS CONNECTED AT ONE TIME

#define WHY2_CLEAR_AND_GO_UP "\33[2K\r\033[A" //i mean read the name

#define WHY2_INVALID_POINTER (void*) 0xffffffffffffffff

//(SERVER -> CLIENT) CODES
#define WHY2_CHAT_CODE_ACCEPT_MESSAGES "code_000" //TELL CLIENT THEY CAN SEND MESSAGES
#define WHY2_CHAT_CODE_PICK_USERNAME "code_001" //TELL CLIENT TO PICK USERNAME
#define WHY2_CHAT_CODE_SERVER_SIDE_QUIT_COMMUNICATION "code_002" //TELL CLIENT TO END COMMUNICATION (just so they don't get segfault on server-side disconnect)
#define WHY2_CHAT_CODE_INVALID_USERNAME "code_003" //haha
#define WHY2_CHAT_CODE_LIST_SERVER "code_004" //SAME AS WHY2_CHAT_CODE_LIST BUT BACK TO THE CLIENT
#define WHY2_CHAT_CODE_VERSION_SERVER "code_005" //SAME AS WHY2_CHAT_CODE_VERSION BUT BACK TO THE CLIENT
#define WHY2_CHAT_CODE_PM_SERVER "code_006" //SAME AS WHY2_CHAT_CODE_PM BUT BACK TO THE CLIENT
#define WHY2_CHAT_CODE_ENTER_PASSWORD "code_007" //RECEIVE PASSWORD FROM USER
#define WHY2_CHAT_CODE_INVALID_PASSWORD "code_008"//🌸ꗥ～ꗥ🌸 𝐢 𝐡𝐚𝐭𝐞 𝐲𝐨𝐮 🌸ꗥ～ꗥ🌸

//(CLIENT -> SERVER) CODES
#define WHY2_CHAT_CODE_EXIT "code_999" //TELL SERVER YOU ARE ENDING COMMUNICATION
#define WHY2_CHAT_CODE_LIST "code_998" //TELL SERVER TO GIVE YOU ALL CONNECTED USERS
#define WHY2_CHAT_CODE_PM "code_997" //TELL SERVER TO SEND MESSAGE ONLY TO SPECIFIC ID
#define WHY2_CHAT_CODE_VERSION "code_996" //TELL SERVER TO GIVE YOU ITS VERSION

//COMMANDS
#define WHY2_CHAT_COMMAND_PREFIX "!" //the little thingy you write before the command names to make the program recognise them boy. You know? Like in minecraft you use /kill... Also, are you dumb?
#define WHY2_CHAT_COMMAND_EXIT "exit" //QUIT THE PROGRAM CMD
#define WHY2_CHAT_COMMAND_HELP "help" //PRINT ALL COMMANDS
#define WHY2_CHAT_COMMAND_LIST "list" //LIST ALL CONNECTIONS
#define WHY2_CHAT_COMMAND_PM "pm" //PRIVATE MESSAGES
#define WHY2_CHAT_COMMAND_VERSION "version" //COMPARE CLIENT VERSION AND SERVER VERSION

//SHORTCUTS CAUSE I'M LAZY BITCH
#define WHY2_CHAT_CODE_SSQC WHY2_CHAT_CODE_SERVER_SIDE_QUIT_COMMUNICATION

//FUNCTIONS
void __why2_set_asking_password(unsigned char asking_password); //IF HASH SHOULD BE SENT INSTEAD OF NORMAL MESSAGE
void __why2_reset_asking_password();

#ifdef __cplusplus
}
#endif

#endif