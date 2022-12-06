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

#include <why2/logger/logger.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <dirent.h>
#include <time.h>
#include <sys/stat.h>

#include <why2/logger/flags.h>

logFile initLogger(char *directoryPath)
{
    //VARIABLES
    struct stat st;
    struct dirent *entry;
    time_t timeL = time(NULL);
    struct tm tm = *localtime(&timeL);
    int buffer = 1;
    int file;
    char *filePath = malloc(strlen(directoryPath) + 1 + strlen(LOG_FORMAT) + 1);
    char *dateBuffer = malloc(strlen(LOG_FORMAT_START) + 1);
    DIR *dir;

    sprintf(dateBuffer, LOG_FORMATTING_START, tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday);

    //CREATE directoryPath DIRECTORY IF IT DOESN'T EXISTS ALREADY
    if (stat(directoryPath, &st) == -1)
    {
        mkdir(directoryPath, 0700);
    }

    //COUNT HOW MANY LOGS WERE GENERATED THIS DAY
    dir = opendir(directoryPath);
    while ((entry = readdir(dir)) != NULL)
    {
        if (entry -> d_type == DT_REG)
        {
            if (strncmp(dateBuffer, entry -> d_name, strlen(dateBuffer)) == 0) buffer++;
        }
    }

    if (buffer > MAX_USAGE) //MAX_USAGE WAS REACHED
    {
        file = INVALID_FILE;

        goto deallocation;
    }

    sprintf(filePath, LOG_FORMATTING, directoryPath, tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, buffer); //GENERATE LOG-NAME

    file = open(filePath, O_WRONLY | O_APPEND | O_CREAT, 0644);

    deallocation:

    //DEALLOCATION
    free(dateBuffer);
    closedir(dir);

    return (logFile)
    {
        file,
        filePath
    };
}

void writeLog(int loggerFile, char *logMessage)
{
    //CHECK IF loggerFile IS CREATED
    if (loggerFile == INVALID_FILE)
    {
        return; //TODO: Add some kind of error message
    }

    //VARIABLES
    char *buffer = malloc(strlen(WRITE_FORMAT) + strlen(logMessage) + 2);
    time_t timeL = time(NULL);
    struct tm tm = *localtime(&timeL);

    sprintf(buffer, WRITE_FORMATTING, tm.tm_hour, tm.tm_min, tm.tm_sec, logMessage); //LOAD MESSAGE

    write(loggerFile, buffer, strlen(buffer)); //WRITE (YAY)

    //DEALLOCATION
    free(buffer);
}