#include <why2/decrypter.h>

#include <stdio.h>
#include <string.h>
#include <stdlib.h>

#include <why2/flags.h>
#include <why2/misc.h>

outputFlags decryptText(char *text, char *keyNew, inputFlags flags)
{
    //CHECK FOR INVALID text
    checkText(text, flags);

    //CHECK FOR INVALID key
    checkKey(keyNew, flags);

    //REDEFINE keyLength
    setKeyLength(strlen(keyNew));

    //VARIABLES
    char *returningText;
    int numberBuffer = 1;
    char *textBuffer;
    int textKeyChainLength;
    int *textKeyChain;
    char *key = malloc(strlen(keyNew));

    //COPY keyNew TO key
    strcpy(key, keyNew);

    //GET LENGTH OF returningText AND textKeyChain
    for (int i = 0; i < strlen(text); i++)
    {
        if (text[i] == ENCRYPTION_SEPARATOR) numberBuffer++;
    }

    //SET LENGTH (numberBuffer)
    returningText = malloc(numberBuffer);
    textKeyChain = malloc(sizeof(int) * numberBuffer);
    int encryptedTextKeyChain[sizeof(int) * numberBuffer];
    textKeyChainLength = numberBuffer;

    //LOAD textKeyChain
    generateTextKeyChain(key, textKeyChain, numberBuffer);

    //LOAD encryptedTextKeyChain
    for (int i = 0; i < textKeyChainLength; i++)
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

    //FIX returningText
    strcpy(returningText, "");
    for (int i = 0; i < textKeyChainLength; i++)
    {
        strcat(returningText, " ");
    }

    //LOAD returningText
    for (int i = 0; i < textKeyChainLength; i++)
    {
        returningText[i] = textKeyChain[i];
    }

    //LOAD output
    outputFlags output =
    {
        returningText, //DECRYPTED TEXT
        key, //USED KEY
        countUnusedKeySize(text, key)
    };

    //DEALLOCATION
    free(textKeyChain);

    return output;
}
