#ifndef WHY2_LOGGER_FLAGS_H
#define WHY2_LOGGER_FLAGS_H

//CONSTS
const enum RETURN_CODES //exit codes you fucking idiot (2#)
{
    INVALID_FILE = -1 //THIS WILL HAPPEN IF YOU USE TOO MUCH LOGS lol
};

#define LOG_FORMAT "yyyy-mm-dd_xxx.log" //THE LAST xxx IS FOR BASE-16 NUMBER OF USAGE THAT DAY (SO MAX IS 4095 [FFF] USAGES PER DAY)

#endif