#ifndef WHY2_LOGGER_FLAGS_H
#define WHY2_LOGGER_FLAGS_H

//CONSTS
const enum RETURN_CODES //exit codes you fucking idiot (2#)
{
    INVALID_FILE = -1 //THIS WILL HAPPEN IF YOU USE TOO MUCH LOGS lol
};

#define LOG_FORMAT "yyyy-mm-dd_xxx.log" //THE LAST xxx IS FOR BASE-16 NUMBER OF USAGE THAT DAY (SO MAX IS 4095 [FFF] USAGES PER DAY)
#define LOG_FORMATTING "%s/%04d-%02d-%02d_%03x" //SAME THING AS LOG_FORMAT, BUT USED IN sprintf IN logger.c, NOT AS LENGTH

#define WRITE_FORMAT "[hh:mm:ss] " //LOG MESSAGE'S PREFIX (THE SPACE AT THE END IS INTENTIONAL)
#define WRITE_FORMATTING "[%02d:%02d:%02d] %s" //guess what

#define MAX_USAGE 0xfff //LOOK AT LOG_FORMAT

#endif