#include <why2/misc.h>

#include <string.h>
#include <unistd.h>

#include <curl/curl.h>
#include <json-c/json.h>

#include <why2/flags.h>

#define CLEAR_SCREEN "\e[1;1H\e[2J"
#define NOT_FOUND_TRIES 10

#define DOWNLOAD_FAILED 1

void
checkVersion()
{
    if (SKIP_CHECK) return;

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
            fprintf(stderr, "%s'%s' not found!\n", CLEAR_SCREEN, VERSIONS_NAME);
            exit(DOWNLOAD_FAILED);
        }

        printf("%s'%s' not found (%dx)! Trying again in a second.\n", CLEAR_SCREEN, VERSIONS_NAME, notFoundBuffer);
        sleep(1);
    }

    //JSON VARIABLES
    fileBuffer = fopen(VERSIONS_NAME, "r");
	char buffer[256];
	char lineBuffer[64];
	struct json_object *parsedJson;
	struct json_object *active;

    //LOAD jsonFile
	while (fgets(lineBuffer, sizeof(lineBuffer), fileBuffer) != NULL)
    {
        strcat(buffer, lineBuffer);
    }

    //CHECK FOR TEXT IN buffer
    if (strcmp(buffer, "") == 0)
    {
        fprintf(stderr, "You probably aren't connected to internet! This release could be unsafe!\n\n");

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
        fprintf(stderr, "Your version isn't latest! This release could be unsafe! (%s/%s)\n\n", VERSION, json_object_get_string(active));

        //WAIT FOR 5 SECONDS
        sleep(5);
    }
}

void
generateTextKeyChain(char key[], int *textKeyChain, int textKeyChainSize)
{
    int numberBuffer;
    
    for (int i = 0; i < textKeyChainSize; i++)
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
}

int
countIntLength(int number)
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
