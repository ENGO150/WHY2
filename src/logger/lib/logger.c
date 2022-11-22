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

int initLogger(char *directoryPath)
{
    //VARIABLES
    struct stat st;
    struct dirent *entry;
    time_t timeL = time(NULL);
    struct tm tm = *localtime(&timeL);
    int buffer = 1;
    int returning;
    char *filePath = malloc(strlen(directoryPath) + strlen(LOG_FORMAT) + 1);
    DIR *dir;

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
            if (strcmp(entry -> d_name, ".gitkeep") == 0) continue;

            buffer++;
        }
    }

    if (buffer > MAX_USAGE) return INVALID_FILE; //MAX_USAGE WAS REACHED

    sprintf(filePath, LOG_FORMATTING, directoryPath, tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, buffer); //GENERATE LOG-NAME

    returning = open(filePath, O_WRONLY | O_APPEND | O_CREAT, 0644);

    //DEALLOCATION
    free(filePath);
    closedir(dir);

    return returning;
}

void writeLog(int loggerFile, char *logMessage)
{
    //VARIABLES
    char *buffer = malloc(strlen(WRITE_FORMAT) + strlen(logMessage) + 2);
    time_t timeL = time(NULL);
    struct tm tm = *localtime(&timeL);

    sprintf(buffer, WRITE_FORMATTING, tm.tm_hour, tm.tm_min, tm.tm_sec, logMessage); //LOAD MESSAGE

    write(loggerFile, buffer, strlen(buffer)); //WRITE (YAY)

    //DEALLOCATION
    free(buffer);
}