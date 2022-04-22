#include "../include/misc.h"

#include <string.h>
#include <unistd.h>

#include <curl/curl.h>
#include <json-c/json.h>

#include "../include/flags.h"

void
checkVersion()
{
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

    //CLEANUP
	fclose(fileBuffer);

    //GET
	parsedJson = json_tokener_parse(buffer);
	json_object_object_get_ex(parsedJson, "active", &active);

    if (strcmp(VERSION, json_object_get_string(active)) != 0)
    {
        fprintf(stderr, "Your version isn't latest! This release could be unsafe!\n");

        //WAIT FOR 5 SECONDS
        sleep(5);
    }
}
