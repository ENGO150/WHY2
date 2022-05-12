#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <why2.h>

int main(int argc, char *argv[])
{
    char *buffer;
    int githubActions = argc == 2 && strcmp(argv[1], "--github") == 0; //THIS IS JUST SHIT FOR GITHUB ACTIONS (LOW RAM)

    inputFlags flags =
    {
        githubActions, //SKIP CHECK
        0, //NO OUTPUT
    };

    outputFlags encrypted = encryptText(TEST_TEXT, NULL, flags);

    buffer = encrypted.outputText; //GET ENCRYPTED TEXT

    encrypted = decryptText(encrypted.outputText, encrypted.usedKey, flags);

    if (strcmp(encrypted.outputText, TEST_TEXT) == 0)
    {
        printf("Test successful!\n\nTEXT: %s\nOUTPUT: %s\nKEY: %s\n", TEST_TEXT, buffer, encrypted.usedKey);
    }
    else
    {
        fprintf(stderr, "Test failed!\n");
        exit(1);
    }

    //DEALLOCATION
    if (!githubActions)
    {
        free(buffer);
        deallocateOutput(encrypted);
    }

    return 0;
}
