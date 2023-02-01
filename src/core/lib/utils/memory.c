/*
    bro this whole file is fucked up af...

    ...or maybe I am...
*/

#include <why2/memory.h>

#include <stdlib.h>
#include <string.h>

//LOCAL
typedef struct node
{
    void *pointer;
    struct node *next;
    struct node *last;
} node_t; //DOUBLY LINKED LIST

node_t *head = NULL;

void push_to_list(void *pointer)
{
    //CREATE NODE
    node_t *new_node = malloc(sizeof(node_t));
    node_t *node_buffer = head;

    //ADD STUFF TO new_node
    new_node -> pointer = pointer;
    new_node -> next = NULL;
    new_node -> last = NULL;

    if (head == NULL) //FIRST NODE
    {
        head = new_node;
    } else //ADD AT THE END
    {
        while (node_buffer -> next != NULL) node_buffer = node_buffer -> next; //GO TO THE LAST NODE

        //LINK AT THE END
        node_buffer -> next = new_node;
        new_node -> last = node_buffer;
    }
}

void remove_node(node_t *node) //valgrind says this causes memory leaks ('still reachable'), but there is no chance like wtf
{
    //REMOVE NODE
    node_t *node_buffer = head;

    while (node_buffer -> next != NULL) //GO TROUGH THE LIST
    {
        if (node_buffer == node) break; //FOUND

        node_buffer = node_buffer -> next;
    }

    if (node -> last != NULL)
    {
        node -> last -> next = node -> next;
    } else
    {
        head = node -> next;
    }

    if (node -> next != NULL)
    {
        node -> next -> last = node -> last;
    } else
    {
         if (node -> last != NULL)
         {
            node -> last -> next = NULL;
         } else //LIST IS EMPTY NOW => DEALLOCATE
         {
            free(head);
            head = NULL;
         }
    }

    //DEALLOCATION
    free(node);
}

//GLOBAL
void *why2_malloc(unsigned long size)
{
    void *allocated = malloc(size);

    push_to_list(allocated);

    return allocated;
}

void *why2_calloc(unsigned long element, unsigned long size)
{
    void *allocated = calloc(element, size);

    push_to_list(allocated);

    return allocated;
}

void *why2_realloc(void *pointer, unsigned long size)
{
    void *allocated = realloc(pointer, size);

    push_to_list(allocated);

    return allocated;
}

char *why2_strdup(char *string)
{
    char *allocated = strdup(string);

    push_to_list(allocated);

    return allocated;
}

void why2_free(void *pointer)
{
    //VARIABLES
    node_t *node_buffer = head;

    //FIND pointer IN LINKED LIST
    while (node_buffer -> next != NULL)
    {
        if (node_buffer -> pointer == pointer) break; //FOUND

        node_buffer = node_buffer -> next;
    }

	if (pointer == node_buffer -> pointer) remove_node(node_buffer); //REMOVE FROM LIST IF FOUND
	free(pointer);
}