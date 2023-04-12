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

#ifndef WHY2_LLIST_H
#define WHY2_LLIST_H

typedef struct _why2_node
{
    void *value;
    struct _why2_node *next;
} why2_node_t; //SINGLE LINKED LIST

void why2_push(void *value); //PUSH ELEMENT TO LIST BACK
void why2_remove(why2_node_t *node); //REMOVE ELEMENT
why2_node_t *why2_find(void *value); //FIND ELEMENT IN LIST

#endif