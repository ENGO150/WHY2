#include <why2/flags.h>

int getKeyLength()
{
    return keyLength;
}

inputFlags noFlags()
{
    return (inputFlags) {0, 0};
}

void setKeyLength(int keyLengthNew)
{
    keyLength = keyLengthNew;
}