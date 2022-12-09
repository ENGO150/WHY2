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

#ifndef WHY2_LOGGER_FLAGS_H
#define WHY2_LOGGER_FLAGS_H

//CONSTS
const enum RETURN_CODES //exit codes you fucking idiot (2#)
{
    INVALID_FILE = -1 //THIS WILL HAPPEN IF YOU USE TOO MUCH LOGS lol
};

#define WRITE_MESSAGE "Hello from logger-test! 👋"
#define WRITE_DIR "./logs"
#define LOG_LATEST "latest.log"

#define LOG_FORMAT "yyyy-mm-dd_xxx.log" //THE LAST xxx IS FOR BASE-16 NUMBER OF USAGE THAT DAY (SO MAX IS 4095 [FFF] USAGES PER DAY)
#define LOG_FORMAT_START "yyyy-mm-dd" //FIRST PART OF LOG_FORMAT

#define LOG_FORMATTING "%s/%s_%03x.log" //SAME THING AS LOG_FORMAT, BUT USED IN sprintf IN logger.c, NOT AS LENGTH
#define LOG_FORMATTING_START "%04d-%02d-%02d" //FIRST PART OF LOG_FORMATTING

#define WRITE_FORMAT "[hh:mm:ss] " //LOG MESSAGE'S PREFIX (THE SPACE AT THE END IS INTENTIONAL)
#define WRITE_FORMATTING "[%02d:%02d:%02d]: %s" //guess what

#define LOG_LATEST_FORMATTING "%s/%s"

#define MAX_USAGE 0xfff //LOOK AT LOG_FORMAT

//TYPES
typedef struct
{
    int file;
    char *fileName;
} logFile;

typedef struct
{
    char *key;
} logFlags;

//GETTERS
logFlags getLogFlags();

//SETTERS
void setLogFlags(logFlags logFlagsNew);

#endif