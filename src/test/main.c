#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <why2/encrypter.h>
#include <why2/decrypter.h>

#define TEST_TEXT "Pepa smrdí."
#define TEST_KEY "lZwOBFvjJEmaYRIaKsALKLkSeJvXhFPbZIRNFbjQRNyiOuLTexhgOpObHzyQgNT"

int
main(int argc, char *argv[])
{
    if (argc == 2 && strcmp(argv[1], "skipCheck") == 0)
    {
        #undef SKIP_CHECK
        #define SKIP_CHECK 0
    }

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
