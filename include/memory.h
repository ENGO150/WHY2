#ifndef WHY2_MEMORY_H
#define WHY2_MEMORY_H

void *why2_malloc(unsigned long size);
void *why2_calloc(unsigned long element, unsigned long size);
void *why2_realloc(void *pointer, unsigned long size);

char *why2_strdup(char *string);

void why2_free(void *pointer);

void why2_clean_memory(char *identifier); //identifier SPECIFIES WHICH NODES TO DEALLOCATE

#endif