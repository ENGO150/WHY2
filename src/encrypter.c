#include <stdio.h>
#include <stdlib.h>
#include <time.h>

#define KEY_LENGTH 50

char*
encryptText(char *from)
{
    srand(time(0)); //TRY TO MAKE RANDOM GENERATION REALLY "RANDOM"

    //VARIABLES
    char *key = malloc(KEY_LENGTH);
    int numberBuffer;

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

    exit(0);
    return NULL;
}