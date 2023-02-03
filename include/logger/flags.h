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
const enum WHY2_LOGGER_EXIT_CODES //exit codes you fucking idiot (2#)
{
    INVALID_FILE = -1 //THIS WILL HAPPEN IF YOU USE TOO MUCH LOGS lol
};

#define WHY2_WRITE_MESSAGE_1 "Hello from logger-test! 👋"
#define WHY2_WRITE_MESSAGE_2 "aAzZ(    )!?#\\/śŠ <3|420*;㍿㊓ㅅΔ♛👶🏿"
#define WHY2_WRITE_MESSAGE_3 "ˢᵃʸ ʸᵉˢ ᵗᵒ ᵈʳᵘᵍˢ"

#define WHY2_WRITE_DIR "./logs"
#define WHY2_LOG_LATEST "latest.log"

#define WHY2_LOG_FORMAT "yyyy-mm-dd_xxx.log" //THE LAST xxx IS FOR BASE-16 NUMBER OF USAGE THAT DAY (SO MAX IS 4095 [FFF] USAGES PER DAY)
#define WHY2_LOG_FORMAT_START "yyyy-mm-dd" //FIRST PART OF WHY2_LOG_FORMAT

#define WHY2_LOG_FORMATTING "%s/%s_%03x.log" //SAME THING AS WHY2_LOG_FORMAT, BUT USED IN sprintf IN logger.c, NOT AS LENGTH
#define WHY2_LOG_FORMATTING_START "%04d-%02d-%02d" //FIRST PART OF WHY2_LOG_FORMATTING

#define WHY2_WRITE_FORMAT "[hh:mm:ss]: " //LOG MESSAGE'S PREFIX (THE SPACE AT THE END IS INTENTIONAL)
#define WHY2_WRITE_FORMATTING "[%02d:%02d:%02d]: %s\n" //guess what

#define WHY2_LOG_LATEST_FORMATTING "%s/%s"

#define WHY2_MAX_USAGE 0xfff //LOOK AT WHY2_LOG_FORMAT

//TYPES
typedef struct
{
    int file;
    char *filename;
} why2_log_file;

typedef struct
{
    char *key;
} why2_log_flags;

typedef struct
{
    char **decrypted_text;
    unsigned long length;
} why2_decrypted_output;

//GETTERS
why2_log_flags why2_get_log_flags(void);

//SETTERS
void why2_set_log_flags(why2_log_flags why2_log_flagsNew);

#endif