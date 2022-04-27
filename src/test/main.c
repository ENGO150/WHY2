#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "../../include/encrypter.h"
#include "../../include/decrypter.h"

#define TEST_TEXT "Pepa smrdí."
#define TEST_KEY "lZwOBFvjJEmaYRIaKsALKLkSeJvXhFPbZIRNFbjQRNyiOuLTexhgOpObHzyQgNT"

int
main(int args, char *argv[])
{
    char *text = encryptText(TEST_TEXT, TEST_KEY);
    text = decryptText(text, TEST_KEY);

    if (strcmp(text, TEST_TEXT) == 0)
    {
        printf("Test successful!\n");
    }
    else
    {
        fprintf(stderr, "Test failed!\n");
        exit(1);
    }

	free(text);
    return 0;
}
