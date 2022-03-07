#include "../include/encrypter.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <math.h>

#define KEY_LENGTH 50

#define INVALID_KEY 1

char*
encryptText(char *text, char *keyNew)
{
    srand(time(0)); //TRY TO MAKE RANDOM GENERATION REALLY "RANDOM"

    //VARIABLES
    char *key = malloc(KEY_LENGTH);
    char *returningText;
    char *textBuffer;
    int textKeyChain[strlen(text)];
    int numberBuffer;

    if (keyNew != NULL)
    {
        if (strlen(keyNew) != KEY_LENGTH)
        {
            fprintf(stderr, "Key must be 50 characters long!\n");
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
        numberBuffer += floor(log10(abs(textKeyChain[i]))) + 1;

        //CHECK FOR MINUS
        if (textKeyChain[i] > 0) numberBuffer++;
    }

    //ALLOCATE returningText (WITH THE SEPARATORS)
    returningText = malloc(numberBuffer + (sizeof(textKeyChain) / sizeof(int) - 1));

    //LOAD returningText
    for (int i = 0; i < (sizeof(textKeyChain) / sizeof(int)); i++)
    {
        textBuffer = malloc(10);

        sprintf(textBuffer, "%d", textKeyChain[i]);

        strcat(returningText, textBuffer);

        if (i != (sizeof(textKeyChain) / sizeof(int) - 1))
        {
            strcat(returningText, ".");
        }
    }

    //DEALLOCATION
    free(key);
    free(textBuffer);
    
    return returningText;
}