/*
    bro this whole file is fucked up af...

    ...or maybe I am...
*/

#include <why2/memory.h>

#include <stdlib.h>
#include <string.h>

#include <why2/flags.h>

//LOCAL
typedef struct node
{
    void *pointer;
    char *identifier;
    struct node *next;
} node_t; //SINGLE LINKED LIST

node_t *head = NULL;

void push_to_list(void *pointer)
{
    //CREATE NODE
    node_t *new_node = malloc(sizeof(node_t));
    node_t *buffer = head;

    new_node -> pointer = pointer;
    new_node -> identifier = why2_get_memory_identifier();
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
    node_t *buffer_1 = head;
    node_t *buffer_2;

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

    //DEALLOCATION
    free(node);
}

node_t *get_node(void *pointer)
{
    node_t *buffer = head;
    while (buffer -> next != NULL)
    {
        if (buffer -> pointer == pointer) return buffer;

        buffer = buffer -> next;
    }

    if (pointer != buffer -> pointer) buffer = NULL; //PREVENT FROM RETURNING INVALID NODE

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
    if (pointer != NULL) why2_free(pointer);

    void *allocated = malloc(size);

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

	if (node != NULL) remove_node(node); //REMOVE FROM LIST IF FOUND

	free(pointer);
}

void why2_clean_memory(char *identifier)
{
    if (head == NULL) return; //LIST IS EMPTY

    node_t *buffer = head;

    while (buffer -> next != NULL) //GO TROUGH LIST
    {
        if (buffer -> identifier == identifier) remove_node(buffer);

        buffer = buffer -> next;
    }

    if (buffer -> identifier == identifier) remove_node(buffer); //LAST NODE
}