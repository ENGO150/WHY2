#include <why2/decrypter.h>

#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <sys/time.h>

#include <why2/flags.h>
#include <why2/misc.h>

outputFlags decryptText(char *text, char *keyNew)
{
    //CHECK VARIABLE
    int checkExitCode;

    //TIME VARIABLES
    struct timeval startTime;
    struct timeval finishTime;
    gettimeofday(&startTime, NULL);

    //CHECK FOR ACTIVE VERSION
    if ((checkExitCode = checkVersion()) != SUCCESS)
    {
        return noOutput(checkExitCode);
    }

    //CHECK FOR INVALID text
    if ((checkExitCode = checkText(text)) != SUCCESS)
    {
        return noOutput(checkExitCode);
    }

    //CHECK FOR INVALID key
    if ((checkExitCode = checkKey(keyNew)) != SUCCESS)
    {
        return noOutput(checkExitCode);
    }

    //REDEFINE keyLength
    setKeyLength(strlen(keyNew));

    //VARIABLES
    char *returningText;
    int numberBuffer = 1;
    char *textBuffer;
    int textKeyChainLength;
    int *textKeyChain;
    char *key = malloc(strlen(keyNew) + 1);
    int *encryptedTextKeyChain;

    //COPY keyNew TO key
    strcpy(key, keyNew);

    //GET LENGTH OF returningText AND textKeyChain
    for (int i = 0; i < (int) strlen(text); i++)
    {
        if (text[i] == getEncryptionSeparator()) numberBuffer++;
    }

    //SET LENGTH (numberBuffer)
    returningText = malloc(numberBuffer + 1);
    textKeyChain = malloc(sizeof(int) * numberBuffer);
    encryptedTextKeyChain = malloc(sizeof(int) * numberBuffer);
    textKeyChainLength = numberBuffer;

    //LOAD textKeyChain
    generateTextKeyChain(key, textKeyChain, numberBuffer);

    //LOAD encryptedTextKeyChain
    for (int i = 0; i < textKeyChainLength; i++)
    {
        numberBuffer = 0;

        //GET LENGTH OF EACH CHARACTER
        for (int j = 0; j < (int) strlen(text); j++)
        {
            if (text[j] == getEncryptionSeparator()) break;

            numberBuffer++;
        }

        if (i != 0)
        {
            textBuffer = realloc(textBuffer, numberBuffer + 1);
        } else
        {
            textBuffer = malloc(numberBuffer + 1);
        }

        //CLEAN (POSSIBLY EXISTING) JUNK in textBuffer
        for (int i = 0; i <= numberBuffer; i++)
        {
            textBuffer[i] = '\0';
        }

        //LOAD textBuffer
        for (int j = 0; j < (int) strlen(text); j++)
        {
            textBuffer[j] = text[j];

            if (numberBuffer == j) break;
        }

        encryptedTextKeyChain[i] = atoi(textBuffer);

        text += numberBuffer + 1;
    }

    //DECRYPT TEXT
    for (int i = 0; i < textKeyChainLength; i++)
    {
        textKeyChain[i] -= encryptedTextKeyChain[i];
    }

    //FIX (CLEAN) returningText
    for (int i = 0; i <= textKeyChainLength; i++)
    {
        returningText[i] = '\0';
    }

    //LOAD returningText
    for (int i = 0; i < textKeyChainLength; i++)
    {
        returningText[i] = textKeyChain[i];
    }

    //GET FINISH TIME
    gettimeofday(&finishTime, NULL);

    //LOAD output
    outputFlags output =
    {
        returningText, //DECRYPTED TEXT
        key, //USED KEY
        countUnusedKeySize(returningText, key), // NUMBER OF UNUSED CHARS IN KEY
        countRepeatedKeySize(returningText, key), //NUMBER OF REPEATED CHARS IN KEY
        compareTimeMicro(startTime, finishTime), // ELAPSED TIME
        SUCCESS //EXIT CODE
    };

    //DEALLOCATION
    free(textKeyChain);
    free(encryptedTextKeyChain);
    free(textBuffer);

    return output;
}
