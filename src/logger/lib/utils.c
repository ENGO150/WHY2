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

#include <why2/logger/utils.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include <why2/decrypter.h>
#include <why2/flags.h>
#include <why2/misc.h>

#include <why2/logger/flags.h>

void deallocateLogger(logFile logger)
{
    close(logger.file);
    free(logger.fileName);
}

void deallocateDecryptedOutput(decryptedOutput output)
{
    for (int i = 0; i < output.length; i++)
    {
        free(output.decryptedText[i]);
    }

    output.length = 0;
    free(output.decryptedText);
}

decryptedOutput decryptLogger(logFile logger) //TODO: Fix valgrind issues
{
    FILE *file = fdopen(logger.file, "r");
    outputFlags outputBuffer;
    char *rawContent;
    char **content;
    char **contentDecrypted;
    int rawContentL;
    int lines = 0;
    int startingAtBuffer = 0;
    int contentIndexBuffer = 0;

    //COUNT LENGTH OF buffer AND STORE IT IN bufferSize
    fseek(file, 0, SEEK_END);
    rawContentL = ftell(file);
    rewind(file); //REWIND file (NO SHIT)

    //ALLOCATE rawContent
    rawContent = calloc(rawContentL + 1, sizeof(char)); //CALLOC WILL BE USED FOR CLEANING AFTER ALLOCATION

    //LOAD rawContent
    (void) (fread(rawContent, rawContentL, 1, file) + 1); //TODO: Try to create some function for processing exit value

    for (int i = 0; i < rawContentL; i++)
    {
        if (rawContent[i] == '\n') lines++;
    }

    //ALLOCATE content & contentDecrypted
    content = calloc(lines + 1, sizeof(char));
    contentDecrypted = calloc(lines + 1, sizeof(char));

    for (int i = 0; i < rawContentL; i++) //LOAD rawContent AND SPLIT TO content
    {
        if (rawContent[i] == '\n') //END OF ONE LINE
        {
            content[contentIndexBuffer] = calloc((i - (startingAtBuffer + strlen(WRITE_FORMAT)) + 1), sizeof(char)); //ALLOCATE ELEMENT

            for (int j = startingAtBuffer + strlen(WRITE_FORMAT); j <= i; j++) //LOAD CONTENT OF EACH LINE
            {
                content[contentIndexBuffer][j - (startingAtBuffer + strlen(WRITE_FORMAT))] = rawContent[j]; //SET
            }

            contentIndexBuffer++;
            startingAtBuffer = i + 1;
        }
    }

    for (int i = 0; i < lines; i++) //DECRYPT content
    {
        outputBuffer = decryptText(content[i], getLogFlags().key); //DECRYPT

        contentDecrypted[i] = strdup(outputBuffer.outputText); //COPY

        deallocateOutput(outputBuffer); //DEALLOCATE outputBuffer
    }

    //DEALLOCATION
    free(rawContent);
    fclose(file);
    deallocateDoublePointer(content);

    return (decryptedOutput)
    {
        contentDecrypted,
        lines
    };
}