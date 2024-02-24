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

why2_output_flags why2_decrypt_text(char *text, char *key_new)
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

    //CHECK FOR INVALID key
    if ((check_exit_code = why2_check_key(key_new)) != WHY2_SUCCESS)
    {
        return why2_no_output(check_exit_code);
    }

    //REDEFINE keyLength
    why2_set_key_length(strlen(key_new));

    //VARIABLES
    char *returning_text;
    int number_buffer = 0;
    int used_text_deallocation_buffer = 0;
    char *text_buffer = NULL;
    int text_key_chainLength;
    int *text_key_chain;
    char *key = why2_strdup(key_new); //COPY key_new TO key
    int *encrypted_text_key_chain;
    char *used_text = NULL; //COPY text TO used_text

    if (why2_get_flags().format == WHY2_OUTPUT_BYTE)
    {
        //REBUILD THE BYTE FORMAT AS TEXT FORMAT
        int *encrypted_input = why2_malloc(sizeof(int) * why2_byte_format_length(text));
        char *text_copy = why2_strdup(text);

        //GET ENCRYPTED NUMBERS
        for (unsigned short i = 0; i < why2_byte_format_length(text_copy); i++)
        {
            for (unsigned short j = 2 + (i * 2); j <= 3 + (i * 2); j++)
            {
                //ENSURE THERE IS NO \0 (REVERSED)
                if (text_copy[j] > 0)
                {
                    text_copy[j]--;
                } else
                {
                    text_copy[j]++;
                }
            }

            //PUT TOGETHER
            encrypted_input[i] = (text_copy[3 + (i * 2)] << 7) | text_copy[2 + (i * 2)];

            //ALSO COUNT REQUIRED SIZE
            number_buffer += why2_count_int_length(encrypted_input[i]);
        }
        number_buffer += why2_byte_format_length(text_copy); //ADD THE SEPARATORS (I didn't remove one cause 1 index will be empty at used_text end)

        used_text = why2_calloc(number_buffer, sizeof(char));

        for (unsigned short i = 0; i < why2_byte_format_length(text_copy); i++)
        {
            number_buffer = why2_count_int_length(encrypted_input[i]);
            text_buffer = why2_realloc(text_buffer, number_buffer + 1);

            sprintf(text_buffer, "%d", encrypted_input[i]);
            strcat(used_text, text_buffer);

            if (i != why2_byte_format_length(text_copy) - 1)
            {
                used_text[strlen(used_text)] = why2_get_encryption_separator();
            }
        }

        //DEALLOCATION
        why2_deallocate(encrypted_input);
        why2_deallocate(text_buffer);
        why2_deallocate(text_copy);
    } else if (why2_get_flags().format == WHY2_OUTPUT_TEXT)
    {
        used_text = why2_strdup(text);
    }

    number_buffer = 1;

    //GET LENGTH OF returning_text AND text_key_chain
    for (int i = 0; i < (int) strlen(used_text); i++)
    {
        if (used_text[i] == why2_get_encryption_separator()) number_buffer++;
    }

    //SET LENGTH (number_buffer)
    returning_text = why2_calloc(number_buffer + 1, sizeof(char));
    text_key_chain = why2_malloc(sizeof(int) * number_buffer);
    encrypted_text_key_chain = why2_malloc(sizeof(int) * number_buffer);
    text_key_chainLength = number_buffer;

    //LOAD text_key_chain
    why2_generate_text_key_chain(key, text_key_chain, number_buffer);

    //LOAD encrypted_text_key_chain
    for (int i = 0; i < text_key_chainLength; i++)
    {
        number_buffer = 0;

        //GET LENGTH OF EACH CHARACTER
        for (int j = 0; j < (int) strlen(used_text); j++)
        {
            if (used_text[j] == why2_get_encryption_separator()) break;

            number_buffer++;
        }

        text_buffer = why2_realloc(text_buffer, number_buffer + 1);

        //CLEAN (POSSIBLY EXISTING) JUNK in text_buffer
        for (int j = 0; j <= number_buffer; j++)
        {
            text_buffer[j] = '\0';
        }

        //LOAD text_buffer
        for (int j = 0; j < (int) strlen(used_text); j++)
        {
            text_buffer[j] = used_text[j];

            if (number_buffer == j) break;
        }

        encrypted_text_key_chain[i] = atoi(text_buffer);

        used_text += number_buffer + 1;
        used_text_deallocation_buffer += number_buffer + 1;
    }

    //DECRYPT TEXT
    for (int i = 0; i < text_key_chainLength; i++)
    {
        text_key_chain[i] = why2_get_encryption_operation()(text_key_chain[i], encrypted_text_key_chain[i]);
    }

    //LOAD returning_text
    for (int i = 0; i < text_key_chainLength; i++)
    {
        returning_text[i] = text_key_chain[i];
    }

    //GET FINISH TIME
    gettimeofday(&finish_time, NULL);

    //LOAD output
    why2_output_flags output =
    {
        returning_text, //DECRYPTED TEXT
        key, //USED KEY
        why2_count_unused_key_size(returning_text, key), // NUMBER OF WHY2_UNUSED CHARS IN KEY
        why2_count_repeated_key_size(returning_text, key), //NUMBER OF REPEATED CHARS IN KEY
        why2_compare_time_micro(start_time, finish_time), // ELAPSED TIME
        WHY2_SUCCESS //EXIT CODE
    };

    //DEALLOCATION
    why2_deallocate(text_key_chain);
    why2_deallocate(encrypted_text_key_chain);
    why2_deallocate(text_buffer);
    why2_deallocate(used_text - used_text_deallocation_buffer);

    return output;
}
