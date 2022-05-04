#include <stdio.h>
#include <stdlib.h>

#include <why2/encrypter.h>
#include <why2/flags.h>

#define TEXT_TO_ENCRYPT "Some text yk"

int main(void)
{
    setNoOutput(1);
    char *encryptedText = encryptText(TEXT_TO_ENCRYPT, NULL);

    printf
    (
        "Hi.\n"
        "This is an simple application written using WHY2 Encryption System.\n\n"

        "\"%s\" => \"%s\"\n\n"

        "If you'd like to know more about WHY2 Encryption System, please visit: https://github.com/ENGO150/WHY2/wiki \n"
        "Thank you so much for supporting this project!\n"

        , TEXT_TO_ENCRYPT, encryptedText
    );

    free(encryptedText);
    return 0;
}