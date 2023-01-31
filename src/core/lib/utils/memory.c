/*
    bro this whole file is fucked up af...

    ...or maybe I am...
*/

#include <why2/memory.h>

#include <stdlib.h>

#include <why2/flags.h>

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

void remove_node(node_t *node)
{
    if (head == NULL) return; //LIST IS EMPTY

    //REMOVE NODE
    node_t *node_buffer = head;
    why2_bool found = 0;

    while (node_buffer -> next != NULL) //GO TROUGH THE LIST
    {
        if (node_buffer == node) //FOUND
        {
            found = 1;
            break;
        } //TODO: Many times it isn't found

        node_buffer = node_buffer -> next;
    }

    if (!found) return; //node WASN'T FOUND IN THE LIST

    if (node_buffer -> last != NULL) node_buffer -> last -> next = node_buffer -> next;
    if (node_buffer -> next != NULL) node_buffer -> next -> last = node_buffer -> last;

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