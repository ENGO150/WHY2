#include <stdio.h>

#include <why2.h>

int main(void)
{
    //RUN ENCRYPTION WITH TEXT_TO_ENCRYPT, GENERATE NEW KEY AND DO NOT CHECK FOR ACTIVE VERSION & PREVENT ANY OUTPUT
    outputFlags encryptedText = encryptText(TEXT_TO_ENCRYPT, NULL, (inputFlags) {1, 1});

    //SIMPLE TEXT
    printf
    (
        "Hi.\n"
        "This is an simple application written using WHY2 Encryption System.\n\n"

        "\"%s\" => \"%s\"\n\n"

        "If you'd like to know more about WHY2 Encryption System, please visit: https://github.com/ENGO150/WHY2/wiki \b\n"
        "Thank you so much for supporting this project!\n"

        , TEXT_TO_ENCRYPT, encryptedText.outputText
    );

    //DEALLOCATION
    deallocateOutput(encryptedText);

    return 0;
}