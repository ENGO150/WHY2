#ifndef WHY2_MEMORY_H
#define WHY2_MEMORY_H

void *why2_malloc(unsigned long size);
void *why2_calloc(unsigned long element, unsigned long size);
void *why2_realloc(void *pointer, unsigned long size);

char *why2_strdup(char *string);

void *why2_fopen(char *name, char *modes);
void *why2_fdopen(int file, char *modes);
void *why2_opendir(char *name);

void why2_deallocate(void *pointer);

void why2_clean_memory(char *identifier); //identifier SPECIFIES WHICH NODES TO DEALLOCATE | THIS IS BASICALLY GARBAGE COLLECTOR | PASS why2_get_default_memory_identifier() FOR DEALLOCATING EVERYTHING

#endif