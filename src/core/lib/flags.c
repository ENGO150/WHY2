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

#include <why2/flags.h>

#include <why2/memory.h>

//CONSTS (this is just local)
#define DEFAULT_FLAGS (why2_input_flags) { 0, 0, 0 }

int encryptionOperation(int text, int encryptedText);

//VARIABLES
char encryptionSeparator = '.'; //NOPE     > DO NOT TOUCH THIS, USE why2_set_encryption_separator instead <
unsigned long keyLength = 50; //LENGTH OF KEY     > DO NOT TOUCH THIS, USE why2_set_key_length instead <
why2_input_flags flagsAllah = DEFAULT_FLAGS; //IT IS CALLED flagsAllah CUZ flags CAUSED SOME FUCKING MEMORY PROBLEMS
why2_encryption_operation_cb encryptionOperation_cb = encryptionOperation;
why2_bool flagsChanged = 0; //CHANGES TO 1 WHEN U USE why2_set_flags

//GETTERS
char why2_get_encryption_separator(void)
{
    return encryptionSeparator;
}

unsigned long why2_get_key_length(void)
{
    return keyLength;
}

why2_input_flags why2_get_default_flags(void)
{
    return DEFAULT_FLAGS;
}

why2_input_flags why2_get_flags(void)
{
    return flagsAllah;
}

why2_output_flags why2_no_output(enum WHY2_EXIT_CODES exitCode)
{
    char *emptyText = why2_malloc(1); //TEXT
    emptyText[0] = '\0';

    char *emptyKey = why2_malloc(why2_get_key_length() + 1); //KEY
    for (int i = 0; i < (int) why2_get_key_length(); i++)
    {
        emptyKey[i] = 'x';
    }
    emptyKey[why2_get_key_length()] = '\0';

    return (why2_output_flags) { emptyText, emptyKey, 0, 0, 0, exitCode };
}

why2_encryption_operation_cb why2_get_encryption_operation(void)
{
    return encryptionOperation_cb;
}

why2_bool why2_get_flags_changed(void)
{
    return flagsChanged;
}

//SETTERS
void why2_set_encryption_separator(char encryptionSeparatorNew)
{
    if (encryptionSeparatorNew <= 31) return;

    encryptionSeparator = encryptionSeparatorNew;
}

void why2_set_key_length(int keyLengthNew)
{
    if (keyLengthNew < 1) return;

    keyLength = keyLengthNew;
}

void why2_set_flags(why2_input_flags newFlags)
{
    flagsAllah = newFlags;

    if (!flagsChanged) flagsChanged = 1;
}

void why2_set_encryption_operation(why2_encryption_operation_cb newEncryptionOperation)
{
    encryptionOperation_cb = newEncryptionOperation;
}

//SOME OTHER SHIT
int encryptionOperation(int text, int encryptedText)
{
    //CHANGE THE '-' (MINUS) OPERATOR TO SOMETHING YOU WANT TO USE I GUESS
    return text - encryptedText;

    //I DO NOT RECOMMEND CHANGING THIS, BUT IF YOU WANT TO, XOR IS A GOOD OPERATOR (IDK IF OTHERS WORK lmao)
}