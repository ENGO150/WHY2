#include <stdio.h>

#include "../../include/encrypter.h"

int
main(int args, char * argv[])
{
    char *text = encryptText("ENGO WAS HERE");

    printf("%s\n", text);
    return 0;
}