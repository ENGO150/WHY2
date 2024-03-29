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

#include <why2/llist.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void why2_list_push(why2_list_t *list, void *value, unsigned long size)
{
    //CREATE NODE
    why2_node_t *head = list -> head;
    why2_node_t *new_node = malloc(sizeof(why2_node_t));
    new_node -> value = malloc(size);
    why2_node_t *buffer = head;

    //INSERT DATA
    memcpy(new_node -> value, value, size);
    new_node -> next = NULL;

    if (head == NULL) //INIT LIST
    {
        buffer = new_node;
    } else
    {
        while (buffer -> next != NULL) buffer = buffer -> next; //GET TO THE END OF LIST

        buffer -> next = new_node; //LINK
    }

    list -> head = buffer;
}

void why2_list_remove(why2_list_t *list, why2_node_t *node)
{
    if (node == NULL) return; //NULL NODE

    why2_node_t *head = list -> head;
    why2_node_t *buffer_1 = head;
    why2_node_t *buffer_2;

    while (buffer_1 -> next != NULL) //GO TROUGH EVERY ELEMENT IN LIST
    {
        if (buffer_1 == node) break; //FOUND (IF THE WHILE GOES TROUGH THE WHOLE LIST WITHOUT THE break, I ASSUME THE LAST NODE IS THE CORRECT ONE)

        buffer_1 = buffer_1 -> next;
    }

    if (node != buffer_1) return; //node WASN'T FOUND

    if (buffer_1 == head) //node WAS THE FIRST NODE IN THE LIST
    {
        //UNLINK
        head = buffer_1 -> next;
    } else //wdyt
    {
        //GET THE NODE BEFORE node
        buffer_2 = head;

        while (buffer_2 -> next != buffer_1) buffer_2 = buffer_2 -> next;

        //UNLINK
        buffer_2 -> next = buffer_1 -> next;
    }

    list -> head = head;

    //DEALLOCATION
    free(node -> value);
    free(node);
}

void why2_list_remove_back(why2_list_t *list)
{
    why2_node_t *head = list -> head;
    if (head == NULL) return; //EMPTY LIST

    why2_node_t *buffer = head;
    why2_node_t *deallocating_node;

    if (buffer -> next == NULL) //ONLY ONE NODE
    {
        deallocating_node = buffer;

        list -> head = NULL;
    } else
    {
        while (buffer -> next -> next != NULL) buffer = buffer -> next; //GO TO THE NODE BEFORE END

        deallocating_node = buffer -> next;

        buffer -> next = NULL; //UNLINK
    }

    free(deallocating_node -> value);
    free(deallocating_node);
}

why2_node_t *why2_list_find(why2_list_t *list, void *value)
{
    why2_node_t *head = list -> head;
    if (head == NULL) return NULL; //EMPTY LIST

    why2_node_t *buffer = head;

    while (buffer -> next != NULL)
    {
        if (buffer -> value == value) return buffer;

        buffer = buffer -> next;
    }

    if (value != buffer -> value) buffer = NULL; //PREVENT FROM RETURNING INVALID NODE

    return buffer;
}