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
} node_t; //SINGLE LINKED LIST

node_t *head = NULL;

void push_to_list(void *pointer)
{
    //CREATE NODE
    node_t *new_node = malloc(sizeof(node_t));
    node_t *buffer = head;

    new_node -> pointer = pointer;
    new_node -> next = NULL;

    if (head == NULL) //INIT LIST
    {
        head = new_node;
    } else
    {
        while (buffer -> next != NULL) buffer = buffer -> next; //GET TO THE END OF LIST

        buffer -> next = new_node; //LINK
    }
}

void remove_node(node_t *node)
{
    /*
        This whole function assumes the node is in the list, so if you use this without pushing it, bad things will happen :)
    */

    node_t *buffer_1 = head;
    node_t *buffer_2;

    while (buffer_1 -> next != NULL) //GO TROUGH EVERY ELEMENT IN LIST
    {
        if (buffer_1 == node) break; //FOUND (IF THE WHILE GOES TROUGH THE WHOLE LIST WITHOUT THE break, I ASSUME THE LAST NODE IS THE CORRECT ONE)

        buffer_1 = buffer_1 -> next;
    }

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

    //DEALLOCATION
    free(node);
}

node_t *get_node(void *string)
{
    node_t *buffer = head;
    while (buffer -> next != NULL)
    {
        if (buffer -> pointer == string) return buffer;

        buffer = buffer -> next;
    }

    return buffer;
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
    node_t *node = get_node(pointer);

	if (pointer == node)
    {
        remove_node(node); //REMOVE FROM LIST IF FOUND
    } //TODO: ELSE happens really often

	free(pointer);
}