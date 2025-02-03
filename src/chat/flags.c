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

#include <why2/memory.h>

why2_bool asking_password = 0;
why2_bool asking_username = 0;
why2_bool is_server = 0;
char *client_server_key = NULL;

void why2_chat_set_client_server_key(char *key)
{
    client_server_key = key;
}

char *why2_chat_get_client_server_key(void)
{
    return client_server_key;
}

void why2_chat_deallocate_client_server_key(void)
{
    why2_deallocate(client_server_key);
    client_server_key = NULL;
}

void __why2_chat_set_server(why2_bool value)
{
    is_server = value;
}

why2_bool __why2_chat_is_server(void)
{
    return is_server;
}

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

why2_bool __why2_get_asking_password(void)
{
    return asking_password;
}

void __why2_set_asking_username(why2_bool value)
{
    asking_username = value;
}

why2_bool __why2_get_asking_username(void)
{
    return asking_username;
}