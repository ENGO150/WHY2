#include <why2/flags.h>

unsigned long getKeyLength()
{
    return keyLength;
}

inputFlags noFlags()
{
    return (inputFlags) {0, 0, 1};
}

void setKeyLength(int keyLengthNew)
{
    keyLength = keyLengthNew;
}