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

why2_output_flags why2_encrypt_text(char *text, char *key_new)
{
    //CHECK VARIABLE
    unsigned char check_exit_code;

    //TIME VARIABLES
    struct timeval start_time;
    struct timeval finish_time;
    gettimeofday(&start_time, NULL);

    //CHECK FOR ACTIVE WHY2_VERSION
    if ((check_exit_code = why2_check_version()) != WHY2_SUCCESS)
    {
        return why2_no_output(check_exit_code);
    }

    //CHECK FOR INVALID text
    if ((check_exit_code = why2_check_text(text)) != WHY2_SUCCESS)
    {
        return why2_no_output(check_exit_code);
    }

    why2_set_memory_identifier("core_decrypt");

    //VARIABLES
    char *key = NULL;
    char *returning_text = NULL;
    char *text_buffer = NULL;
    int *text_key_chain = why2_malloc(sizeof(int) * strlen(text));
    int number_buffer = 0;

    if (key_new != NULL)
    {
        //CHECK FOR INVALID key
        if ((check_exit_code = why2_check_key(key_new)) != WHY2_SUCCESS)
        {
            why2_clean_memory("core_decrypt");
            return why2_no_output(check_exit_code);
        }

        key = why2_strdup(key_new);

        //REDEFINE keyLength
        why2_set_key_length(strlen(key));
    } else //LOAD KEY
    {
        key = why2_generate_key(why2_get_key_length());
    }

    //LOAD text_key_chain
    why2_generate_text_key_chain(key, text_key_chain, strlen(text));

    //ACTUALLY ENCRYPT TEXT
    for (int i = 0; i < (int) strlen(text); i++)
    {
        text_key_chain[i] = why2_get_encryption_operation()(text_key_chain[i], (int) text[i]);
    }

    //OUTPUT FORMATS
    if (why2_get_flags().format == WHY2_OUTPUT_TEXT) //NORMAL 420.-69 FORMAT
    {
        //COUNT REQUIRED SIZE FOR returning_text
        for (int i = 0; i < (int) strlen(text); i++)
        {
            number_buffer += why2_count_int_length(text_key_chain[i]);
        }

        //ALLOCATE returning_text (WITH THE SEPARATORS)
        returning_text = why2_calloc(number_buffer + strlen(text), sizeof(char));

        //LOAD returning_text
        for (int i = 0; i < (int) strlen(text); i++)
        {
            number_buffer = sizeof(int) * why2_count_int_length(text_key_chain[i]);

            text_buffer = why2_realloc(text_buffer, number_buffer);

            sprintf(text_buffer, "%d", text_key_chain[i]);

            strcat(returning_text, text_buffer);

            if (i != (int) strlen(text) - 1)
            {
                returning_text[strlen(returning_text)] = why2_get_encryption_separator();
            }
        }
    } else if (why2_get_flags().format == WHY2_OUTPUT_BYTE) //FUCKED BUT SHORT(ER) OUTPUT
    {
        number_buffer = (strlen(text) + 1) * 2; //EACH CHARACTER WILL BE SPLIT INTO TWO CHARS AND FIRST TWO WILL BE LENGTH OF text

        returning_text = why2_calloc(number_buffer + 1, sizeof(char)); //ALLOCATE

        //SET LENGTH
        returning_text[0] = (strlen(text) & 0x7f) + 1; //+1 BECAUSE WE DON'T WANT \0
        returning_text[1] = (strlen(text) >> 7) + 1;

        //PUT THE text_key_chain INTO returning_text DIRECTLY
        for (unsigned long i = 0; i < strlen(text); i++)
        {
            //BUILD returning_text
            returning_text[2 + (i * 2)] = text_key_chain[i] & 0x7f;
            returning_text[3 + (i * 2)] = (text_key_chain[i] >> 7) | ((text_key_chain[i] < 0) ? 0x80 : 0);

            for (unsigned long j = 2 + (i * 2); j <= 3 + (i * 2); j++)
            {
                //ENSURE THERE IS NO \0
                if (returning_text[j] > 0)
                {
                    returning_text[j]++;
                } else
                {
                    returning_text[j]--;
                }
            }
        }
    }

    //GET FINISH TIME
    gettimeofday(&finish_time, NULL);

    //LOAD output
    why2_output_flags output =
    {
        returning_text, //ENCRYPTED TEXT
        key, //GENERATED/USED KEY
        why2_count_unused_key_size(text, key), // NUMBER OF WHY2_UNUSED CHARS IN KEY
        why2_count_repeated_key_size(text, key), //NUMBER OF REPEATED CHARS IN KEY
        why2_compare_time_micro(start_time, finish_time), // ELAPSED TIME
        WHY2_SUCCESS //EXIT CODE
    };

    //DEALLOCATION
    why2_deallocate(text_key_chain);
    why2_deallocate(text_buffer);

    why2_reset_memory_identifier();

    return output;
}
