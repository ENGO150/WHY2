#include <stdio.h>
#include <stdlib.h>

#include "../../include/encrypter.h"

int
main(int args, char * argv[])
{
    char *text = encryptText("TEXT");

    printf("%s\n", text);
    return 0;
}