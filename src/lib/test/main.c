#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <why2.h>

int main(void)
{
    //VARIABLES
    char *textBuffer = malloc(128);
    char *keyBuffer;
    int exitCode = 0;

    //FLAGS
    inputFlags flags =
    {
        0, //SKIP CHECK
        0, //NO OUTPUT
    };

    //SET KEY_LENGTH TO 100
    setKeyLength(100);
    keyBuffer = malloc(getKeyLength());

    //ENCRYPT & DECRYPT
    outputFlags encrypted = encryptText(TEST_TEXT, NULL, flags);

    strcpy(textBuffer, encrypted.outputText); //GET ENCRYPTED TEXT
    strcpy(keyBuffer, encrypted.usedKey); //GET KEY

    //DEALLOCATE BUFFER
    deallocateOutput(encrypted);

    encrypted = decryptText(textBuffer, keyBuffer, flags);

    //COMPARE DIFFERENCE
    if (strcmp(encrypted.outputText, TEST_TEXT) == 0)
    {
        printf("Test successful!\n\nTEXT: %s\nOUTPUT: %s\nKEY: %s\n", TEST_TEXT, textBuffer, encrypted.usedKey);
    }
    else
    {
        fprintf(stderr, "Test failed!\n\n%s // %s\n", encrypted.outputText, TEST_TEXT);
        exitCode = 1;
    }

    //DEALLOCATION
    free(textBuffer);
    free(keyBuffer);
    deallocateOutput(encrypted);

    return exitCode;
}
