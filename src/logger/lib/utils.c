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

#include <why2/logger/flags.h>

void deallocateLogger(logFile logger)
{
    close(logger.file);
    free(logger.fileName);
}

void removeSpaces(char* string)
{
    char* d = string;
    do
    {
        while (*d == ' ')
        {
            ++d;
        }
    } while ((*string++ = *d++));
}

void decryptLogger(logFile logger) //TODO: Fix valgrind issues
{

    FILE *file = fdopen(logger.file, "r"); //OPEN logFile AS FILE POINTER
    char *rawContent;
    char **linesContent;
    int rawContentLength;
    int lines = 0;
    int buffer = 0;
    int buffer2;
    int buffer3 = 0;
    //int buffer4 = 0;

    fseek(file, 0, SEEK_END);
    rawContentLength = ftell(file);
    rewind(file); //REWIND FILE

    //ALLOCATE rawContent
    rawContent = malloc(rawContentLength + 1);

    memset(rawContent, '\0', rawContentLength + 1);

    (void) (fread(rawContent, rawContentLength, 1, file) + 1); //TODO: Try to create some function for processing exit value

    //GET lines
    for (int i = 0; i < rawContentLength; i++)
    {
        if (rawContent[i] == '\n') lines++;
    }

    linesContent = malloc(lines + 1);

    for (int i = 0; i < rawContentLength; i++) //LOAD/SPIT rawContent INTO linesContent
    {
        if (rawContent[i] == '\n')
        {
            buffer2 = i - buffer;
            if (buffer != 0) buffer2--;

            //printf("%d\t%d\t%d\n", i, buffer, buffer2);
            //printf("%d\t%d\n", buffer3, buffer2);
            linesContent[buffer3] = malloc((buffer2 + 1) - strlen(WRITE_FORMAT));
            //memset(linesContent[buffer3], '\0', buffer2 + 1);
            // for (int j = 0; j <= buffer2; j++)
            // {
            //     linesContent[buffer3][j] = '\0';
            // }

            for (int j = buffer + strlen(WRITE_FORMAT); j < i; j++)
            {
                linesContent[buffer3][j - (buffer + strlen(WRITE_FORMAT))/* - buffer4*/] = rawContent[j];
                //printf("%d ", j - buffer);
                //printf("%c", rawContent[j]);
            }
            linesContent[buffer3][(buffer2 + 1) - strlen(WRITE_FORMAT)] = '\0';
            removeSpaces(linesContent[buffer3]);
            //printf("\n");

            buffer = i;
            buffer3++;
            //buffer4 = 1; | e
        }
    }

    //TODO: Decrypt


    for (int i = 0; i < buffer3; i++)
    {
        free(linesContent[i]);
    }
    free(linesContent);

    free(rawContent);
    return;
}