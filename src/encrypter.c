#include "../include/encrypter.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <math.h>
#include <unistd.h>

#include <curl/curl.h>
#include <json-c/json.h>

#include "../include/flags.h"

char*
encryptText(char *text, char *keyNew)
{
    //CHECK FOR ACTIVE VERSION
    
    //CURL VARIABLES
    CURL *curl = curl_easy_init();
    FILE *fileBuffer = fopen("versions.json", "w");
    
    //GET versions.json
    curl_easy_setopt(curl, CURLOPT_URL, "https://raw.githubusercontent.com/ENDev-WHY2/WHY2-Encryption-System/c/versions.json");
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, fileBuffer);

    //DOWNLOAD versions.json
    curl_easy_perform(curl);

    //CLEANUP
    curl_easy_cleanup(curl);
    fclose(fileBuffer);

    //JSON VARIABLES
    FILE *jsonFile = fopen("versions.json", "r");
	char buffer[256];
	char lineBuffer[64];
	struct json_object *parsedJson;
	struct json_object *active;

    //LOAD jsonFile
	while (fgets(lineBuffer, sizeof(lineBuffer), jsonFile) != NULL)
    {
        strcat(buffer, lineBuffer);
    }

    //CLEANUP
	fclose(jsonFile);

    //GET
	parsedJson = json_tokener_parse(buffer);
	json_object_object_get_ex(parsedJson, "active", &active);

    if (strcmp(VERSION, json_object_get_string(active)) != 0)
    {
        fprintf(stderr, "Your version isn't latest! This release could be unsafe!\n");

        //WAIT FOR 5 SECONDS
        sleep(5);
    }

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
        textBuffer = malloc(floor(log10(abs(textKeyChain[i]))));

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