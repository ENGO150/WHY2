#ifndef WHY2_MEMORY_H
#define WHY2_MEMORY_H

void *why2_malloc(unsigned long size);
void *why2_calloc(unsigned long element, unsigned long size);
void *why2_realloc(void *pointer, unsigned long size);

void why2_free(void *pointer);

#endif