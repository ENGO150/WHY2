#include <why2/memory.h>

#include <stdlib.h>

void *why2_malloc(unsigned long size)
{
    //TODO: Add linked list

    return malloc(size);
}