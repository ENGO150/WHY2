#include <why2/flags.h>

#include <stdlib.h>

//CONSTS (this is just local)
#define DEFAULT_FLAGS (inputFlags) { 0, 0, 0 }

//VARIABLES
static char encryptionSeparator = '.'; //NOPE     > DO NOT TOUCH THIS, USE setEncryptionSeparator instead <
static unsigned long keyLength = 50; //LENGTH OF KEY     > DO NOT TOUCH THIS, USE setKeyLength instead <
static inputFlags flags = DEFAULT_FLAGS;

//GETTERS
char getEncryptionSeparator()
{
    return encryptionSeparator;
}

unsigned long getKeyLength()
{
    return keyLength;
}

inputFlags defaultFlags()
{
    return DEFAULT_FLAGS;
}

inputFlags getFlags()
{
    return flags;
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

//SETTERS
void setEncryptionSeparator(char encryptionSeparatorNew)
{
    if (encryptionSeparatorNew == '\0') return;

    encryptionSeparator = encryptionSeparatorNew;
}

void setKeyLength(int keyLengthNew)
{
    keyLength = keyLengthNew;
}

void setFlags(inputFlags newFlags)
{
    flags = newFlags;
}