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

#include <sys/time.h>

#include <why2/flags.h>

void generateTextKeyChain(char *key, int *textKeyChain, int textKeyChainSize); //GENERATES ARRAY FOR ENCRYPTION/DECRYPTION
void generateKey(char *key, int keyLength); //GENERATE ENCRYPTION KEY
void deallocateOutput(outputFlags flags); //DEALLOCATES flags
enum EXIT_CODES checkVersion(void); //THIS FUNCTION CHECKS IF LATEST VERSION OF WHY2 IS USED
enum EXIT_CODES checkKey(char *key); //CHECKS IF KEY IS VALID
enum EXIT_CODES checkText(char *text); //CHECKS IF TEXT IS VALID
unsigned long countIntLength(int number); //RETURNS LENGTH OF number
unsigned long countUnusedKeySize(char *text, char *key); //COUNT unusedKeySize
unsigned long countRepeatedKeySize(char *text, char *key); //COUNT repeatedKeySize
unsigned long compareTimeMicro(struct timeval startTime, struct timeval finishTime); //COMPARE TIMES IN MICROSECONDS

#endif
