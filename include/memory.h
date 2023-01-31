#ifndef WHY2_MEMORY_H
#define WHY2_MEMORY_H

#include <stdio.h> //TODO: Make the FILE* shit smarter | idk if it is possible

void *why2_malloc(unsigned long size);
void *why2_calloc(unsigned long element, unsigned long size);
void *why2_realloc(void *pointer, unsigned long size);

FILE *why2_fopen(char *file_name, char *modes);
FILE *why2_fdopen(int file, char *modes);
int open(char *file, int flags, ...);

void why2_free(void *pointer);

#endif