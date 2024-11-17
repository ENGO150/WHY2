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

#include <why2/chat/flags.h>

#include <unistd.h>
#include <termios.h>

why2_bool asking_password = 0;

void __why2_set_asking_password(why2_bool value)
{
    asking_password = value;

    struct termios tty;
    tcgetattr(STDIN_FILENO, &tty); //GET ATTRS

    if (!value)
    {
        tty.c_lflag |= ECHO; //DISABLE
    } else
    {
        tty.c_lflag &= ~ECHO; //ENABLE
    }

    tcsetattr(STDIN_FILENO, TCSANOW, &tty); //SET ATTRS
}

why2_bool __why2_get_asking_password()
{
    return asking_password;
}