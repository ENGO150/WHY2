#include "../include/encrypter.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <math.h>

#include "../include/flags.h"
#include "../include/misc.h"

char*
encryptText(char *text, char *keyNew)
{
    //CHECK FOR ACTIVE VERSION
    checkVersion();

    srand(time(0)); //TRY TO MAKE RANDOM GENERATION REALLY "RANDOM"

    //VARIABLES
    char *key = malloc(KEY_LENGTH);
    char *returningText;
    char *textBuffer;
    int textKeyChain[strlen(text)];
    int numberBuffer;

    if (keyNew != NULL)
    {
        if (strlen(keyNew) < KEY_LENGTH)
        {
            fprintf(stderr, "Key must be at least %d characters long!\n", KEY_LENGTH);
            exit(INVALID_KEY);
        }

        strcpy(key, keyNew);

        goto skipKey;
    }

    //LOAD KEY
    for (int i = 0; i < KEY_LENGTH; i++)
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

    printf("Your key is: %s\n!!! SAVE IT SOMEWHERE !!!\n\n", key);

    skipKey:

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

    //ACTUALLY ENCRYPT TEXT
    for (int i = 0; i < strlen(text); i++)
    {
        textKeyChain[i] -= (int) text[i];
    }

    numberBuffer = 0;

    //COUNT REQUIRED SIZE FOR returningText
    for (int i = 0; i < (sizeof(textKeyChain) / sizeof(int)); i++)
    {
        numberBuffer += countIntLength(textKeyChain[i]);
    }

    //ALLOCATE returningText (WITH THE SEPARATORS)
    returningText = malloc(numberBuffer + (sizeof(textKeyChain) / sizeof(int) - 1));
    strcpy(returningText, "");

    //LOAD returningText
    for (int i = 0; i < (sizeof(textKeyChain) / sizeof(int)); i++)
    {
        textBuffer = malloc(countIntLength(textKeyChain[i]));

        sprintf(textBuffer, "%d", textKeyChain[i]);

        strcat(returningText, textBuffer);

        if (i != (sizeof(textKeyChain) / sizeof(int) - 1))
        {
            strcat(returningText, ENCRYPTION_SEPARATOR_STRING);
        }

        free(textBuffer);
    }

    //DEALLOCATION
    free(key);

    return returningText;
}
