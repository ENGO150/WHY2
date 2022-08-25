#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <why2.h>

int main(void)
{
    //VARIABLES
    char *textBuffer = malloc(256);
    char *keyBuffer;
    char *statusBuffer;
    char *outputBuffer = malloc(1); //THIS IS TEMP ALLOCATION
    int exitCode = 0;
    unsigned long timeBuffer;
    FILE *outputStreamBuffer = stdout;

    //FLAGS
    inputFlags flags =
    {
        0, //SKIP CHECK
        0, //NO OUTPUT
        0 //UPDATE
    };

    //SET FLAGS
    setFlags(flags);

    //SET KEY_LENGTH TO 100
    setKeyLength(100);
    keyBuffer = malloc(getKeyLength() + 1);

    //SET ENCRYPTION_SEPARATOR TO '|'
    setEncryptionSeparator('|');

    //SET outputBuffer to NULL
    outputBuffer[0] = '\0';

    //ENCRYPT
    outputFlags encrypted = encryptText(TEST_TEXT, NULL);

    strcpy(textBuffer, encrypted.outputText); //GET ENCRYPTED TEXT
    strcpy(keyBuffer, encrypted.usedKey); //GET KEY
    timeBuffer = encrypted.elapsedTime; //GET TIME 1

    //DEALLOCATE BUFFER
    deallocateOutput(encrypted);

    //DECRYPT
    encrypted = decryptText(textBuffer, keyBuffer);

    timeBuffer += encrypted.elapsedTime; //GET TIME 1

    //COMPARE DIFFERENCE
    if (strcmp(encrypted.outputText, TEST_TEXT) == 0 && encrypted.exitCode == 0) //SUCCESS
    {
        statusBuffer = malloc(11);
        strcpy(statusBuffer, "successful");
    }
    else //FAILURE
    {
        statusBuffer = malloc(8);
        strcpy(statusBuffer, "failed");

        outputBuffer = realloc(outputBuffer, strlen(encrypted.outputText) + 6);
        sprintf(outputBuffer, "\t\t\"%s\"\n", encrypted.outputText);

        outputStreamBuffer = stderr;
        exitCode = 1;
    }

    //PRINT OUTPUT
    fprintf
    (
        outputStreamBuffer,

        "Test %s!\n\n"

        "TEXT: \t\t\"%s\"\n%s"
        "OUTPUT: \t\"%s\"\n"
        "KEY: \t\t\"%s\"\n"
        "TIME: \t\t\"%lums\"\n"
        "UNUSED KEY: \t\"%lu\"\n"
        "REPEATED KEY: \t\"%lu\"\n"
        "EXIT CODE: \t\"%d\"\n"

        , statusBuffer, TEST_TEXT, outputBuffer, textBuffer, encrypted.usedKey, timeBuffer / 1000, encrypted.unusedKeySize, encrypted.repeatedKeySize, encrypted.exitCode
    );

    //DEALLOCATION
    free(textBuffer);
    free(keyBuffer);
    free(statusBuffer);
    free(outputBuffer);
    deallocateOutput(encrypted);

    return exitCode;
}
