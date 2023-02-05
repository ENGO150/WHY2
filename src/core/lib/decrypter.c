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

#include <why2/decrypter.h>

#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <sys/time.h>

#include <why2/flags.h>
#include <why2/memory.h>
#include <why2/misc.h>

why2_output_flags why2_decrypt_text(char *text, char *keyNew)
{
    //CHECK VARIABLE
    unsigned char checkExitCode;

    //TIME VARIABLES
    struct timeval startTime;
    struct timeval finishTime;
    gettimeofday(&startTime, NULL);

    //CHECK FOR ACTIVE WHY2_VERSION
    if ((checkExitCode = why2_check_version()) != WHY2_SUCCESS)
    {
        return why2_no_output(checkExitCode);
    }

    //CHECK FOR INVALID text
    if ((checkExitCode = why2_check_text(text)) != WHY2_SUCCESS)
    {
        return why2_no_output(checkExitCode);
    }

    //CHECK FOR INVALID key
    if ((checkExitCode = why2_check_key(keyNew)) != WHY2_SUCCESS)
    {
        return why2_no_output(checkExitCode);
    }

    //REDEFINE keyLength
    why2_set_key_length(strlen(keyNew));

    //VARIABLES
    char *returningText;
    int numberBuffer = 1;
    int usedTextDeallocationBuffer = 0;
    char *textBuffer = NULL;
    int textKeyChainLength;
    int *textKeyChain;
    char *key = why2_strdup(keyNew); //COPY keyNew TO key
    int *encryptedTextKeyChain;
    char *used_text = why2_strdup(text); //COPY text TO used_text

    //GET LENGTH OF returningText AND textKeyChain
    for (int i = 0; i < (int) strlen(used_text); i++)
    {
        if (used_text[i] == why2_get_encryption_separator()) numberBuffer++;
    }

    //SET LENGTH (numberBuffer)
    returningText = why2_calloc(numberBuffer + 1, sizeof(char));
    textKeyChain = why2_malloc(sizeof(int) * numberBuffer);
    encryptedTextKeyChain = why2_malloc(sizeof(int) * numberBuffer);
    textKeyChainLength = numberBuffer;

    //LOAD textKeyChain
    why2_generate_text_key_chain(key, textKeyChain, numberBuffer);

    //LOAD encryptedTextKeyChain
    for (int i = 0; i < textKeyChainLength; i++)
    {
        numberBuffer = 0;

        //GET LENGTH OF EACH CHARACTER
        for (int j = 0; j < (int) strlen(used_text); j++)
        {
            if (used_text[j] == why2_get_encryption_separator()) break;

            numberBuffer++;
        }

        textBuffer = why2_realloc(textBuffer, numberBuffer + 1);

        //CLEAN (POSSIBLY EXISTING) JUNK in textBuffer
        for (int j = 0; j <= numberBuffer; j++)
        {
            textBuffer[j] = '\0';
        }

        //LOAD textBuffer
        for (int j = 0; j < (int) strlen(used_text); j++)
        {
            textBuffer[j] = used_text[j];

            if (numberBuffer == j) break;
        }

        encryptedTextKeyChain[i] = atoi(textBuffer);

        used_text += numberBuffer + 1;
        usedTextDeallocationBuffer += numberBuffer + 1;
    }

    //DECRYPT TEXT
    for (int i = 0; i < textKeyChainLength; i++)
    {
        textKeyChain[i] = why2_get_encryption_operation()(textKeyChain[i], encryptedTextKeyChain[i]);
    }

    //LOAD returningText
    for (int i = 0; i < textKeyChainLength; i++)
    {
        returningText[i] = textKeyChain[i];
    }

    //GET FINISH TIME
    gettimeofday(&finishTime, NULL);

    //LOAD output
    why2_output_flags output =
    {
        returningText, //DECRYPTED TEXT
        key, //USED KEY
        why2_count_unused_key_size(returningText, key), // NUMBER OF WHY2_UNUSED CHARS IN KEY
        why2_count_repeated_key_size(returningText, key), //NUMBER OF REPEATED CHARS IN KEY
        why2_compare_time_micro(startTime, finishTime), // ELAPSED TIME
        WHY2_SUCCESS //EXIT CODE
    };

    //DEALLOCATION
    why2_deallocate(textKeyChain);
    why2_deallocate(encryptedTextKeyChain);
    why2_deallocate(textBuffer);
    why2_deallocate(used_text - usedTextDeallocationBuffer);

    return output;
}
