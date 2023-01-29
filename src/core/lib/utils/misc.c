/*
This is part of WHY2
Copyright (C) 2022 Václav Šmejkal

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

#define _XOPEN_SOURCE 500

#include <why2/misc.h>

#include <string.h>
#include <unistd.h>
#include <sys/time.h>
#include <ftw.h>

#include <curl/curl.h>
#include <json-c/json.h>
#include <git2.h>

#include <why2/flags.h>

why2_bool seedSet = 0; //DO NOT FUCKING TOUCH THIS!!!

double multiply_cb(int a, int b) { return a * b; }
double subtract_cb(int a, int b) { return a - b; }
double sum_cb(int a, int b) { return a + b; }

int unlink_cb(const char *fpath, WHY2_UNUSED const struct stat *sb, WHY2_UNUSED int typeflag, WHY2_UNUSED struct FTW *ftwbuf)
{
    int rv = remove(fpath);

    if (rv) perror(fpath);

    return rv;
}

int removeDirectory(char *path)
{
    return nftw(path, unlink_cb, 64, FTW_DEPTH | FTW_PHYS);
}

char *replaceWord(char *string, char *old, char *new) //CODE FROM: https://www.geeksforgeeks.org/c-program-replace-word-text-another-given-word
{
    char *result;
    int i, cnt = 0;
    int newLen = strlen(new);
    int oldLen = strlen(old);

    for (i = 0; string[i] != '\0'; i++)
    {
        if (strstr(&string[i], old) == &string[i])
        {
            cnt++;

            i += oldLen - 1;
        }
    }

    result = (char*) malloc(i + cnt * (newLen - oldLen) + 1);

    i = 0;
    while (*string)
    {
        // compare the substring with the result
        if (strstr(string, old) == string)
        {
            strcpy(&result[i], new);
            i += newLen;
            string += oldLen;
        }
        else result[i++] = *string++;
    }

    result[i] = '\0';
    return result;
}

enum WHY2_EXIT_CODES why2_check_version(void)
{
    if (why2_get_flags().noCheck) return WHY2_SUCCESS;

    //FILE-CHECK VARIABLES
    int notFoundBuffer = 0;

    //CURL VARIABLES
    CURL *curl = curl_easy_init();
    FILE *fileBuffer = fopen(WHY2_VERSIONS_NAME, "w+");

    //GET versions.json
    curl_easy_setopt(curl, CURLOPT_URL, WHY2_VERSIONS_URL);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, fileBuffer);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT, WHY2_CURL_TIMEOUT);

    //DOWNLOAD versions.json
    curl_easy_perform(curl);

    //CLEANUP
    curl_easy_cleanup(curl);
    fclose(fileBuffer);

    while (access(WHY2_VERSIONS_NAME, R_OK) != 0)
    {
        notFoundBuffer++;

        if (notFoundBuffer == WHY2_NOT_FOUND_TRIES)
        {
            if (!why2_get_flags().why2_no_output) fprintf(stderr, "%s'%s' not found! Exiting...\n", WHY2_CLEAR_SCREEN, WHY2_VERSIONS_NAME);
            return WHY2_DOWNLOAD_FAILED;
        }

        if (!why2_get_flags().why2_no_output) printf("%s'%s' not found (%dx)! Trying again in a second.\n", WHY2_CLEAR_SCREEN, WHY2_VERSIONS_NAME, notFoundBuffer);
        sleep(1);
    }

    //JSON VARIABLES
	char *buffer;
	struct json_object *parsedJson;
	struct json_object *active;
    int bufferSize;

    //COUNT LENGTH OF buffer AND STORE IT IN bufferSize
    fileBuffer = fopen(WHY2_VERSIONS_NAME, "r");
    fseek(fileBuffer, 0, SEEK_END);
    bufferSize = ftell(fileBuffer);
    rewind(fileBuffer); //REWIND fileBuffer (NO SHIT)

    //SET LENGTH OF buffer
    buffer = calloc(bufferSize + 1, sizeof(char));

    //LOAD jsonFile
    (void) (fread(buffer, bufferSize, 1, fileBuffer) + 1); //TODO: Try to create some function for processing exit value

    buffer[bufferSize] = '\0';

    //CHECK FOR TEXT IN buffer
    if (strcmp(buffer, "") == 0)
    {
        if (!why2_get_flags().why2_no_output) fprintf(stderr, "You probably aren't connected to internet! This release could be unsafe!\n\n");

        //WAIT FOR 5 SECONDS
        sleep(5);

        free(buffer);
        fclose(fileBuffer);
        return WHY2_SUCCESS;
    }

    //CLEANUP
	fclose(fileBuffer);

    //GET
	parsedJson = json_tokener_parse(buffer); //yes, ik, i could use json_object_from_file, but I need to check for internet somehow
	json_object_object_get_ex(parsedJson, "active", &active);

    if (strcmp(WHY2_VERSION, json_object_get_string(active)) != 0)
    {
        //UPDATE
        if (why2_get_flags().update)
        {
            //CHECK FOR ROOT PERMISSIONS
            if (getuid() != 0)
            {
                if (!why2_get_flags().why2_no_output) fprintf(stderr, "You need to be root to update!\t[I DO NOT RECOMMEND USING THIS]\n");
                return WHY2_WHY2_UPDATE_FAILED;
            }

            //VARIABLES
            git_repository *repo = NULL;
            int exitCode;
            char *installCommand;
            int installCode;

            //MESSAGE
            if (!why2_get_flags().why2_no_output) printf("Your WHY2 version is outdated!\nUpdating...\t[BETA]\n\n");

            //CHECK IF WHY2 REPO ISN'T ALREADY FOUND IN 'WHY2_UPDATE_NAME'
            if (access(WHY2_UPDATE_NAME, F_OK) == 0)
            {
                removeDirectory(WHY2_UPDATE_NAME);
            }

            git_libgit2_init(); //START GIT2

            exitCode = git_clone(&repo, WHY2_UPDATE_URL, WHY2_UPDATE_NAME, NULL); //CLONE

            git_libgit2_shutdown(); //STOP GIT2

            //CHECK FOR ERRORS
            if (exitCode != 0)
            {
                if (!why2_get_flags().why2_no_output) fprintf(stderr, "Updating failed! (cloning)\n");
                return WHY2_WHY2_UPDATE_FAILED;
            }

            //COUNT installCommand LENGTH & ALLOCATE IT
            installCommand = replaceWord(WHY2_UPDATE_COMMAND, "{DIR}", WHY2_UPDATE_NAME);

            installCode = system(installCommand); //INSTALL

            //REMOVE versions.json - OTHERWISE WILL CAUSE SEGFAULT IN NEXT RUN
            remove(WHY2_VERSIONS_NAME);

            free(installCommand);

            //CHECK FOR ERRORS
            if (installCode != 0)
            {
                if (!why2_get_flags().why2_no_output) fprintf(stderr, "Updating failed! (installing)\n");
                return WHY2_WHY2_UPDATE_FAILED;
            }

            goto deallocation; //GREAT WHY2_SUCCESS!
        }

        //COUNT WHY2_VERSIONS BEHIND
        int versionsIndex = -1;
        int versionsBuffer = 0;

	    struct json_object *deprecated;
	    json_object_object_get_ex(parsedJson, "deprecated", &deprecated);

        //COUNT versionsIndex
        for (int i = 0; i < (int) json_object_array_length(deprecated); i++)
        {
            //IT'S A MATCH, BABY :D
		    if (strcmp(json_object_get_string(json_object_array_get_idx(deprecated, i)), WHY2_VERSION) == 0)
            {
                versionsIndex = i;

                break;
            }
        }

        //versions.json DOESN'T CONTAIN WHY2_VERSION (THIS WILL NOT HAPPEN IF YOU WILL NOT EDIT IT)
        if (versionsIndex == -1)
        {
            if (!why2_get_flags().why2_no_output) printf("Version %s not found! Check your flags.\n\n", WHY2_VERSION);

            goto deallocation;
        }

        //COUNT versionsBuffer
        versionsBuffer = json_object_array_length(deprecated) - versionsIndex;

        if (!why2_get_flags().why2_no_output) fprintf(stderr, "This release could be unsafe! You're %d versions behind! (%s/%s)\n\n", versionsBuffer, WHY2_VERSION, json_object_get_string(active));

        //WAIT FOR 5 SECONDS
        sleep(5);
    }

    deallocation:

    //DEALLOCATION
    json_object_put(parsedJson); //THIS FREES EVERY json_object - AT LEAST JSON-C'S DOCUMENTATION SAYS THAT
    free(buffer);

    return WHY2_SUCCESS;
}

void why2_generate_text_key_chain(char *key, int *textKeyChain, int textKeyChainSize)
{
    int numberBuffer;
    int numberBuffer2;
    double (*cb)(int, int);

    for (int i = 0; i < textKeyChainSize; i++)
    {
        numberBuffer = i;

        //CHECK, IF numberBuffer ISN'T GREATER THAN keyLength AND CUT WHY2_UNUSED LENGTH
        while (numberBuffer >= (int) why2_get_key_length())
        {
            numberBuffer -= why2_get_key_length();
        }

        numberBuffer2 = why2_get_key_length() - (numberBuffer + (i < textKeyChainSize));

        //FILL textKeyChain
        if ((numberBuffer + 1) % 3 == 0)
        {
            cb = multiply_cb;
        }
        else if ((numberBuffer + 1) % 2 == 0)
        {
            cb = subtract_cb;
        }
        else
        {
            cb = sum_cb;
        }

        textKeyChain[textKeyChainSize - (i + 1)] = cb(key[numberBuffer], key[numberBuffer2]);
    }
}

void why2_deallocate_output(why2_output_flags flags)
{
    free(flags.outputText);
    free(flags.usedKey);

    flags.elapsedTime = 0;
    flags.exitCode = WHY2_SUCCESS;
    flags.repeatedKeySize = 0;
    flags.unusedKeySize = 0;

    flags.outputText = NULL;
    flags.usedKey = NULL;
}

enum WHY2_EXIT_CODES why2_check_key(char *key)
{
    if (strlen(key) < why2_get_key_length())
    {
        if (!why2_get_flags().why2_no_output) fprintf(stderr, "Key must be at least %lu characters long!\n", why2_get_key_length());
        return WHY2_INVALID_KEY;
    }

    return WHY2_SUCCESS;
}

enum WHY2_EXIT_CODES why2_check_text(char *text)
{
    if (strcmp(text, "") == 0)
    {
        if (!why2_get_flags().why2_no_output) fprintf(stderr, "No text to encrypt!\n");
        return WHY2_INVALID_TEXT;
    }

    return WHY2_SUCCESS;
}

unsigned long why2_count_int_length(int number)
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

unsigned long why2_count_unused_key_size(char *text, char *key)
{
    unsigned long returning = 0;

    if ((long double) (strlen(key)) / 2 > strlen(text))
    {
        returning = strlen(key) - 2 * strlen(text);
    }

    return returning;
}

unsigned long why2_count_repeated_key_size(char *text, char *key)
{
    unsigned long returning = 0;

    if (strlen(key) < 2 * strlen(text))
    {
        returning = 2 * strlen(text) - strlen(key);
    }

    return returning;
}

unsigned long why2_compare_time_micro(struct timeval startTime, struct timeval finishTime)
{
    return (finishTime.tv_sec - startTime.tv_sec) * 1000000 + finishTime.tv_usec - startTime.tv_usec;
}

void why2_generate_key(char *key, int keyLength)
{
    int numberBuffer;

    if (!seedSet)
    {
        //TRY TO MAKE RANDOM GENERATION REALLY "RANDOM"
        FILE *fileBuffer;

        fileBuffer = fopen("/dev/urandom", "r");
        (void) (fread(&numberBuffer, sizeof(numberBuffer), 1, fileBuffer) + 1); //TODO: Try to create some function for processing exit value
        numberBuffer = abs(numberBuffer); //MAKE numberBuffer POSITIVE
        srand(numberBuffer);

        fclose(fileBuffer);

        seedSet = 1;
    }

    for (int i = 0; i < keyLength; i++)
    {
        //SET numberBuffer TO RANDOM NUMBER BETWEEN 0 AND 52
        numberBuffer = (rand() % 52) + 1;

        //GET CHAR FROM numberBuffer
        if (numberBuffer > 26)
        {
            numberBuffer += 70;
        }
        else
        {
            numberBuffer += 64;
        }

        key[i] = (char) numberBuffer;
    }

    key[keyLength] = '\0';
}