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

int encryptionOperationTest(int text, int encryptedText)
{
    return text ^ encryptedText;
}

int main(void)
{
    //VARIABLES
    char *textBuffer;
    char *keyBuffer;
    char *statusBuffer;
    char *outputBuffer = why2_malloc(1); //THIS IS TEMP ALLOCATION
    int exitCode = 0;
    unsigned long timeBuffer;
    FILE *outputStreamBuffer = stdout;

    //FLAGS
    why2_input_flags flags =
    {
        0, //SKIP CHECK
        0, //NO OUTPUT
        0 //UPDATE
    };

    //SET FLAGS
    why2_set_flags(flags);

    //SET KEY_LENGTH TO 100
    why2_set_key_length(100);

    //SET ENCRYPTION_SEPARATOR TO '|'
    why2_set_encryption_separator('|');

    //SET outputBuffer to NULL
    outputBuffer[0] = '\0';

    //SET encryptionOperation to encryptionOperationTest
    why2_set_encryption_operation(encryptionOperationTest);

    //ENCRYPT
    why2_output_flags encrypted = why2_encrypt_text(WHY2_TEST_TEXT, NULL);

    textBuffer = strdup(encrypted.outputText); //GET ENCRYPTED TEXT
    keyBuffer = strdup(encrypted.usedKey); //GET KEY
    timeBuffer = encrypted.elapsedTime; //GET TIME 1

    //DEALLOCATE BUFFER
    why2_deallocate_output(encrypted);

    //DECRYPT
    encrypted = why2_decrypt_text(textBuffer, keyBuffer);

    timeBuffer += encrypted.elapsedTime; //GET TIME 1

    //COMPARE DIFFERENCE
    if (strcmp(encrypted.outputText, WHY2_TEST_TEXT) == 0 && encrypted.exitCode == 0) //WHY2_SUCCESS
    {
        statusBuffer = strdup("successful");
    }
    else //FAILURE
    {
        statusBuffer = strdup("failed");

        outputBuffer = realloc(outputBuffer, strlen(encrypted.outputText) + 6);
        sprintf(outputBuffer, "\t\t\"%s\"\n", encrypted.outputText);

        outputStreamBuffer = stderr;
        exitCode = 1;
    }

    //PRINT OUTPUT
    fprintf
    (
        outputStreamBuffer,

        "Test %s!\n\n"

        "TEXT: \t\t\"%s\"\n%s"
        "OUTPUT: \t\"%s\"\n"
        "KEY: \t\t\"%s\"\n"
        "TIME: \t\t\"%lums\"\n"
        "UNUSED KEY: \t\"%lu\"\n"
        "REPEATED KEY: \t\"%lu\"\n"
        "EXIT CODE: \t\"%d\"\n"

        , statusBuffer, WHY2_TEST_TEXT, outputBuffer, textBuffer, encrypted.usedKey, timeBuffer / 1000, encrypted.unusedKeySize, encrypted.repeatedKeySize, encrypted.exitCode
    );

    //DEALLOCATION
    why2_free(textBuffer);
    why2_free(keyBuffer);
    why2_free(statusBuffer);
    why2_free(outputBuffer);
    why2_deallocate_output(encrypted);

    return exitCode;
}
