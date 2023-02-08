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
#include <string.h>
#include <sys/time.h>

#include <why2/flags.h>
#include <why2/memory.h>
#include <why2/misc.h>

why2_output_flags why2_encrypt_text(char *text, char *keyNew)
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

    why2_set_memory_identifier("core_decrypt");

    //VARIABLES
    char *key = NULL;
    char *returningText;
    char *textBuffer = NULL;
    int *textKeyChain = why2_malloc(sizeof(int) * strlen(text));
    int numberBuffer = 0;

    if (keyNew != NULL)
    {
        //CHECK FOR INVALID key
        if ((checkExitCode = why2_check_key(keyNew)) != WHY2_SUCCESS)
        {
            why2_clean_memory("core_decrypt");
            return why2_no_output(checkExitCode);
        }

        key = why2_strdup(keyNew);

        //REDEFINE keyLength
        why2_set_key_length(strlen(key));
    } else //LOAD KEY
    {
        key = why2_generate_key(why2_get_key_length());
    }

    //LOAD textKeyChain
    why2_generate_text_key_chain(key, textKeyChain, strlen(text));

    //ACTUALLY ENCRYPT TEXT
    for (int i = 0; i < (int) strlen(text); i++)
    {
        textKeyChain[i] = why2_get_encryption_operation()(textKeyChain[i], (int) text[i]);
    }

    //COUNT REQUIRED SIZE FOR returningText
    for (int i = 0; i < (int) strlen(text); i++)
    {
        numberBuffer += why2_count_int_length(textKeyChain[i]);
    }

    //ALLOCATE returningText (WITH THE SEPARATORS)
    returningText = why2_calloc(numberBuffer + strlen(text), sizeof(char));

    //LOAD returningText
    for (int i = 0; i < (int) strlen(text); i++)
    {
        numberBuffer = sizeof(int) * why2_count_int_length(textKeyChain[i]);

        textBuffer = why2_realloc(textBuffer, numberBuffer);

        sprintf(textBuffer, "%d", textKeyChain[i]);

        strcat(returningText, textBuffer);

        if (i != (int) strlen(text) - 1)
        {
            textBuffer = why2_realloc(textBuffer, 2);
            sprintf(textBuffer, "%c", why2_get_encryption_separator());

            strcat(returningText, textBuffer);
        }
    }

    //GET FINISH TIME
    gettimeofday(&finishTime, NULL);

    //LOAD output
    why2_output_flags output =
    {
        returningText, //ENCRYPTED TEXT
        key, //GENERATED/USED KEY
        why2_count_unused_key_size(text, key), // NUMBER OF WHY2_UNUSED CHARS IN KEY
        why2_count_repeated_key_size(text, key), //NUMBER OF REPEATED CHARS IN KEY
        why2_compare_time_micro(startTime, finishTime), // ELAPSED TIME
        WHY2_SUCCESS //EXIT CODE
    };

    //DEALLOCATION
    why2_deallocate(textKeyChain);
    why2_deallocate(textBuffer);

    why2_reset_memory_identifier();

    return output;
}
