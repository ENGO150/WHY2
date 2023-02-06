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
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <dirent.h>
#include <time.h>
#include <sys/stat.h>

#include <why2/logger/flags.h>
#include <why2/logger/utils.h>

#include <why2/encrypter.h>
#include <why2/memory.h>
#include <why2/misc.h>

why2_log_file why2_init_logger(char *directoryPath)
{
    why2_set_memory_identifier("logger_logfile_init");

    //VARIABLES
    struct stat st;
    struct dirent *entry;
    time_t timeL = time(NULL);
    struct tm tm = *localtime(&timeL);
    int buffer = 1;
    int file;
    char *filePath = why2_malloc(strlen(directoryPath) + 1 + strlen(WHY2_LOG_FORMAT) + 1);
    char *dateBuffer = why2_malloc(strlen(WHY2_LOG_FORMAT_START) + 1);
    char *latestBuffer = why2_malloc(strlen(WHY2_WRITE_DIR) + strlen(WHY2_LOG_LATEST) + 2);
    char *latestFilePath = NULL;
    DIR *dir;

    sprintf(dateBuffer, WHY2_LOG_FORMATTING_START, tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday);

    //CREATE directoryPath DIRECTORY IF IT DOESN'T EXISTS ALREADY
    if (stat(directoryPath, &st) == -1)
    {
        mkdir(directoryPath, 0700);
    }

    //COUNT HOW MANY LOGS WERE GENERATED THIS DAY
    dir = why2_opendir(directoryPath);
    while ((entry = readdir(dir)) != NULL)
    {
        if (entry -> d_type == DT_REG)
        {
            if (strncmp(dateBuffer, entry -> d_name, strlen(dateBuffer)) == 0) buffer++;
        }
    }

    if (buffer > WHY2_MAX_USAGE) //WHY2_MAX_USAGE WAS REACHED
    {
        file = INVALID_FILE;

        goto deallocation;
    }

    sprintf(filePath, WHY2_LOG_FORMATTING, directoryPath, dateBuffer, buffer); //GENERATE LOG-NAME

    file = open(filePath, O_RDWR | O_APPEND | O_CREAT, 0644); //CREATE LOG FILE

    //CREATE SYMLINK
    sprintf(latestBuffer, WHY2_LOG_LATEST_FORMATTING, WHY2_WRITE_DIR, WHY2_LOG_LATEST); //GENERATE LATEST.log PATH

    latestFilePath = why2_strdup(filePath);

    if (access(latestBuffer, R_OK) == 0) { unlink(latestBuffer); } //REMOVE SYMLINK IF IT ALREADY EXISTS

    if (symlink(latestFilePath + (strlen(WHY2_WRITE_DIR) + 1), latestBuffer) != 0) //CREATE SYMLINK
    {
        if (!why2_get_flags().no_output) fprintf(stderr, "Creating symlink failed!\n");

        close(file);
        why2_clean_memory("logger_logfile_init");
        return why2_empty_log_file();
    }

    deallocation:

    //DEALLOCATION
    why2_deallocate(dateBuffer);
    why2_deallocate(latestBuffer);
    why2_deallocate(latestFilePath);
    why2_deallocate(dir);

    why2_reset_memory_identifier();

    return (why2_log_file)
    {
        file,
        filePath
    };
}

void why2_write_log(int loggerFile, char *logMessage)
{
    //CHECK IF loggerFile IS CREATED
    if (loggerFile == INVALID_FILE)
    {
        return; //TODO: Add some kind of error message
    }

    //COPY logMessage without '\n'
    char *logMessageUsed = why2_strdup(logMessage);
    for (int i = 0; i < (int) strlen(logMessageUsed); i++)
    {
        if (logMessageUsed[i] == '\n') logMessageUsed[i] = '\0';
    }

    //VARIABLES
    char *buffer = NULL;
    char *message = NULL; //FINAL MESSAGE
    time_t timeL = time(NULL);
    struct tm tm = *localtime(&timeL);
    why2_log_flags flags = why2_get_log_flags();

    //SET ENCRYPTER FLAGS
    if (!why2_get_flags_changed()) why2_set_flags((why2_input_flags) { 0, 1, 0 });

    if (flags.key != NULL) //ENCRYPT TEXT IF KEY WAS CHANGED
    {
        why2_output_flags encrypted = why2_encrypt_text(logMessageUsed, flags.key); //ENCRYPT

        message = why2_strdup(encrypted.output_text); //COPY

        //DEALLOCATION
        why2_deallocate_output(encrypted);
        why2_deallocate(logMessageUsed); //I COULD DO THIS SMART SOMEHOW, BUT I AM TOO LAZY FOR THAT SHIT
    } else //FUCK ENCRYPTION, LET'S DO IT; WHY WOULD WE EVEN USE WHY2-CORE? HUH?
    {
        message = logMessageUsed;
    }

    buffer = why2_malloc(strlen(WHY2_WRITE_FORMAT) + strlen(message) + 2); //ALLOCATE

    sprintf(buffer, WHY2_WRITE_FORMATTING, tm.tm_hour, tm.tm_min, tm.tm_sec, message); //LOAD MESSAGE

    if (write(loggerFile, buffer, strlen(buffer)) != (ssize_t) strlen(buffer))
    {
        if (!why2_get_flags().no_output) fprintf(stderr, "Writing failed!");
    }

    //DEALLOCATION
    why2_deallocate(buffer);
    why2_deallocate(message);
}