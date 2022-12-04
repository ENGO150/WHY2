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

#include <stdlib.h>

//CONSTS (this is just local)
#define DEFAULT_FLAGS (inputFlags) { 0, 0, 0 }

int encryptionOperation(int text, int encryptedText);

//VARIABLES
char encryptionSeparator = '.'; //NOPE     > DO NOT TOUCH THIS, USE setEncryptionSeparator instead <
unsigned long keyLength = 50; //LENGTH OF KEY     > DO NOT TOUCH THIS, USE setKeyLength instead <
inputFlags flags = DEFAULT_FLAGS;
encryptionOperation_type_cb encryptionOperation_cb = encryptionOperation;

//GETTERS
char getEncryptionSeparator()
{
    return encryptionSeparator;
}

unsigned long getKeyLength()
{
    return keyLength;
}

inputFlags defaultFlags()
{
    return DEFAULT_FLAGS;
}

inputFlags getFlags()
{
    return flags;
}

outputFlags noOutput(boolean exitCode)
{
    char *emptyText = malloc(1); //TEXT
    emptyText[0] = '\0';

    char *emptyKey = malloc(getKeyLength() + 1); //KEY
    for (int i = 0; i < (int) getKeyLength(); i++)
    {
        emptyKey[i] = 'x';
    }
    emptyKey[getKeyLength()] = '\0';

    return (outputFlags) { emptyText, emptyKey, 0, 0, 0, exitCode };
}

encryptionOperation_type_cb getEncryptionOperation()
{
    return encryptionOperation_cb;
}

//SETTERS
void setEncryptionSeparator(char encryptionSeparatorNew)
{
    if (encryptionSeparatorNew <= 31) return;

    encryptionSeparator = encryptionSeparatorNew;
}

void setKeyLength(int keyLengthNew)
{
    if (keyLengthNew < 1) return;

    keyLength = keyLengthNew;
}

void setFlags(inputFlags newFlags)
{
    flags = newFlags;
}

void setEncryptionOperation(encryptionOperation_type_cb newEncryptionOperation)
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