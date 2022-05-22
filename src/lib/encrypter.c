#include <why2/encrypter.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

#include <why2/flags.h>
#include <why2/misc.h>

outputFlags encryptText(char *text, char *keyNew, inputFlags flags)
{
    //CHECK FOR ACTIVE VERSION
    if (!flags.skipCheck) checkVersion(flags);

    //VARIABLES
    char *key = malloc(sizeof(int) * getKeyLength());
    char *returningText;
    char *textBuffer;
    int *textKeyChain = malloc(sizeof(int) * strlen(text));
    int numberBuffer;
    FILE *fileBuffer;

    //TRY TO MAKE RANDOM GENERATION REALLY "RANDOM"
    fileBuffer = fopen("/dev/urandom", "r");
    fread(&numberBuffer, 4, 1, fileBuffer);
    srand(numberBuffer);

    if (keyNew != NULL)
    {
        if (strlen(keyNew) < getKeyLength())
        {
            if (!flags.noOutput) fprintf(stderr, "Key must be at least %d characters long!\n", getKeyLength());
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
    returningText = malloc(sizeof(int) * numberBuffer + strlen(text) - 1);
    strcpy(returningText, "");

    //LOAD returningText
    for (int i = 0; i < strlen(text); i++)
    {
        numberBuffer = sizeof(int) * countIntLength(textKeyChain[i]);

        if (i != 0)
        {
            textBuffer = realloc(textBuffer, numberBuffer);
        } else
        {
            textBuffer = malloc(numberBuffer);
        }

        sprintf(textBuffer, "%d", textKeyChain[i]);

        strcat(returningText, textBuffer);

        if (i != strlen(text) - 1)
        {
            strcat(returningText, ENCRYPTION_SEPARATOR_STRING);
        }
    }

    //LOAD output
    outputFlags output =
    {
        returningText, //ENCRYPTED TEXT
        key //GENERATED/USED KEY
    };

    //DEALLOCATION
    fclose(fileBuffer);
    free(textKeyChain);
    free(textBuffer);

    return output;
}
