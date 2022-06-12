#include <why2/flags.h>

#include <stdlib.h>

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
    char *empty1 = malloc(1);
    char *empty2 = malloc(1);
    empty1[0] = '\0';
    empty2[0] = '\0';

    return (outputFlags) { empty1, empty2, 0, 0, exitCode };
}

void setKeyLength(int keyLengthNew)
{
    keyLength = keyLengthNew;
}