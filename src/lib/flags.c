#include <why2/flags.h>

unsigned long getKeyLength()
{
    return keyLength;
}

inputFlags noFlags()
{
    return (inputFlags) {0, 0, 1};
}

outputFlags noOutput(unsigned char exitCode)
{
    return (outputFlags) {"", "", 0, 0, exitCode};
}

void setKeyLength(int keyLengthNew)
{
    keyLength = keyLengthNew;
}