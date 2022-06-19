#include <why2/decrypter.h>

#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <sys/time.h>

#include <why2/flags.h>
#include <why2/misc.h>

outputFlags decryptText(char *text, char *keyNew, inputFlags flags)
{
    //CHECK VARIABLE
    int checkExitCode;

    //TIME VARIABLES
    struct timeval startTime;
    struct timeval finishTime;
    gettimeofday(&startTime, NULL);

    //CHECK FOR ACTIVE VERSION
    if ((checkExitCode = checkVersion(flags)) != SUCCESS)
    {
        return noOutput(checkExitCode);
    }

    //CHECK FOR INVALID text
    if ((checkExitCode = checkText(text, flags)) != SUCCESS)
    {
        return noOutput(checkExitCode);
    }

    //CHECK FOR INVALID key
    if ((checkExitCode = checkKey(keyNew, flags)) != SUCCESS)
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
    int encryptedTextKeyChain[sizeof(int) * numberBuffer];
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

        textBuffer = malloc(numberBuffer + 1);

        //LOAD textBuffer
        for (int j = 0; j < (int) strlen(text); j++)
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

    //GET FINISH TIME
    gettimeofday(&finishTime, NULL);

    //LOAD output
    outputFlags output =
    {
        returningText, //DECRYPTED TEXT
        key, //USED KEY
        countUnusedKeySize(returningText, key), // NUMBER OF UNUSED CHARS IN KEY
        compareTimeMicro(startTime, finishTime), // ELAPSED TIME
        SUCCESS //EXIT CODE
    };

    //DEALLOCATION
    free(textKeyChain);

    return output;
}
