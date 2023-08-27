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

#include <why2/chat/parser.h>

#include <stdio.h>
#include <string.h>

#include <yaml.h>

#include <why2/memory.h>
#include <why2/misc.h>

char *why2_yml_read(char *path, char *key)
{
    FILE *file = fopen(path, "r");

    yaml_parser_t parser;
    yaml_token_t token;
    int state = 0;  // 0: looking for key, 1: looking for value

    if (!yaml_parser_initialize(&parser)) why2_die("Failed to initialize parser.");

    yaml_parser_set_input_file(&parser, file);
    char *value = NULL;

    while (yaml_parser_scan(&parser, &token))
    {
        switch (token.type)
        {
            case YAML_SCALAR_TOKEN:
                if (state == 0 && strcmp((char*) token.data.scalar.value, key) == 0)
                {
                    state = 1;
                } else if (state == 1)
                {
                    value = why2_strdup((char*) token.data.scalar.value);
                    state = 0;  // Reset state for the next key-value pair
                }

                break;

            case YAML_STREAM_END_TOKEN:
                goto skip;

            default:
                break;
        }

        yaml_token_delete(&token); //TODO: Possible memory leak (or maybe not i have no fucking idea)
    }

    skip:

    return value;
}
