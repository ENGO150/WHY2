#include <why2/flags.h>

#include <stdlib.h>

char getEncryptionSeparator()
{
    return encryptionSeparator;
}

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
    char *empty1 = malloc(1); //TEXT
    empty1[0] = '\0';

    char *empty2 = malloc(getKeyLength() + 1); //KEY
    for (int i = 0; i < getKeyLength(); i++)
    {
        empty2[i] = 'x';
    }
    empty2[getKeyLength()] = '\0';

    return (outputFlags) { empty1, empty2, 0, 0, exitCode };
}

void setEncryptionSeparator(char encryptionSeparatorNew)
{
    if (encryptionSeparatorNew == '\0') return;

    encryptionSeparator = encryptionSeparatorNew;
}

void setKeyLength(int keyLengthNew)
{
    keyLength = keyLengthNew;
}