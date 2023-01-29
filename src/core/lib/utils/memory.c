#include <why2/memory.h>

#include <stdlib.h>

void *why2_malloc(unsigned long size)
{
    //TODO: Add linked list

    return malloc(size);
}

void *why2_calloc(unsigned long element, unsigned long size)
{
    //TODO: Add linked list

    return calloc(element, size);
}

void why2_free(void *pointer)
{
    //TODO: Add linked list

    free(pointer);
}