#include "../include/decrypter.h"

#include <stdio.h>
#include <string.h>
#include <stdlib.h>

#include "../include/flags.h"

char *decryptText(char *text, char *key)
{
    //CHECK FOR INVALID key
    if (strlen(key) != KEY_LENGTH)
    {
        fprintf(stderr, "Key must be 50 characters long!\n");
        exit(INVALID_KEY);
    }
    
    //VARIABLES
    char *returningText;
    int numberBuffer;
    char *textBuffer;

    numberBuffer = 1;

    //GET LENTGH OF returningText AND textKeyChain
    for (int i = 0; i < strlen(text); i++)
    {
        if (text[i] == ENCRYPTION_SEPARATOR) numberBuffer++;
    }

    //SET LENGTH
    returningText = malloc(numberBuffer);
    int textKeyChain[numberBuffer];
    int encryptedTextKeyChain[numberBuffer];

    //LOAD textKeyChain
    for (int i = 0; i < (sizeof(textKeyChain) / sizeof(int)); i++)
    {
        numberBuffer = i;

        //CHECK, IF numberBuffer ISN'T GREATER THAN KEY_LENGTH AND CUT UNUSED LENGTH
        while (numberBuffer >= KEY_LENGTH)
        {
            numberBuffer -= KEY_LENGTH;
        }

        //FILL textKeyChain
        if ((numberBuffer + 1) % 3 == 0)
        {
            textKeyChain[i] = key[numberBuffer] * key[numberBuffer + 1];
        } else if ((numberBuffer + 1) % 2 == 0)
        {
            textKeyChain[i] = key[numberBuffer] - key[numberBuffer + 1];
        } else
        {
            textKeyChain[i] = key[numberBuffer] + key[numberBuffer + 1];
        }
    }

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
    for (int i = 0; i < (sizeof(textKeyChain) / sizeof(int)); i++)
    {
        textKeyChain[i] -= encryptedTextKeyChain[i];
    }

    //LOAD returningText
    for (int i = 0; i < sizeof(textKeyChain) / sizeof(int); i++)
    {
        returningText[i] = (char) textKeyChain[i];
    }

    return returningText;
}