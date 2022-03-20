#include <stdio.h>
#include <stdlib.h>

#include "../../include/encrypter.h"
#include "../../include/decrypter.h"

int
main(int args, char *argv[])
{
    char *text = encryptText("Pepa smrdí.", "dsadhagsdhuhasvbdzgavdgasvgzduasvgzdavdhbashudbuas");
    printf("%s\n", text);

    text = decryptText(text, "dsadhagsdhuhasvbdzgavdgasvgzduasvgzdavdhbashudbuas");
    printf("%s\n", text);

	free(text);
    return 0;
}