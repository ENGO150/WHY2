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

int main(void)
{
    //VARIABLES
    logFile logger = initLogger(WRITE_DIR); //INITIALIZE LOGGER FILE
    int bufferSize;
    char *buffer;
    FILE *fileBuffer;

    writeLog(logger.file, WRITE_MESSAGE); //WRITE

    fileBuffer = fopen(logger.fileName, "r");
    fseek(fileBuffer, 0, SEEK_END);
    bufferSize = ftell(fileBuffer);
    rewind(fileBuffer); //REWIND fileBuffer (NO SHIT)

    //SET LENGTH OF buffer
    buffer = malloc(bufferSize + 1);

    //LOAD jsonFile
    if (fread(buffer, bufferSize, 1, fileBuffer) == 0) abort(); //TODO: Make it safe

    printf("%s\n", buffer);

    //DEALLOCATION
    free(buffer);
    fclose(fileBuffer);
    deallocateLogger(logger);
    return 0;
}