#include <why2/encrypter.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <math.h>

#include <why2/flags.h>
#include <why2/misc.h>

char*
encryptText(char *text, char *keyNew)
{
    //CHECK FOR ACTIVE VERSION
    checkVersion();

    srand(time(0)); //TRY TO MAKE RANDOM GENERATION REALLY "RANDOM"

    //VARIABLES
    char *key = malloc(getKeyLength());
    char *returningText;
    char *textBuffer;
    int *textKeyChain = malloc(strlen(text) * sizeof(int));
    int numberBuffer = 0;

    if (keyNew != NULL)
    {
        if (strlen(keyNew) < getKeyLength())
        {
            fprintf(stderr, "Key must be at least %d characters long!\n", getKeyLength());
            exit(INVALID_KEY);
        }

        strcpy(key, keyNew);

        //REDEFINE keyLength
        setKeyLength(strlen(key));

        goto skipKey;
    }

    //LOAD KEY
    for (int i = 0; i < getKeyLength(); i++)
    {
        //SET numberBuffer TO RANDOM NUMBER BETWEEN 0 AND 52
        numberBuffer = (rand() % 52) + 1;
        
        //GET CHAR FROM numberBuffer
        if (numberBuffer > 26)
        {
            numberBuffer += 70;
        } else
        {
            numberBuffer += 64;
        }

        key[i] = (char) numberBuffer;
    }

    if (!noOutput) printf("Your key is: %s\n!!! SAVE IT SOMEWHERE !!!\n\n", key);

    skipKey:

    //LOAD textKeyChain
    generateTextKeyChain(key, textKeyChain, strlen(text));

    //ACTUALLY ENCRYPT TEXT
    for (int i = 0; i < strlen(text); i++)
    {
        textKeyChain[i] -= (int) text[i];
    }

    //COUNT REQUIRED SIZE FOR returningText
    for (int i = 0; i < strlen(text); i++)
    {
        numberBuffer += countIntLength(textKeyChain[i]);
    }

    //ALLOCATE returningText (WITH THE SEPARATORS)
    returningText = malloc(numberBuffer + strlen(text) - 1);
    strcpy(returningText, "");

    //LOAD returningText
    for (int i = 0; i < strlen(text); i++)
    {
        textBuffer = malloc(countIntLength(textKeyChain[i]));

        sprintf(textBuffer, "%d", textKeyChain[i]);

        strcat(returningText, textBuffer);

        if (i != strlen(text) - 1)
        {
            strcat(returningText, ENCRYPTION_SEPARATOR_STRING);
        }

        free(textBuffer);
    }

    //DEALLOCATION
    free(key);

    return returningText;
}
