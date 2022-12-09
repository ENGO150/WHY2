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

#include <why2/encrypter.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/time.h>

#include <why2/flags.h>
#include <why2/misc.h>

outputFlags encryptText(char *text, char *keyNew)
{
    //CHECK VARIABLE
    unsigned char checkExitCode;

    //TIME VARIABLES
    struct timeval startTime;
    struct timeval finishTime;
    gettimeofday(&startTime, NULL);

    //CHECK FOR ACTIVE VERSION
    if ((checkExitCode = checkVersion()) != SUCCESS)
    {
        return noOutput(checkExitCode);
    }

    //CHECK FOR INVALID text
    if ((checkExitCode = checkText(text)) != SUCCESS)
    {
        return noOutput(checkExitCode);
    }

    //VARIABLES
    char *key = malloc(getKeyLength() + 1);
    char *returningText;
    char *textBuffer = malloc(1);
    int *textKeyChain = malloc(sizeof(int) * strlen(text));
    int numberBuffer;
    FILE *fileBuffer;

    //TRY TO MAKE RANDOM GENERATION REALLY "RANDOM"
    fileBuffer = fopen("/dev/urandom", "r");
    (void) (fread(&numberBuffer, sizeof(numberBuffer), 1, fileBuffer) + 1); //TODO: Try to create some function for processing exit value
    srand(numberBuffer);
    numberBuffer = abs(numberBuffer); //MAKE numberBuffer POSITIVE

    if (keyNew != NULL)
    {
        //CHECK FOR INVALID key
        if ((checkExitCode = checkKey(keyNew)) != SUCCESS)
        {
            return noOutput(checkExitCode);
        }

        strcpy(key, keyNew);

        //REDEFINE keyLength
        setKeyLength(strlen(key));

        goto skipKey;
    }

    //LOAD KEY
    for (int i = 0; i < (int) getKeyLength(); i++)
    {
        //SET numberBuffer TO RANDOM NUMBER BETWEEN 0 AND 52
        numberBuffer = (rand() % 52) + 1;

        //GET CHAR FROM numberBuffer
        if (numberBuffer > 26)
        {
            numberBuffer += 70;
        }
        else
        {
            numberBuffer += 64;
        }

        key[i] = (char) numberBuffer;
    }

    key[getKeyLength()] = '\0';

    skipKey:

    //LOAD textKeyChain
    generateTextKeyChain(key, textKeyChain, strlen(text));

    //ACTUALLY ENCRYPT TEXT
    for (int i = 0; i < (int) strlen(text); i++)
    {
        textKeyChain[i] = getEncryptionOperation()(textKeyChain[i], (int) text[i]);
    }

    //COUNT REQUIRED SIZE FOR returningText
    for (int i = 0; i < (int) strlen(text); i++)
    {
        numberBuffer += countIntLength(textKeyChain[i]);
    }

    //ALLOCATE returningText (WITH THE SEPARATORS)
    returningText = malloc(numberBuffer + strlen(text) - 1);
    strcpy(returningText, "");

    //LOAD returningText
    for (int i = 0; i < (int) strlen(text); i++)
    {
        numberBuffer = sizeof(int) * countIntLength(textKeyChain[i]);

        textBuffer = realloc(textBuffer, numberBuffer);

        sprintf(textBuffer, "%d", textKeyChain[i]);

        strcat(returningText, textBuffer);

        if (i != (int) strlen(text) - 1)
        {
            textBuffer = realloc(textBuffer, 2);
            sprintf(textBuffer, "%c", getEncryptionSeparator());

            strcat(returningText, textBuffer);
        }
    }

    //GET FINISH TIME
    gettimeofday(&finishTime, NULL);

    //LOAD output
    outputFlags output =
    {
        returningText, //ENCRYPTED TEXT
        key, //GENERATED/USED KEY
        countUnusedKeySize(text, key), // NUMBER OF UNUSED CHARS IN KEY
        countRepeatedKeySize(text, key), //NUMBER OF REPEATED CHARS IN KEY
        compareTimeMicro(startTime, finishTime), // ELAPSED TIME
        SUCCESS //EXIT CODE
    };

    //DEALLOCATION
    free(textKeyChain);
    free(textBuffer);
    fclose(fileBuffer);

    return output;
}
