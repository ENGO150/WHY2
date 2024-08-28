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

#ifndef WHY2_MISC_H
#define WHY2_MISC_H

#ifdef __cplusplus
extern "C" {
#endif

#include <sys/time.h>

#include <why2/flags.h>

void why2_generate_text_key_chain(char *key, int *text_key_chain, int text_key_chain_size); //GENERATES ARRAY FOR ENCRYPTION/DECRYPTION
char *why2_generate_key(int key_length); //GENERATE ENCRYPTION KEY
void why2_deallocate_output(why2_output_flags flags); //DEALLOCATES flags
enum WHY2_EXIT_CODES why2_check_version(void); //THIS FUNCTION CHECKS IF LATEST WHY2_VERSION OF WHY2 IS USED
enum WHY2_EXIT_CODES why2_check_key(char *key); //CHECKS IF KEY IS VALID
enum WHY2_EXIT_CODES why2_check_text(char *text); //CHECKS IF TEXT IS VALID
unsigned long why2_count_int_length(int number); //RETURNS LENGTH OF number
unsigned long why2_count_unused_key_size(char *text, char *key); //COUNT unused_key_size
unsigned long why2_count_repeated_key_size(char *text, char *key); //COUNT repeated_key_size
unsigned long why2_compare_time_micro(struct timeval startTime, struct timeval finishTime); //COMPARE TIMES IN MICROSECONDS
void why2_die(char *exit_message); //PRINTS exit_message ERROR AND EXITS WITH CODE 1
char *why2_replace(char *string, char *old, char *new); //REPLACES old IN string WITH new
unsigned short why2_byte_format_length(char *s); //GET LENGTH OF s OF WHY2_OUTPUT_BYTE FORMAT

#ifdef __cplusplus
}
#endif

#endif
