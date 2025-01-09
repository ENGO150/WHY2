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
#include <limits.h>

#include <why2/crypto.h>
#include <why2/flags.h>
#include <why2/llist.h>
#include <why2/memory.h>
#include <why2/misc.h>

why2_output_flags why2_encrypt_text(char *text, char *key)
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
    char *key_new = NULL;
    char *text_new = NULL;
    char *returning_text = NULL;
    char *text_buffer = NULL;
    int *text_key_chain;
    int number_buffer = 0;

    if (key != NULL)
    {
        //CHECK FOR INVALID key_new
        if ((check_exit_code = why2_check_key(key)) != WHY2_SUCCESS)
        {
            why2_clean_memory("core_decrypt");
            return why2_no_output(check_exit_code);
        }

        key_new = why2_strdup(key);

        //REDEFINE keyLength
        why2_set_key_length(strlen(key_new));
    } else //LOAD KEY
    {
        key_new = why2_generate_key(why2_get_key_length());
    }

    //PADDING
    if (why2_get_flags().padding > 0)
    {
        why2_list_t split_text = WHY2_LIST_EMPTY; //LIST OF text SPLIT INTO CHARS

        //ADD CHARS
        for (unsigned long i = 0; i < strlen(text); i++)
        {
            why2_list_push(&split_text, &(text[i]), sizeof(text[i])); //ADD
        }

        //OBTAIN SEED FROM key_new
        srand(why2_sum_segment(key_new));

        //ADD PADDING TO split_text LIST
        for (unsigned long i = 0; i < why2_get_flags().padding; i++)
        {
            unsigned long random_position = (unsigned long) (rand() % (why2_list_get_size(&split_text))); //GET RANDOM POSITION

            char random_char = 0;
            for (int j = 0; j < WHY2_PADDING_NONZERO_TRIES && random_char == 0 ; j++) //GET RANDOM (EXCLUDING 0)
            {
                why2_random(&random_char, sizeof(random_char)); //GENERATE COMPLETELY RANDOM CHARACTER
            }
            if (random_char == 0) random_char = 1;

            //INSERT RANDOM VALUE
            why2_list_push_at(&split_text, random_position, &random_char, sizeof(random_char));
        }

        //PUT PADDED TEXT INTO text_new
        text_new = why2_calloc(why2_list_get_size(&split_text) + 1, sizeof(char));
        why2_node_t *buffer = split_text.head;
        why2_node_t *buffer_2;
        unsigned long index_buffer = 0;

        do
        {
            text_new[index_buffer++] = *(char*) buffer -> value; //COPY VALUE

            buffer_2 = buffer;
            buffer = buffer -> next; //ITER

            why2_deallocate(buffer_2 -> value); //DEALLOCATION
            why2_deallocate(buffer_2); //DEALLOCATION
        } while (buffer != NULL);
    } else text_new = why2_strdup(text); //USE TEXT WITHOUT PADDING

    text_key_chain = why2_malloc(sizeof(int) * strlen(text_new));

    //LOAD text_key_chain
    why2_generate_text_key_chain(key_new, text_key_chain, strlen(text_new));

    //ACTUALLY ENCRYPT TEXT
    for (int i = 0; i < (int) strlen(text_new); i++)
    {
        text_key_chain[i] = why2_get_encryption_operation()(text_key_chain[i], (int) text_new[i]);
    }

    //OUTPUT FORMATS
    if (why2_get_flags().format == WHY2_OUTPUT_TEXT) //NORMAL 420.-69 FORMAT
    {
        //COUNT REQUIRED SIZE FOR returning_text
        for (int i = 0; i < (int) strlen(text_new); i++)
        {
            number_buffer += why2_count_int_length(text_key_chain[i]);
        }

        //ALLOCATE returning_text (WITH THE SEPARATORS)
        returning_text = why2_calloc(number_buffer + strlen(text_new), sizeof(char));

        //LOAD returning_text
        for (int i = 0; i < (int) strlen(text_new); i++)
        {
            number_buffer = sizeof(int) * why2_count_int_length(text_key_chain[i]);

            text_buffer = why2_realloc(text_buffer, number_buffer);

            sprintf(text_buffer, "%d", text_key_chain[i]);

            strcat(returning_text, text_buffer);

            if (i != (int) strlen(text_new) - 1)
            {
                returning_text[strlen(returning_text)] = why2_get_encryption_separator();
            }
        }
    } else if (why2_get_flags().format == WHY2_OUTPUT_BYTE) //FUCKED BUT SHORT(ER) OUTPUT
    {
        number_buffer = (strlen(text_new) + 1) * 2; //EACH CHARACTER WILL BE SPLIT INTO TWO CHARS AND FIRST TWO WILL BE LENGTH OF text_new

        returning_text = why2_calloc(number_buffer + 1, sizeof(char)); //ALLOCATE

        //SET LENGTH
        returning_text[0] = (strlen(text_new) & 0x7f) + 1; //+1 BECAUSE WE DON'T WANT \0
        returning_text[1] = (strlen(text_new) >> 7) + 1;

        //PUT THE text_key_chain INTO returning_text DIRECTLY
        for (unsigned long i = 0; i < strlen(text_new); i++)
        {
            //BUILD returning_text
            returning_text[2 + (i * 2)] = text_key_chain[i] & 0x7f;
            returning_text[3 + (i * 2)] = (text_key_chain[i] >> 7) | ((text_key_chain[i] < 0) ? 0x80 : 0);

            for (unsigned long j = 2 + (i * 2); j <= 3 + (i * 2); j++)
            {
                //ENSURE THERE IS NO \0
                if (returning_text[j] == 0) returning_text[j] = -128;
            }
        }
    }

    //GET FINISH TIME
    gettimeofday(&finish_time, NULL);

    //LOAD output
    why2_output_flags output =
    {
        returning_text, //ENCRYPTED TEXT
        key_new, //GENERATED/USED KEY
        why2_count_unused_key_size(text_new, key_new), // NUMBER OF WHY2_UNUSED CHARS IN KEY
        why2_count_repeated_key_size(text_new, key_new), //NUMBER OF REPEATED CHARS IN KEY
        why2_compare_time_micro(start_time, finish_time), // ELAPSED TIME
        WHY2_SUCCESS //EXIT CODE
    };

    //DEALLOCATION
    why2_deallocate(text_key_chain);
    why2_deallocate(text_buffer);
    why2_deallocate(text_new);

    why2_reset_memory_identifier();

    return output;
}
