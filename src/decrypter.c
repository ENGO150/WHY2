#include <why2/decrypter.h>

#include <stdio.h>
#include <string.h>
#include <stdlib.h>

#include <why2/flags.h>
#include <why2/misc.h>

char*
decryptText(char *text, char *key)
{
    //CHECK FOR INVALID key
    if (strlen(key) < getKeyLength())
    {
        fprintf(stderr, "Key must be at least %d characters long!\n", getKeyLength());
        exit(INVALID_KEY);
    }

    //REDEFINE keyLength
    setKeyLength(strlen(key));

    //VARIABLES
    char *returningText;
    int numberBuffer = 1;
    char *textBuffer;
    int textKeyChainLength;
    int *textKeyChain;

    //GET LENGTH OF returningText AND textKeyChain
    for (int i = 0; i < strlen(text); i++)
    {
        if (text[i] == ENCRYPTION_SEPARATOR) numberBuffer++;
    }

    //SET LENGTH (numberBuffer)
    returningText = malloc(numberBuffer);
    textKeyChain = malloc(numberBuffer * sizeof(int));
    int encryptedTextKeyChain[numberBuffer];
    textKeyChainLength = numberBuffer;

    //LOAD textKeyChain
    generateTextKeyChain(key, textKeyChain, numberBuffer);

    //LOAD encryptedTextKeyChain
    for (int i = 0; i < (sizeof(encryptedTextKeyChain) / sizeof(int)); i++)
    {
        numberBuffer = 0;

        //GET LENGTH OF EACH CHARACTER
        for (int j = 0; j < strlen(text); j++)
        {
            if (text[j] == ENCRYPTION_SEPARATOR) break;

            numberBuffer++;
        }

        textBuffer = malloc(numberBuffer);

        //LOAD textBuffer
        for (int j = 0; j < strlen(text); j++)
        {
            textBuffer[j] = text[j];

            if (numberBuffer == j) break;
        }

        encryptedTextKeyChain[i] = atoi(textBuffer);

        text += numberBuffer + 1;
        free(textBuffer);
    }

    //DECRYPT TEXT
    for (int i = 0; i < textKeyChainLength; i++)
    {
        textKeyChain[i] -= encryptedTextKeyChain[i];
    }

    //LOAD returningText
    for (int i = 0; i < textKeyChainLength; i++)
    {
        returningText[i] = (char) textKeyChain[i];
    }

    //DEALLOCATION
    free(textKeyChain);

    return returningText;
}
