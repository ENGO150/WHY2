/*
    bro this whole file is fucked up af...

    ...or maybe I am...
*/

#include <why2/memory.h>

#include <stdio.h>
#include <stdlib.h>

//LOCAL
typedef struct node
{
    void *pointer;
    struct node *next;
    struct node *last;
} node_t; //DOUBLY LINKED LIST

node_t *head = NULL; //TODO: Find a way to deallocate this shit at the end

void push_to_list(void *pointer)
{
    node_t *new_node = malloc(sizeof(node_t)); //CREATE ANOTHER ELEMENT
    node_t *node_buffer_1;

    new_node -> pointer = pointer; //ALLOCATED POINTER
    new_node -> next = NULL; //NEXT NODE
    new_node -> last = NULL; //LAST NODE

    if (head == NULL) //ALL MEMORY IS FREED (THERE AREN'T ANY NODES)
    {
        head = malloc(sizeof(node_t)); //INIT HEAD

        new_node -> last = head;

        head -> pointer = NULL;
        head -> next = new_node; //LINK TO new_node
    } else
    {
        node_buffer_1 = head;

        while (node_buffer_1 -> next != NULL) //GET 'LAST' NODE (next POINTER)
        {
            node_buffer_1 = node_buffer_1 -> next;
        }

        //LINK
        node_buffer_1 -> next = new_node;
        new_node -> last = node_buffer_1;
    }
}

void remove_node(node_t *node)
{
    //REMOVE NODE
    node -> last -> next = node -> next; //last WILL BE NEVER NULL CUZ CAN ONLY BE ANOTHER NODE OR HEAD
    if (node -> next != NULL)
    {
        node -> next -> last = node -> last;
    }

    //DEALLOCATION
    free(node); //TODO: Valgrind says node is still reachable, but it is deallocated here (I think)
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

void why2_free(void *pointer)
{
    //VARIABLES
    node_t *node_buffer = head;

    //FIND pointer IN LINKED LIST
    while (node_buffer -> next != NULL) //if pointer won't be found, nothing will happen | idk if I wanna fix this or leave it like this
    {
        node_buffer = node_buffer -> next;
        if (node_buffer -> pointer == pointer) break; //FOUND
    }

	remove_node(node_buffer);
	free(pointer);
}