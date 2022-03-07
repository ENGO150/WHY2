#include <stdio.h>

#include "../../include/encrypter.h"
#include "../../include/decrypter.h"

int
main(int args, char * argv[])
{
    char *text = encryptText("ENGO WAS HERE");

    text = decryptText(text, "someRandomKeyLulw");

    printf("%s\n", text);
    return 0;
}