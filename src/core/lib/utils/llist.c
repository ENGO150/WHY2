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

#include <why2/memory.h>

void why2_list_push(why2_list_t *list, void *value, unsigned long size)
{
    //CREATE NODE
    why2_node_t *head = list -> head;
    why2_node_t *new_node = why2_malloc(sizeof(why2_node_t));
    new_node -> value = why2_malloc(size);
    why2_node_t *buffer = head;

    //INSERT DATA
    memcpy(new_node -> value, value, size);
    new_node -> next = NULL;

    if (head == NULL) //INIT LIST
    {
        buffer = new_node;
    } else
    {
        why2_node_t *buffer_2 = buffer;

        while (buffer -> next != NULL) buffer = buffer -> next; //GET TO THE END OF LIST

        buffer -> next = new_node; //LINK
        buffer = buffer_2; //GO BACK TO THE START OF THE LLIST
    }

    //APPEND THE new_node TO THE END OF list
    list -> head = buffer;
}

void why2_list_push_at(why2_list_t *list, unsigned long index, void *value, unsigned long size)
{
    //CREATE NODE
    why2_node_t *head = list -> head;
    why2_node_t *new_node = why2_malloc(sizeof(why2_node_t));
    new_node -> value = why2_malloc(size);

    //INSERT DATA
    memcpy(new_node -> value, value, size);

    if (index != 0 && head != NULL) //ISN'T FIRST
    {
        why2_node_t *node_before = head;
        for (unsigned long j = 0; j < index - 1 && j < why2_list_get_size(list); j++) node_before = node_before -> next; //SCROLL TO THE POSITION

        new_node -> next = node_before -> next; //SEW THE LIST BACK
        node_before -> next = new_node;
    } else //ADD BEFORE THE WHOLE LIST
    {
        new_node -> next = head;

        list -> head = new_node;
    }
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
    why2_deallocate(node -> value);
    why2_deallocate(node);
}

void why2_list_remove_at(why2_list_t *list, unsigned long index)
{
    if (list -> head == NULL) return; //EMPTY LIST

    why2_node_t *node_to_remove;

    if (index != 0) //SHOULDN'T BE FIRST
    {
        why2_node_t *node_before = list -> head;
        for (unsigned long j = 0; j < index - 1; j++) node_before = node_before -> next; //SCROLL TO THE POSITION

        node_to_remove = node_before -> next;
        node_before -> next = node_to_remove -> next;
    } else //ADD BEFORE THE WHOLE LIST
    {
        node_to_remove = list -> head;
        list -> head = node_to_remove -> next;
    }

    why2_deallocate(node_to_remove -> value);
    why2_deallocate(node_to_remove);
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

    why2_deallocate(deallocating_node -> value);
    why2_deallocate(deallocating_node);
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

unsigned long why2_list_get_size(why2_list_t *list)
{
    unsigned long n = 0; //RETURNING SIZE

    why2_node_t *head = list -> head;
    if (head == NULL) return n; //EMPTY LIST

    why2_node_t *buffer = head;

    do
    {
        n++;
        buffer = buffer -> next; //ITER
    } while (buffer != NULL);

    return n;
}

void why2_list_reverse(why2_list_t *list, unsigned long size)
{
    if (list -> head == NULL) return; //LIST IS EMPTY

    why2_list_t reversed_list = WHY2_LIST_EMPTY;
    why2_node_t *buffer = list -> head;
    why2_node_t *buffer2;

    //REVERSE INTO reversed_list AND DEALLOCATE list
    do
    {
        //COPY
        why2_node_t *current_node = why2_malloc(sizeof(why2_node_t));
        current_node -> value = why2_malloc(size);
        memcpy(current_node -> value, buffer -> value, size);

        //INSERT INTO reversed_list
        current_node -> next = reversed_list.head; //CHANGE NEXT POINTER
        reversed_list.head = current_node; //INSERT

        buffer2 = buffer;
        buffer = buffer -> next; //ITER

        //DEALLOCATE
        why2_deallocate(buffer2 -> value);
        why2_deallocate(buffer2);
    } while (buffer != NULL);

    //SET list TO reversed_list
    list -> head = reversed_list.head;
}