#include <why2/misc.h>

#include <string.h>
#include <unistd.h>

#include <curl/curl.h>
#include <json-c/json.h>

#include <why2/flags.h>

void checkVersion(inputFlags flags)
{
    if (flags.skipCheck) return;

    //FILE-CHECK VARIABLES
    int notFoundBuffer = 0;

    //REMOVE versions.json
    if (access(VERSIONS_NAME, F_OK) == 0)
    {
        remove(VERSIONS_NAME);
    }

    //CURL VARIABLES
    CURL *curl = curl_easy_init();
    FILE *fileBuffer = fopen(VERSIONS_NAME, "w");

    //GET versions.json
    curl_easy_setopt(curl, CURLOPT_URL, VERSIONS_URL);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, fileBuffer);

    //DOWNLOAD versions.json
    curl_easy_perform(curl);

    //CLEANUP
    curl_easy_cleanup(curl);
    fclose(fileBuffer);

    while (access(VERSIONS_NAME, R_OK) != 0)
    {
        notFoundBuffer++;

        if (notFoundBuffer == NOT_FOUND_TRIES)
        {
            if (!flags.noOutput) fprintf(stderr, "%s'%s' not found! Exiting...\n", CLEAR_SCREEN, VERSIONS_NAME);
            exit(DOWNLOAD_FAILED);
        }

        if (!flags.noOutput) printf("%s'%s' not found (%dx)! Trying again in a second.\n", CLEAR_SCREEN, VERSIONS_NAME, notFoundBuffer);
        sleep(1);
    }

    //JSON VARIABLES
	char *buffer;
	char lineBuffer[32];
	struct json_object *parsedJson;
	struct json_object *active;

    //COUNT LENGTH OF buffer
    fileBuffer = fopen(VERSIONS_NAME, "r");
    fseek(fileBuffer, 0, SEEK_END);
    buffer = malloc(ftell(fileBuffer));

    rewind(fileBuffer); //REWIND fileBuffer (NO SHIT)

    //FIX buffer
    strcpy(buffer, "");

    //LOAD jsonFile
	while (fgets(lineBuffer, sizeof(lineBuffer), fileBuffer) != NULL)
    {
        strcat(buffer, lineBuffer);
    }

    //CHECK FOR TEXT IN buffer
    if (strcmp(buffer, "") == 0)
    {
        if (!flags.noOutput) fprintf(stderr, "You probably aren't connected to internet! This release could be unsafe!\n\n");

        //WAIT FOR 5 SECONDS
        sleep(5);
    }

    //CLEANUP
	fclose(fileBuffer);

    //GET
	parsedJson = json_tokener_parse(buffer);
	json_object_object_get_ex(parsedJson, "active", &active);

    if (strcmp(VERSION, json_object_get_string(active)) != 0)
    {
        //COUNT VERSIONS BEHIND
        int versionsIndex = -1;
        int versionsBuffer = 0;

	    struct json_object *deprecated;
	    json_object_object_get_ex(parsedJson, "deprecated", &deprecated);

        //COUNT versionsIndex
        for (int i = 0; i < json_object_array_length(deprecated); i++)
        {
            //IT'S A MATCH, BABY :D
		    if (strcmp(json_object_get_string(json_object_array_get_idx(deprecated, i)), VERSION) == 0)
            {
                versionsIndex = i;

                break;
            }
        }

        //versions.json DOESN'T CONTAIN VERSION (THIS WILL NOT HAPPEN IF YOU WILL NOT EDIT IT)
        if (versionsIndex == -1)
        {
            if (!flags.noOutput) printf("Version %s not found! Check your flags.\n\n", VERSION);

            free(deprecated);
            goto newerVersion;
        }

        //COUNT versionsBuffer
        versionsBuffer = json_object_array_length(deprecated) - versionsIndex;

        if (!flags.noOutput) fprintf(stderr, "This release could be unsafe! You're %d versions behind! (%s/%s)\n\n", versionsBuffer, VERSION, json_object_get_string(active));

        //WAIT FOR 5 SECONDS
        free(deprecated);
        sleep(5);
    }

    newerVersion:

    //DEALLOCATION
    free(parsedJson);
    free(active);
    free(buffer);
}

void generateTextKeyChain(char *key, int *textKeyChain, int textKeyChainSize)
{
    int numberBuffer;
    int numberBuffer2;

    for (int i = 0; i < textKeyChainSize; i++)
    {
        numberBuffer = i;

        //CHECK, IF numberBuffer ISN'T GREATER THAN keyLength AND CUT UNUSED LENGTH
        while (numberBuffer >= getKeyLength())
        {
            numberBuffer -= getKeyLength();
        }

        numberBuffer2 = getKeyLength() - (numberBuffer + (i < textKeyChainSize));

        //FILL textKeyChain
        if ((numberBuffer + 1) % 3 == 0)
        {
            textKeyChain[i] = key[numberBuffer] * key[numberBuffer2];
        } else if ((numberBuffer + 1) % 2 == 0)
        {
            textKeyChain[i] = key[numberBuffer] - key[numberBuffer2];
        } else
        {
            textKeyChain[i] = key[numberBuffer] + key[numberBuffer2];
        }
    }
}

void deallocateOutput(outputFlags flags)
{
    free(flags.outputText);
    free(flags.usedKey);
}

void checkKey(char *key, inputFlags flags)
{
    if (strlen(key) < getKeyLength())
    {
        if (!flags.noOutput) fprintf(stderr, "Key must be at least %lu characters long!\n", getKeyLength());
        exit(INVALID_KEY);
    }
}

void checkText(char *text, inputFlags flags)
{
    if (strcmp(text, "") == 0)
    {
        if (!flags.noOutput) fprintf(stderr, "No text to encrypt!\n");
        exit(1);
    }
}

int countIntLength(int number)
{
    int returning = 1;
    int buffer = 10;

    //CHECK FOR NEGATIVE NUMBER
    if (number < 0)
    {
        returning++;
        number *= -1;
    }

    //COUNT LENGTH
    while (buffer <= number)
    {
        buffer *= 10;
        returning++;
    }

    return returning;
}
