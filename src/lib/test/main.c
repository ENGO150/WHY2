#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <why2/encrypter.h>
#include <why2/decrypter.h>
#include <why2/flags.h>
#include <why2/misc.h>

int main(void)
{
    inputFlags flags =
    {
        1, //SKIP CHECK
        0, //NO OUTPUT
    };

    outputFlags encrypted = encryptText(TEST_TEXT, TEST_KEY, flags);
    encrypted = decryptText(encrypted.outputText, TEST_KEY, flags);

    if (strcmp(encrypted.outputText, TEST_TEXT) == 0)
    {
        printf("Test successful!\n");
    }
    else
    {
        fprintf(stderr, "Test failed!\n");
        exit(1);
    }

    //DEALLOCATION
    deallocateOutput(encrypted);

    return 0;
}
