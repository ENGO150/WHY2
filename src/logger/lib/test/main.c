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

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <why2.h>

int main(void)
{
    //VARIABLES
    why2_log_file logger = why2_init_logger(WHY2_WRITE_DIR); //INITIALIZE LOGGER FILE
    char *used_key = why2_generate_key(why2_get_key_length()); //GENERATE KEY
    why2_decrypted_output decrypted;
    int exit_code = 0;

    //FLAGS
    why2_log_flags flags =
    {
        used_key
    };

    //SET FLAGS
    why2_set_log_flags(flags);

    //WRITE
    why2_write_log(logger.file, WHY2_WRITE_MESSAGE_1);
    why2_write_log(logger.file, WHY2_WRITE_MESSAGE_2);
    why2_write_log(logger.file, WHY2_WRITE_MESSAGE_3);

    decrypted = why2_decrypt_logger(logger); //DECRYPT

    //COMPARE OUTPUT
    if //WHY2_SUCCESS //TODO: Make this smart somehow
    (
        strcmp(decrypted.decrypted_text[0], WHY2_WRITE_MESSAGE_1) == 0 &&
        strcmp(decrypted.decrypted_text[1], WHY2_WRITE_MESSAGE_2) == 0 &&
        strcmp(decrypted.decrypted_text[2], WHY2_WRITE_MESSAGE_3) == 0
    )
    {
        printf
        (
            "TEST SUCCESSFUL!\n\n"

            "TEXT:\t\"%s\"\n"
                 "\t\"%s\"\n"
                 "\t\"%s\"\n",

            decrypted.decrypted_text[0], decrypted.decrypted_text[1], decrypted.decrypted_text[2]
        );
    } else //FAILED
    {
        fprintf(stderr, "TEST FAILED!\n");
        exit_code = 1;
    }

    //DEALLOCATION
    why2_deallocate(used_key);
    why2_deallocate_logger(logger);
    why2_deallocate_decrypted_output(decrypted);

    return exit_code;
}