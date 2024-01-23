/*
This is part of WHY2
Copyright (C) 2022 Václav Šmejkal

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

#ifndef WHY2_MEMORY_H
#define WHY2_MEMORY_H

#ifdef __cplusplus
extern "C" {
#endif

void *why2_malloc(unsigned long size);
void *why2_calloc(unsigned long element, unsigned long size);
void *why2_realloc(void *pointer, unsigned long size);

char *why2_strdup(char *string);

void *why2_fopen(char *name, char *modes);
void *why2_fdopen(int file, char *modes);
void *why2_opendir(char *name);

void why2_deallocate(void *pointer);

void why2_clean_memory(char *identifier); //identifier SPECIFIES WHICH NODES TO DEALLOCATE | THIS IS BASICALLY GARBAGE COLLECTOR | PASS why2_get_default_memory_identifier() FOR DEALLOCATING EVERYTHING

#ifdef __cplusplus
}
#endif

#endif