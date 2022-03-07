#include <stdio.h>

#include "../../include/encrypter.h"
#include "../../include/decrypter.h"

int
main(int args, char *argv[])
{
    char *text = encryptText("Pepa smrdi.", NULL);

    printf("%s\n", text);

    text = decryptText(text, "someRandomKeyLulw");

    printf("%s\n", text);
    return 0;
}