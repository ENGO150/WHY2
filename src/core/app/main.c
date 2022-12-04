/*
This is part of WHY2
Copyright (C) 2022 Václav Šmejkal

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

#include <stdio.h>

#include <why2.h>

int main(void)
{
    //SET FLAGS
    setFlags((inputFlags) {1, 1, 0});

    //RUN ENCRYPTION WITH TEXT_TO_ENCRYPT, GENERATE NEW KEY AND DO NOT CHECK FOR ACTIVE VERSION & PREVENT ANY OUTPUT
    outputFlags encryptedText = encryptText(TEXT_TO_ENCRYPT, NULL);

    //SIMPLE TEXT
    printf
    (
        "Hi.\n"
        "This is a simple application written using WHY2 Encryption System.\n\n"

        "\"%s\" => \"%s\"\n\n"

        "If you'd like to know more about WHY2 Encryption System, please visit: https://github.com/ENGO150/WHY2/wiki \b\n"
        "Thank you so much for supporting this project!\n"

        , TEXT_TO_ENCRYPT, encryptedText.outputText
    );

    //DEALLOCATION
    deallocateOutput(encryptedText);

    return 0;
}