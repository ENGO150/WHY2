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
    char *emptyText = malloc(1); //TEXT
    emptyText[0] = '\0';

    char *emptyKey = malloc(getKeyLength() + 1); //KEY
    for (int i = 0; i < (int) getKeyLength(); i++)
    {
        emptyKey[i] = 'x';
    }
    emptyKey[getKeyLength()] = '\0';

    return (outputFlags) { emptyText, emptyKey, 0, 0, exitCode };
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