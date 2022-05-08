#include <stdio.h>
#include <stdlib.h>

#include <why2/encrypter.h>
#include <why2/flags.h>
#include <why2/misc.h>

int main(void)
{
    inputFlags flags =
    {
        1, //SKIP CHECK
        1 //NO OUTPUT
    };

    outputFlags encryptedText = encryptText(TEXT_TO_ENCRYPT, NULL, flags);

    printf
    (
        "Hi.\n"
        "This is an simple application written using WHY2 Encryption System.\n\n"

        "\"%s\" => \"%s\"\n\n"

        "If you'd like to know more about WHY2 Encryption System, please visit: https://github.com/ENGO150/WHY2/wiki \n"
        "Thank you so much for supporting this project!\n"

        , TEXT_TO_ENCRYPT, encryptedText.outputText
    );

    //DEALLOCATION
    deallocateOutput(encryptedText);

    return 0;
}