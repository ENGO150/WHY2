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

int unlink_cb(const char *fpath, const struct stat *sb, int typeflag, struct FTW *ftwbuf)
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

unsigned char checkVersion(inputFlags flags)
{
    if (flags.noCheck) return SUCCESS;

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
            return DOWNLOAD_FAILED;
        }

        if (!flags.noOutput) printf("%s'%s' not found (%dx)! Trying again in a second.\n", CLEAR_SCREEN, VERSIONS_NAME, notFoundBuffer);
        sleep(1);
    }

    //JSON VARIABLES
	char *buffer;
	struct json_object *parsedJson;
	struct json_object *active;
    int bufferSize;

    //COUNT LENGTH OF buffer AND STORE IT IN bufferSize
    fileBuffer = fopen(VERSIONS_NAME, "r");
    fseek(fileBuffer, 0, SEEK_END);
    bufferSize = ftell(fileBuffer);
    rewind(fileBuffer); //REWIND fileBuffer (NO SHIT)

    //SET LENGTH OF buffer
    buffer = malloc(bufferSize + 1);

    //FIX buffer
    strcpy(buffer, "");

    //LOAD jsonFile
    fread(buffer, bufferSize, 1, fileBuffer);

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
        //UPDATE
        if (!flags.noUpdate)
        {
            //CHECK FOR ROOT PERMISSIONS
            if (getuid() != 0)
            {
                if (!flags.noOutput) fprintf(stderr, "You need to be root to update!\t[I DO NOT RECOMMEND USING THIS]\n");
                return UPDATE_FAILED;
            }

            //VARIABLES
            git_repository *repo = NULL;
            int exitCode;
            char *installCommand;
            int installCode;

            //MESSAGE
            if (!flags.noOutput) printf("Your WHY2 version is outdated!\nUpdating...\t[BETA]\n\n");

            //CHECK IF WHY2 REPO ISN'T ALREADY FOUND IN 'UPDATE_NAME'
            if (access(UPDATE_NAME, F_OK) == 0)
            {
                removeDirectory(UPDATE_NAME);
            }

            git_libgit2_init(); //START GIT2

            exitCode = git_clone(&repo, UPDATE_URL, UPDATE_NAME, NULL); //CLONE

            git_libgit2_shutdown(); //STOP GIT2

            //CHECK FOR ERRORS
            if (exitCode != 0)
            {
                if (!flags.noOutput) fprintf(stderr, "Updating failed! (cloning)\n");
                return UPDATE_FAILED;
            }

            //COUNT installCommand LENGTH & ALLOCATE IT
            installCommand = replaceWord(UPDATE_COMMAND, "{DIR}", UPDATE_NAME);

            installCode = system(installCommand); //INSTALL

            //REMOVE versions.json - OTHERWISE WILL CAUSE SEGFAULT IN NEXT RUN
            remove(VERSIONS_NAME);

            //CHECK FOR ERRORS
            if (installCode != 0)
            {
                if (!flags.noOutput) fprintf(stderr, "Updating failed! (installing)\n");
                return UPDATE_FAILED;
            }

            goto deallocation; //GREAT SUCCESS!
        }

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
            goto deallocation;
        }

        //COUNT versionsBuffer
        versionsBuffer = json_object_array_length(deprecated) - versionsIndex;

        if (!flags.noOutput) fprintf(stderr, "This release could be unsafe! You're %d versions behind! (%s/%s)\n\n", versionsBuffer, VERSION, json_object_get_string(active));

        //WAIT FOR 5 SECONDS
        free(deprecated);
        sleep(5);
    }

    deallocation:

    //DEALLOCATION
    free(parsedJson);
    free(active);
    free(buffer);

    return SUCCESS;
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
        }
        else if ((numberBuffer + 1) % 2 == 0)
        {
            textKeyChain[i] = key[numberBuffer] - key[numberBuffer2];
        }
        else
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

unsigned char checkKey(char *key, inputFlags flags)
{
    if (strlen(key) < getKeyLength())
    {
        if (!flags.noOutput) fprintf(stderr, "Key must be at least %lu characters long!\n", getKeyLength());
        return INVALID_KEY;
    }

    return SUCCESS;
}

unsigned char checkText(char *text, inputFlags flags)
{
    if (strcmp(text, "") == 0)
    {
        if (!flags.noOutput) fprintf(stderr, "No text to encrypt!\n");
        return INVALID_TEXT;
    }

    return SUCCESS;
}

unsigned long countIntLength(int number)
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

unsigned long countUnusedKeySize(char *text, char *key)
{
    unsigned long returning = 0;

    if (strlen(key) / 2 > strlen(text))
    {
        returning = strlen(key) - 2 * strlen(text);
    }

    return returning;
}

unsigned long compareTimeMicro(struct timeval startTime, struct timeval finishTime)
{
    return (finishTime.tv_sec - startTime.tv_sec) * 1000000 + finishTime.tv_usec - startTime.tv_usec;
}