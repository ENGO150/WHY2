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
    logFile logger = initLogger(WRITE_DIR); //INITIALIZE LOGGER FILE
    char *usedKey = malloc(getKeyLength() + 1);
    decryptedOutput decrypted;
    int exitCode = 0;

    //GENERATE KEY
    generateKey(usedKey, getKeyLength());

    //FLAGS
    logFlags flags =
    {
        usedKey
    };

    //SET FLAGS
    setLogFlags(flags);

    //WRITE
    writeLog(logger.file, WRITE_MESSAGE_1);
    writeLog(logger.file, WRITE_MESSAGE_2);
    writeLog(logger.file, WRITE_MESSAGE_3);

    decrypted = decryptLogger(logger); //DECRYPT

    //COMPARE OUTPUT
    if //SUCCESS //TODO: Make this smart somehow
    (
        strcmp(decrypted.decryptedText[0], WRITE_MESSAGE_1) == 0 &&
        strcmp(decrypted.decryptedText[1], WRITE_MESSAGE_2) == 0 &&
        strcmp(decrypted.decryptedText[2], WRITE_MESSAGE_3) == 0
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
    free(usedKey);
    deallocateLogger(logger);
    deallocateDecryptedOutput(decrypted);
    return exitCode;
}