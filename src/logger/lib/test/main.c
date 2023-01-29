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

#include <why2.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(void)
{
    //VARIABLES
    why2_log_file logger = why2_init_logger(WHY2_WRITE_DIR); //INITIALIZE LOGGER FILE
    char *usedKey = why2_malloc(why2_get_key_length() + 1);
    why2_decrypted_output decrypted;
    int exitCode = 0;

    //GENERATE KEY
    why2_generate_key(usedKey, why2_get_key_length());

    //FLAGS
    why2_log_flags flags =
    {
        usedKey
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
        strcmp(decrypted.decryptedText[0], WHY2_WRITE_MESSAGE_1) == 0 &&
        strcmp(decrypted.decryptedText[1], WHY2_WRITE_MESSAGE_2) == 0 &&
        strcmp(decrypted.decryptedText[2], WHY2_WRITE_MESSAGE_3) == 0
    )
    {
        printf
        (
            "TEST SUCCESSFUL!\n\n"

            "TEXT:\t\"%s\"\n"
                 "\t\"%s\"\n"
                 "\t\"%s\"\n",

            decrypted.decryptedText[0], decrypted.decryptedText[1], decrypted.decryptedText[2]
        );
    } else //FAILED
    {
        fprintf(stderr, "TEST FAILED!\n");
        exitCode = 1;
    }

    //DEALLOCATION
    why2_free(usedKey);
    why2_deallocate_logger(logger);
    why2_deallocate_decrypted_output(decrypted);

    return exitCode;
}