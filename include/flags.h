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

#ifndef WHY2_FLAGS_H
#define WHY2_FLAGS_H

//CONSTS
const enum WHY2_EXIT_CODES //exit codes you fucking idiot
{
    WHY2_SUCCESS = 0, //EXIT CODE FOR WHY2_SUCCESSFUL RUN
    WHY2_INVALID_KEY = 1, //EXIT VALUE FOR INVALID KEY
    WHY2_INVALID_TEXT = 4, //EXIT VALUE FOR INVALID TEXT
    WHY2_DOWNLOAD_FAILED = 2, //EXIT VALUE FOR versions.json DOWNLOAD FAILED
    WHY2_WHY2_UPDATE_FAILED = 3 //EXIT VALUE FOR UPDATE FAILED
};

#define WHY2_VERSION "v4.3.2" //WHY2_VERSION OF CURRENT BUILD     > DO NOT TOUCH THIS <
#define WHY2_VERSIONS_URL "https://raw.githubusercontent.com/ENGO150/WHY2/release/versions.json" //URL FOR GETTING versions.json
#define WHY2_VERSIONS_NAME "/tmp/why2-versions.json" //do I have to explain this?

#define WHY2_UPDATE_URL "https://github.com/ENGO150/WHY2.git" // REPOSITORY URL FOR UPDATES (YOU DON'T SAY)
#define WHY2_UPDATE_NAME "/tmp/why2-update" // fuck you
#define WHY2_UPDATE_COMMAND "tmux new-session -d \"cd {DIR} && make install\""

#define WHY2_TEST_TEXT "aAzZ(    )!?#\\/śŠ <3|420*;㍿㊓ㅅΔ♛👶🏿" //TEST TEXT FOR ENCRYPTION IN why2-test BINARY

#define WHY2_TEXT_TO_ENCRYPT "Some text yk" //THIS TEXT WILL BE ENCRYPTED IN why2-app BINARY

#define WHY2_CLEAR_SCREEN "\e[1;1H\e[2J" //TEXT FOR UNIX CLEAR SCREEN

#define WHY2_CURL_TIMEOUT 3 //if you need comment explaining, what the fuck is timeout, don't change WHY2's code, alright? thx, love ya
#define WHY2_NOT_FOUND_TRIES 10 //NUMBER OF TRIES FOR DOWNLOADING versions.json

#define WHY2_DEPRECATED __attribute__((deprecated)) //SAME COMMENT AS WHY2_VERSIONS_NAME'S
#define WHY2_UNUSED __attribute__((unused)) //SAME COMMENT AS WHY2_DEPRECATED'S

//TYPES
typedef char why2_bool; //READ THE NAME OR I WILL FIND YOU AND FUCK YOUR MOTHERFUCKING DOG!!!
typedef int (*why2_encryption_operation_cb)(int, int); //TYPE FOR encryptionOperation CALLBACK
typedef struct
{
    why2_bool noCheck; //BOOLEAN FOR SKIPPING WHY2_VERSION CHECK
    why2_bool why2_no_output; //BOOLEAN FOR NOT PRINTING OUTPUT WHEN ENCRYPTING/DECRYPTING
    why2_bool update; //BOOLEAN FOR UPDATING YOUR WHY WHY2_VERSION IF OLD IS USED
} why2_input_flags;

typedef struct
{
    char *outputText; //VARIABLE FOR ENCRYPTED/DECRYPTED TEXT
    char *usedKey; //VARIABLE FOR USED/GENERATED KEY
    unsigned long unusedKeySize; //VARIABLE FOR COUNT OF WHY2_UNUSED CHARACTERS IN KEY
    unsigned long repeatedKeySize; //VARIABLE FOR COUNT OF REPEATED CHARACTERS IN KEY (basically reversed unusedKeySize)
    unsigned long elapsedTime; //VARIABLE FOR ELAPSED TIME IN MICROSECONDS => 1s = 1000000µs
    enum WHY2_EXIT_CODES exitCode; //VARIABLE FOR EXIT CODE
} why2_output_flags;

//NOTE: Variables were moved to 'flags.c' to force y'all using getters

//GETTERS
char why2_get_encryption_separator(void);
unsigned long why2_get_key_length(void);
why2_input_flags why2_get_default_flags(void); //THIS GENERATES why2_input_flags WITH DEFAULT VALUES
why2_input_flags why2_get_flags(void); //RETURNS USED FLAGS
why2_output_flags why2_no_output(enum WHY2_EXIT_CODES exitCode); //SAME AS why2_get_default_flags() BUT FOR why2_output_flags
why2_encryption_operation_cb why2_get_encryption_operation(void); //RETURNS FUNCTION WHICH IS USED FOR ENCRYPTION & DECRYPTION
why2_bool why2_get_flags_changed(void);
char *why2_get_memory_identifier(void); //RETURNS STRING USED IN LINKED LIST (IN memory.c) FOR IDENTIFYING NODES WHEN RUNNING GARBAGE COLLECTOR

//SETTERS
void why2_set_encryption_separator(char encryptionSeparatorNew);
void why2_set_key_length(int keyLengthNew);
void why2_set_flags(why2_input_flags newFlags); //.... whatcha think?
void why2_set_encryption_operation(why2_encryption_operation_cb newEncryptionOperation); //are you that dumb?
void why2_set_memory_identifier(char *new_memory_identifier);
void why2_reset_memory_identifier(void); //hmmm, what could reset mean.... huh


#endif
