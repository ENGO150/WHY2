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
#include <string.h>
#include <unistd.h>

#include <why2/decrypter.h>
#include <why2/flags.h>
#include <why2/memory.h>
#include <why2/misc.h>

#include <why2/logger/flags.h>

void why2_deallocate_logger(why2_log_file logger)
{
    close(logger.file);
    why2_free(logger.fileName);

    logger.fileName = NULL;
    logger.file = INVALID_FILE;
}

void why2_deallocate_decrypted_output(why2_decrypted_output output)
{
    for (unsigned long i = 0; i < output.length; i++)
    {
        why2_free(output.decryptedText[i]);
    }

    output.length = 0;
    why2_free(output.decryptedText);
}

why2_decrypted_output why2_decrypt_logger(why2_log_file logger)
{
    FILE *file = fdopen(logger.file, "r");
    why2_output_flags outputBuffer;
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
    rawContent = why2_calloc(rawContentL + 1, sizeof(char)); //CALLOC WILL BE USED FOR CLEANING AFTER ALLOCATION

    //LOAD rawContent
    (void) (fread(rawContent, rawContentL, 1, file) + 1); //TODO: Try to create some function for processing exit value

    for (int i = 0; i < rawContentL; i++)
    {
        if (rawContent[i] == '\n') lines++;
    }

    //ALLOCATE content & contentDecrypted
    content = why2_calloc(lines + 1, sizeof(char*));
    contentDecrypted = why2_calloc(lines + 1, sizeof(char*));

    for (int i = 0; i < rawContentL; i++) //LOAD rawContent AND SPLIT TO content
    {
        if (rawContent[i] == '\n') //END OF ONE LINE
        {
            content[contentIndexBuffer] = why2_calloc((i - (startingAtBuffer + strlen(WHY2_WRITE_FORMAT)) + 2), sizeof(char)); //ALLOCATE ELEMENT

            for (int j = startingAtBuffer + strlen(WHY2_WRITE_FORMAT); j <= i; j++) //LOAD CONTENT OF EACH LINE
            {
                content[contentIndexBuffer][j - (startingAtBuffer + strlen(WHY2_WRITE_FORMAT))] = rawContent[j]; //SET
            }

            contentIndexBuffer++;
            startingAtBuffer = i + 1;
        }
    }

    for (int i = 0; i < lines; i++) //DECRYPT content
    {
        outputBuffer = why2_decrypt_text(content[i], why2_get_log_flags().key); //DECRYPT

        contentDecrypted[i] = strdup(outputBuffer.outputText); //COPY

        why2_deallocate_output(outputBuffer); //DEALLOCATE outputBuffer
    }

    //DEALLOCATION
    why2_free(rawContent);
    fclose(file);
    why2_deallocate_decrypted_output((why2_decrypted_output) { content, lines }); //fuck the system lmao

    return (why2_decrypted_output)
    {
        contentDecrypted,
        lines
    };
}