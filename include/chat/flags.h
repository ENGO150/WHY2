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

#include <why2/flags.h>

//ENUMS
enum WHY2_CHAT_SERVER_TYPE //TYPE OF SERVER
{
    WHY2_CHAT_SERVER, //CLASSIC COMMUNICATION SERVER
    WHY2_CHAT_AUTHORITY //CA
};

//MACROS
#define WHY2_CHAT_SERVER_PORT 1204 //PORT FOR SERVER
#define WHY2_CHAT_AUTHORITY_PORT 1203 //PORT FOR CA
#define WHY2_MAX_CONNECTIONS 1000 //MAX USERS CONNECTED AT ONE TIME

#define WHY2_CLEAR_AND_GO_UP "\33[2K\r\033[A" //i mean read the name

#define WHY2_INVALID_POINTER (void*) 0xffffffffffffffff

//(SERVER -> CLIENT) CODES
#define WHY2_CHAT_CODE_ACCEPT_MESSAGES "SC0" //TELL CLIENT THEY CAN SEND MESSAGES
#define WHY2_CHAT_CODE_PICK_USERNAME "SC1" //TELL CLIENT TO PICK USERNAME
#define WHY2_CHAT_CODE_SERVER_SIDE_QUIT_COMMUNICATION "SC2" //TELL CLIENT TO END COMMUNICATION (just so they don't get segfault on server-side disconnect)
#define WHY2_CHAT_CODE_INVALID_USERNAME "SC3" //haha
#define WHY2_CHAT_CODE_LIST_SERVER "SC4" //SAME AS WHY2_CHAT_CODE_LIST BUT BACK TO THE CLIENT
#define WHY2_CHAT_CODE_VERSION_SERVER "SC5" //SAME AS WHY2_CHAT_CODE_VERSION BUT BACK TO THE CLIENT
#define WHY2_CHAT_CODE_DM_SERVER "SC6" //SAME AS WHY2_CHAT_CODE_DM BUT BACK TO THE CLIENT
#define WHY2_CHAT_CODE_ENTER_PASSWORD "SC7" //RECEIVE PASSWORD FROM USER
#define WHY2_CHAT_CODE_INVALID_PASSWORD "SC8"//🌸ꗥ～ꗥ🌸 𝐢 𝐡𝐚𝐭𝐞 𝐲𝐨𝐮 🌸ꗥ～ꗥ🌸

//(CLIENT -> SERVER) CODES
#define WHY2_CHAT_CODE_EXIT "CS1" //TELL SERVER YOU ARE ENDING COMMUNICATION
#define WHY2_CHAT_CODE_LIST "CS2" //TELL SERVER TO GIVE YOU ALL CONNECTED USERS
#define WHY2_CHAT_CODE_DM "CS3" //TELL SERVER TO SEND MESSAGE ONLY TO SPECIFIC ID
#define WHY2_CHAT_CODE_VERSION "CS4" //TELL SERVER TO GIVE YOU ITS VERSION
#define WHY2_CHAT_CODE_USERNAME "CS5" //TELL SERVER YOU ARE GIVING IT YOUR USERNAME
#define WHY2_CHAT_CODE_PASSWORD "CS6" //TELL SERVER YOU ARE GIVING IT YOUR PASSWORD (HASHED)

//(AUTHORITY -> CLIENT) CODES
#define WHY2_CHAT_CODE_KEY_EXCHANGE "AC0" //TELL CLIENT YOU ARE SENDING YOUR PUBLIC KEY
#define WHY2_CHAT_CODE_SUCCESS "AC1" //TELL CLIENT THEY ARE GOOD TO GO
#define WHY2_CHAT_CODE_FAILURE "AC2" //TELL CLIENT THEY FUCKED UP

//(CLIENT -> AUTHORITY) CODES
#define WHY2_CHAT_CODE_CLIENT_KEY_EXCHANGE "CA0" //TELL AUTHORITY YOU ARE SENDING YOUR PUBLIC KEY

//COMMANDS
#define WHY2_CHAT_COMMAND_PREFIX "!" //the little thingy you write before the command names to make the program recognise them boy. You know? Like in minecraft you use /kill... Also, are you dumb?
#define WHY2_CHAT_COMMAND_EXIT "exit" //QUIT THE PROGRAM CMD
#define WHY2_CHAT_COMMAND_HELP "help" //PRINT ALL COMMANDS
#define WHY2_CHAT_COMMAND_LIST "list" //LIST ALL CONNECTIONS
#define WHY2_CHAT_COMMAND_DM "dm" //DIRECT (UNENCRYPTED) MESSAGES
#define WHY2_CHAT_COMMAND_VERSION "version" //COMPARE CLIENT VERSION AND SERVER VERSION
#define WHY2_CHAT_COMMAND_PM "pm" //PRIVATE (ENCRYPTED) MESSAGES

//SHORTCUTS CAUSE I'M LAZY BITCH
#define WHY2_CHAT_CODE_SSQC WHY2_CHAT_CODE_SERVER_SIDE_QUIT_COMMUNICATION

//FUNCTIONS
void __why2_set_asking_password(why2_bool value); //IF HASH SHOULD BE SENT INSTEAD OF NORMAL MESSAGE
why2_bool __why2_get_asking_password();

void __why2_set_asking_username(why2_bool value);
why2_bool __why2_get_asking_username();

#ifdef __cplusplus
}
#endif

#endif