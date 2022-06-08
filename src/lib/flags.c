#include <why2/flags.h>

unsigned long getKeyLength()
{
    return keyLength;
}

inputFlags noFlags()
{
    return (inputFlags) {0, 0, 0};
}

void setKeyLength(int keyLengthNew)
{
    keyLength = keyLengthNew;
}